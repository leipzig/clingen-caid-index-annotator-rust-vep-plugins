// Registry processes: query windows, region enumeration and site lookups. ENUMERATE and LOOKUP
// are the only processes that talk to the Registry, read-only (GET a region, POST a lookup),
// one request stream each (maxForks 1).

process REGIONS {
    tag "${chroms.size() == 1 ? chroms[0] : 'all'}"
    label 'small'

    input:
    path inputs      // [ncbiRefSeq.txt.gz, knownGene.txt.gz] for a named set, or the BED file
    val chroms       // the run's chromosomes: one, or all 25
    val windowing    // [set: cds|pc_exons|exons|genes|bed, pad:, merge_gap:, max_window:, flank:, per_task:]

    output:
    // windows_<chrom>_<NNNN>.tsv: at most per_task windows of one chromosome each, no header,
    // columns chrom accession begin end core_begin core_end. One ENUMERATE task per file.
    path 'windows/windows_*.tsv', arity: '1..*', emit: windows
    path 'windows/summary.tsv', emit: summary

    script:
    // settings() gives one chromosome or every primary one, which is what regions' --chrom takes.
    def chrom = chroms.size() == 1 ? chroms[0] : 'all'
    def from = windowing.set == 'bed'
        ? "--bed '${inputs}'"
        : "--refseq '${inputs[0]}' --gencode '${inputs[1]}' --set ${windowing.set}"
    """
    clingen-index regions ${from} \\
        --pad ${windowing.pad} --merge-gap ${windowing.merge_gap} --max-window ${windowing.max_window} \\
        --flank ${windowing.flank} --chrom ${chrom} --batch ${windowing.per_task} --out-dir windows
    """

    stub:
    """
    mkdir windows
    for c in ${chroms.join(' ')}; do : > windows/windows_\${c}_0000.tsv; done
    printf 'chrom\\tintervals\\tmerged\\twindows\\tcore_bp\\tquery_bp\\n' > windows/summary.tsv
    """
}

process ENUMERATE {
    tag "${windows.baseName}"
    label 'network'
    label 'small'
    maxForks 1       // one request stream to the Registry (also set in nextflow.config)

    input:
    tuple val(chrom), path(windows)   // one windows_<chrom>_<NNNN>.tsv
    val paging                        // [page_size:, split_overlap:]
    val keep_reference                // true: enumerate --keep-reference (--include_reference)

    output:
    tuple val(chrom), path("enum_${windows.baseName.replace('windows_', '')}.tsv"), emit: pairs   // HGVS<TAB>CAID
    // The UTC day (YYYY-MM-DD) the task asked the Registry: the manifest's registry source takes
    // the earliest. A cached task keeps the day it ran, so a -resume does not re-date its answers.
    env('day'), emit: day

    script:
    // A String 'false' (the command line's, or a params file's) is true in a condition.
    if (!(keep_reference instanceof Boolean)) {
        error("ENUMERATE: keep_reference must be a Boolean, not ${keep_reference.getClass().simpleName} '${keep_reference}'")
    }
    // enumerate writes <out>.partial and renames it once every window is in; day is taken only
    // if it succeeds, as Nextflow reads env outputs after the script exits 0.
    """
    day=\$(date -u +%F)
    clingen-index registry enumerate --windows ${windows} --out enum_${windows.baseName.replace('windows_', '')}.tsv \\
        --page-size ${paging.page_size} --split-overlap ${paging.split_overlap}${keep_reference ? ' --keep-reference' : ''} \\
        --base-url '${params.registry_url}'
    """

    stub:
    """
    day=\$(date -u +%F)
    : > enum_${windows.baseName.replace('windows_', '')}.tsv
    """
}

process LOOKUP {
    tag "${batch.baseName}"
    label 'network'
    label 'small'
    maxForks 1       // one request stream to the Registry (also set in nextflow.config)

    input:
    tuple val(chrom), path(batch)     // one lookup_<chrom>_<n>.tsv: CHROM POS REF ALT

    output:
    // The 7 harvest columns: CHROM POS REF ALT CAID HGVS_GRCh38 HGVS_GRCh37
    tuple val(chrom), path("answers_${batch.baseName.replace('lookup_', '')}.tsv"), emit: answers
    // The UTC day the task asked the Registry: the manifest's harvest date is the earliest day of
    // every ENUMERATE and LOOKUP task, each kept as it ran when a -resume reuses the task.
    env('day'), emit: day

    script:
    // lookup writes <out>.partial and renames it once every site is answered.
    """
    day=\$(date -u +%F)
    clingen-index registry lookup --sites ${batch} --out answers_${batch.baseName.replace('lookup_', '')}.tsv \\
        --base-url '${params.registry_url}'
    """

    stub:
    """
    day=\$(date -u +%F)
    : > answers_${batch.baseName.replace('lookup_', '')}.tsv
    """
}
