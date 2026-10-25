// Index processes: harvest assembly, the genome, the key shards, the index, the checks
// that gate publishing, and the report.
// Until their scripts are written, each one refuses a real run; `-stub` runs work.

process SPLIT_CACHE {
    label 'small'

    input:
    path cache_dirs, stageAs: 'cache*'   // the --cache directories, numbered so equal names do not collide
    val chroms                           // the run's chromosomes

    output:
    // cache_<chrom>.tsv for every requested chromosome: registered harvest rows. Always a list,
    // even of one file, so the workflow can flatMap it (flatten is deprecated since Nextflow 26.04).
    path 'cache_*.tsv', arity: '1..*', emit: cache

    script:
    """
    echo 'SPLIT_CACHE has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    """
    for c in ${chroms.join(' ')}; do : > cache_\$c.tsv; done
    """
}

process MERGE_SITES {
    tag "${chrom}"
    label 'sort'

    input:
    // Every site list of one chromosome (numbered, as each gnomAD stream writes sites.tsv),
    // and its cached answers (or [] without --cache).
    tuple val(chrom), path(sites, stageAs: 'sites/sites_*.tsv'), path(cache)
    val batch_size   // sites per LOOKUP task

    output:
    tuple val(chrom), path("cached_${chrom}.tsv"), emit: cached                         // answers reused from the cache
    tuple val(chrom), path("lookup_${chrom}_*.tsv", arity: '0..*'), emit: batches      // sites still to look up

    script:
    """
    echo 'MERGE_SITES has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    """
    : > cached_${chrom}.tsv
    : > lookup_${chrom}_00.tsv
    """
}

process COLLECT {
    tag "${chrom}"
    label 'sort'
    publishDir "${params.outdir}", mode: 'copy'   // harvest/<chrom>.tsv.gz: the next run's --cache

    input:
    tuple val(chrom), path(cached), path(answers, stageAs: 'answers/*')

    output:
    tuple val(chrom), path("harvest/${chrom}.tsv.gz"), emit: harvest   // the 7 harvest columns, sorted by POS

    script:
    """
    echo 'COLLECT has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    """
    mkdir harvest
    printf '' | gzip -c > harvest/${chrom}.tsv.gz
    """
}

process BUILD_GENOME {
    label 'genome'

    input:
    path fasta       // hg38.fa.gz
    val chroms       // the run's chromosomes: the genome holds only these

    output:
    tuple path('genome/GRCh38.bin'), path('genome/GRCh38.bin.idx'), emit: genome

    script:
    """
    echo 'BUILD_GENOME has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    """
    mkdir genome
    : > genome/GRCh38.bin
    : > genome/GRCh38.bin.idx
    """
}

process KEYS {
    tag "${chrom}"
    label 'sort'

    input:
    // The chromosome's enumerated pairs (none when it had no windows) and its harvest.
    tuple val(chrom), val(accession), path(enums, stageAs: 'enum/*'), path(harvest)

    output:
    // <accession>.tsv: HGVS<TAB>CAID in byte order, so the shards concatenated in the byte
    // order of their names are in index order.
    path "${accession}.tsv", emit: shard
    // keys_<chrom>.tsv: header and one row: chrom accession enum_files enumerated harvested keys
    path "keys_${chrom}.tsv", emit: stats

    script:
    """
    echo 'KEYS has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    """
    : > ${accession}.tsv
    n=\$(find enum -name '*.tsv' 2>/dev/null | wc -l | tr -d ' ')
    printf 'chrom\\taccession\\tenum_files\\tenumerated\\tharvested\\tkeys\\n' > keys_${chrom}.tsv
    printf '%s\\t%s\\t%s\\t0\\t0\\t0\\n' ${chrom} ${accession} \$n >> keys_${chrom}.tsv
    """
}

process BUILD_INDEX {
    label 'build'

    input:
    path shards, stageAs: 'shards/*'     // every <accession>.tsv; read in the byte order of their names
    tuple path(genome_bin, stageAs: 'genome/*'), path(genome_idx, stageAs: 'genome/*')
    path provenance, stageAs: 'provenance/*'   // ucsc_sources.tsv, clinvar_date.txt, gnomad_files.tsv: for the sources

    output:
    path 'index', emit: index            // caid.fst and manifest.json

    script:
    """
    echo 'BUILD_INDEX has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    """
    mkdir index
    : > index/caid.fst
    echo '{}' > index/manifest.json
    """
}

process VALIDATE {
    tag "${chrom}"
    label 'small'
    publishDir "${params.outdir}/reports", mode: 'copy', pattern: 'validate_*.txt'

    input:
    tuple val(chrom), path(harvest)
    tuple path(genome_bin), path(genome_idx)

    output:
    // The report, and validate's exit status: 0 every row agrees, 1 a mismatch (the gate
    // FINALIZE applies). A run that could not complete fails the task instead.
    tuple val(chrom), path("validate_${chrom}.txt"), env('status'), emit: result

    script:
    """
    echo 'VALIDATE has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    """
    : > validate_${chrom}.txt
    status=0
    """
}

process REACH {
    tag "${chrom}"
    label 'small'
    publishDir "${params.outdir}/reports", mode: 'copy', pattern: 'reach_*.txt'

    input:
    tuple val(chrom), val(accession)     // reach walks the keys under '<accession>:'
    path index
    tuple path(genome_bin), path(genome_idx)

    output:
    // The report, and reach's exit status: 0 every key reachable, 1 some key not (the gate
    // FINALIZE applies). A run that could not complete fails the task instead.
    tuple val(chrom), path("reach_${chrom}.txt"), env('status'), emit: result

    script:
    """
    echo 'REACH has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    """
    : > reach_${chrom}.txt
    status=0
    """
}

process FINALIZE {
    label 'small'
    publishDir "${params.outdir}", mode: 'copy'

    input:
    path index
    path genome, stageAs: 'genome/*'     // the built genome, published beside the index; [] for a --genome given
    val statuses                         // '<validate|reach>:<chrom>:<exit status>' for every check
    path stats, stageAs: 'keys/*'        // keys_<chrom>.tsv: a chromosome with windows but no enumerated key fails
    val allow_mismatches                 // publish despite failed checks

    output:
    // The staged inputs themselves, published once every check passed.
    path 'index', includeInputs: true, emit: index
    path 'genome/*', includeInputs: true, arity: '0..*', emit: genome

    script:
    """
    echo 'FINALIZE has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    """
    # The outputs are the staged index and genome; a stub run checks nothing.
    true
    """
}

process REPORT {
    label 'small'
    publishDir "${params.outdir}", mode: 'copy'

    input:
    path checks, stageAs: 'checks/*'             // validate_<chrom>.txt and reach_<chrom>.txt
    path stats, stageAs: 'keys/*'                // keys_<chrom>.tsv
    path skipped, stageAs: 'skipped/skipped_*.tsv'   // each source's skipped.tsv
    path provenance, stageAs: 'provenance/*'     // ucsc_sources.tsv, clinvar_date.txt, gnomad_files.tsv
    path regions_summary, stageAs: 'regions_summary.tsv'

    output:
    path 'reports/coverage.tsv', emit: coverage
    path 'reports/summary.md', emit: summary
    path 'reports/sources.tsv', emit: sources
    path 'reports/skipped.tsv.gz', emit: skipped

    script:
    """
    echo 'REPORT has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    """
    mkdir reports
    : > reports/coverage.tsv
    : > reports/summary.md
    : > reports/sources.tsv
    printf '' | gzip -c > reports/skipped.tsv.gz
    """
}
