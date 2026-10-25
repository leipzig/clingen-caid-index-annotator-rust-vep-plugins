// Source processes: downloads and the per-chromosome site lists the index is built from.
// Until their scripts are written, each one refuses a real run; `-stub` runs work.

process RESOLVE_GNOMAD {
    tag "${version}"
    label 'network'
    label 'small'

    input:
    val version     // 'latest' or a release such as '4.1.1'
    val chroms      // the run's chromosomes, e.g. ['21']
    val datasets    // the gnomad_* entries of --datasets

    output:
    // One line per file to stream, no header: dataset chrom url md5_base64 size.
    // gnomAD v4 has no chrM, so MT lists v3.1's chrM genomes file and no exomes file.
    path 'gnomad_files.tsv', emit: files

    script:
    """
    echo 'RESOLVE_GNOMAD has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    // As a real listing: no exomes file for MT. The md5 is an empty file's.
    def rows = datasets.collectMany { d ->
        chroms.findAll { c -> !(d == 'gnomad_exomes' && c == 'MT') }.collect { c ->
            [d, c, "https://stub.invalid/${d}/${c}.vcf.bgz", '1B2M2Y8AsgTpgAmY7PhCfg==', '0'].join('\t') + '\n'
        }
    }
    """
    printf '%s' '${rows.join('')}' > gnomad_files.tsv
    """
}

process GNOMAD_SITES {
    tag "${dataset}:${chrom}"
    label 'network'
    label 'small'

    input:
    tuple val(dataset), val(chrom), val(url), val(md5)   // a gnomad_files.tsv line; the VCF is streamed, never stored

    output:
    tuple val(dataset), val(chrom), path('sites.tsv'), emit: sites   // CHROM POS REF ALT, no header
    path 'skipped.tsv', emit: skipped                                // alleles that cannot be looked up, with the reason

    script:
    """
    echo 'GNOMAD_SITES has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    """
    : > sites.tsv
    : > skipped.tsv
    """
}

process CLINVAR_SITES {
    label 'network'
    label 'small'

    input:
    path vcf        // --clinvar_vcf as a local file, or [] for 'latest': NCBI's weekly clinvar.vcf.gz is downloaded
    path md5file    // the .md5 beside the local file, or []
    val chroms      // the run's chromosomes

    output:
    // sites_<chrom>.tsv for every requested chromosome: CHROM POS REF ALT. Always a list, even
    // of one file, so the workflow can flatMap it (flatten is deprecated since Nextflow 26.04).
    path 'sites_*.tsv', arity: '1..*', emit: sites
    path 'skipped.tsv', emit: skipped             // records that cannot be looked up, with the reason
    path 'clinvar_date.txt', emit: date           // the VCF's ##fileDate, for the manifest's source label

    script:
    """
    echo 'CLINVAR_SITES has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    """
    for c in ${chroms.join(' ')}; do : > sites_\$c.tsv; done
    : > skipped.tsv
    echo 1970-01-01 > clinvar_date.txt
    """
}

process FETCH_UCSC {
    label 'network'
    label 'small'

    output:
    tuple path('ncbiRefSeq.txt.gz'), path('knownGene.txt.gz'), emit: tables
    path 'ucsc_sources.tsv', emit: sources        // each table's size and Last-Modified (UCSC publishes no md5 for them)

    script:
    """
    echo 'FETCH_UCSC has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    """
    printf '' | gzip -c > ncbiRefSeq.txt.gz
    printf '' | gzip -c > knownGene.txt.gz
    : > ucsc_sources.tsv
    """
}

process FETCH_FASTA {
    label 'network'
    label 'small'

    output:
    path 'hg38.fa.gz', emit: fasta                // checked against UCSC's md5sum.txt

    script:
    """
    echo 'FETCH_FASTA has no script yet; run with -stub' >&2
    exit 1
    """

    stub:
    """
    printf '' | gzip -c > hg38.fa.gz
    """
}
