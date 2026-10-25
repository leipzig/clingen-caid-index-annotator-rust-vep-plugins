// Registry processes: query windows, region enumeration and site lookups.
// Until their scripts are written, each one refuses a real run; `-stub` runs work.

process REGIONS {
    tag "${chroms.size() == 1 ? chroms[0] : 'all'}"
    label 'small'

    input:
    path inputs      // ncbiRefSeq.txt.gz and knownGene.txt.gz for a named set, or the BED file
    val chroms       // the run's chromosomes
    val windowing    // [set: cds|pc_exons|exons|genes|bed, pad:, merge_gap:, max_window:, flank:, per_task:]

    output:
    // windows_<chrom>_<NNNN>.tsv: at most per_task windows of one chromosome each, no header,
    // columns chrom accession begin end core_begin core_end. One ENUMERATE task per file.
    path 'windows/windows_*.tsv', arity: '1..*', emit: windows
    path 'windows/summary.tsv', emit: summary

    script:
    """
    echo 'REGIONS has no script yet; run with -stub' >&2
    exit 1
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

    output:
    tuple val(chrom), path("enum_${windows.baseName.replace('windows_', '')}.tsv"), emit: pairs   // HGVS<TAB>CAID

    script:
    """
    echo 'ENUMERATE has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    """
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

    script:
    """
    echo 'LOOKUP has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    """
    : > answers_${batch.baseName.replace('lookup_', '')}.tsv
    """
}
