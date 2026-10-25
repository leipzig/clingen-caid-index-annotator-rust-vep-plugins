// Source processes: downloads and the per-chromosome site lists the index is built from.
//
// Shared formats (TSV, no header, chromosomes in bare GRCh38 form: 21, X, MT):
//   sites      CHROM POS REF ALT, one ALT per line
//   skipped    dataset CHROM POS ID REF ALT reason: an allele that cannot be looked up, with
//              CHROM as the VCF writes it for a contig outside the primary assembly
//   sources    source url size last_modified md5 md5_check: what a download read (its own md5
//              always), and whether that md5 was checked against a published one ('checked',
//              'not checked', or 'none published')
// Every output is written as *.partial and renamed once complete, so a task that dies leaves
// no output that looks finished. Downloads follow redirects (-L): without it curl would save a
// redirect's body as the file and succeed, leaving the md5 check to fail it obscurely.

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
    def mirrors = [
        gcs: 'https://storage.googleapis.com/gcp-public-data--gnomad',
        aws: 'https://gnomad-public-us-east-1.s3.amazonaws.com',
    ]
    def mirror = mirrors[params.gnomad_mirror.toString()]
    if (!mirror) {
        error("--gnomad_mirror ${params.gnomad_mirror}: not one of ${mirrors.keySet().join(', ')}")
    }
    """
    listing='${params.gnomad_listing_url}'

    # list OUT PREFIX [DELIMITER]: every page of the bucket listing under PREFIX into OUT, as
    # 'P<TAB>prefix' lines (with a delimiter) and 'O<TAB>name<TAB>size<TAB>md5Hash' lines.
    # fields= keeps each item flat, so splitting the JSON at its braces leaves one item per line;
    # only names under PREFIX are kept, as a server that ignores the query (file:// in the tests)
    # sends them all. It writes a file rather than stdout so that it is never run inside \$(...)
    # or a condition, where bash ignores set -e and a failed request would read as no objects.
    list() {
        local out=\$1 prefix=\$2 delimiter=\${3:-} token=''
        : > "\$out"
        while :; do
            local query=(--data-urlencode "prefix=\$prefix" --data-urlencode 'fields=prefixes,items(name,size,md5Hash),nextPageToken')
            if [[ -n \$delimiter ]]; then query+=(--data-urlencode "delimiter=\$delimiter"); fi
            if [[ -n \$token ]]; then query+=(--data-urlencode "pageToken=\$token"); fi
            curl -sSL --fail --retry 5 -G "\${query[@]}" "\$listing" > page.json
            tr -d '\\n' < page.json | tr '{}' '\\n\\n' | awk -v prefix="\$prefix" '
                function field(s, key,    m) {
                    if (!match(s, "\\"" key "\\"[ \\t]*:[ \\t]*\\"[^\\"]*\\"")) return ""
                    m = substr(s, RSTART, RLENGTH)
                    sub("^\\"" key "\\"[ \\t]*:[ \\t]*\\"", "", m)
                    return substr(m, 1, length(m) - 1)
                }
                /"prefixes"/ {
                    s = substr(\$0, index(\$0, "\\"prefixes\\""))
                    s = substr(s, index(s, "[") + 1)
                    s = substr(s, 1, index(s, "]") - 1)
                    while (match(s, "\\"[^\\"]*\\"")) {
                        print "P\\t" substr(s, RSTART + 1, RLENGTH - 2)
                        s = substr(s, RSTART + RLENGTH)
                    }
                }
                /"nextPageToken"/ { print "T\\t" field(\$0, "nextPageToken") }
                /"name"/ {
                    name = field(\$0, "name")
                    if (index(name, prefix) == 1) print "O\\t" name "\\t" field(\$0, "size") "\\t" field(\$0, "md5Hash")
                }' > page.tsv
            awk -F'\\t' '\$1 != "T"' page.tsv >> "\$out"
            token=\$(awk -F'\\t' '\$1 == "T" { print \$2 }' page.tsv)
            if [[ -z \$token ]]; then break; fi
        done
    }

    version='${version}'
    if [[ \$version == latest ]]; then
        # The newest release with genomes sites files: 4.1.2 (2026) holds only constraint/, and
        # a release's vcf/genomes/ may hold no more than a directory placeholder.
        list releases.tsv release/ /
        versions=\$(awk -F'\\t' '\$1 == "P" && \$2 ~ /^release\\/[0-9]+(\\.[0-9]+)*\\/\$/ { split(\$2, p, "/"); print p[2] }' \\
                       releases.tsv | sort -t. -k1,1nr -k2,2nr -k3,3nr)
        version=''
        for v in \$versions; do
            list "genomes_\$v.tsv" "release/\$v/vcf/genomes/"
            if awk -F'\\t' '\$1 == "O" && \$2 ~ /\\.vcf\\.bgz\$/ { found = 1 } END { exit !found }' "genomes_\$v.tsv"; then
                version=\$v
                break
            fi
        done
        if [[ -z \$version ]]; then
            echo "RESOLVE_GNOMAD: no release under \$listing has release/<version>/vcf/genomes/*.vcf.bgz" >&2
            exit 1
        fi
        echo "gnomAD latest = \$version" >&2
    fi

    : > gnomad_files.tsv.partial
    for dataset in ${datasets.join(' ')}; do
        kind=\${dataset#gnomad_}   # genomes or exomes
        list "objects_\$kind.tsv" "release/\$version/vcf/\$kind/"
        for chrom in ${chroms.join(' ')}; do
            if [[ \$chrom == MT ]]; then
                # gnomAD v4 has no chrM: MT is v3.1's chrM genomes file, and there are no exomes.
                if [[ \$kind == exomes ]]; then continue; fi
                if [[ ! -e objects_chrM.tsv ]]; then list objects_chrM.tsv release/3.1/vcf/genomes/; fi
                objects=objects_chrM.tsv
                name=release/3.1/vcf/genomes/gnomad.genomes.v3.1.sites.chrM.vcf.bgz
            else
                objects="objects_\$kind.tsv"
                name="release/\$version/vcf/\$kind/gnomad.\$kind.v\$version.sites.chr\$chrom.vcf.bgz"
            fi
            if ! awk -F'\\t' -v OFS='\\t' -v name="\$name" -v dataset="\$dataset" -v chrom="\$chrom" -v base='${mirror}' '
                    \$1 == "O" && \$2 == name { print dataset, chrom, base "/" name, \$4, \$3; found = 1 }
                    END { exit !found }' "\$objects" >> gnomad_files.tsv.partial; then
                echo "RESOLVE_GNOMAD: \$dataset \$chrom: no \$name in the listing \$listing" >&2
                exit 1
            fi
        done
    done
    mv gnomad_files.tsv.partial gnomad_files.tsv
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
    # The md5 is taken from the bytes as they stream past, through a FIFO: a process
    # substitution, >(...), is not waited for, and bash 3.2 (macOS, host profile) cannot wait
    # for one, so got.md5 could be read before it is written. The FIFO lives in a local temp
    # directory, as the work directory may be on a filesystem without FIFOs (a FUSE mount).
    fifo_dir=\$(mktemp -d)
    trap 'rm -rf "\$fifo_dir"' EXIT
    mkfifo "\$fifo_dir/stream"
    openssl md5 -binary < "\$fifo_dir/stream" | base64 > got.md5 &
    digest=\$!

    # gnomAD writes chr21 and chrM; every file and column here uses the bare names 21 and MT.
    # Multi-ALT records are split (v4 records are split already).
    curl -sSL --fail --retry 5 '${url}' | tee "\$fifo_dir/stream" | gzip -dc | awk -F'\\t' -v OFS='\\t' \\
        -v dataset='${dataset}' -v chrom='${chrom}' -v url='${url}' '
        /^#/ { next }
        {
            c = \$1
            sub(/^chr/, "", c)
            if (c == "M") c = "MT"
            if (c != chrom) {
                printf "GNOMAD_SITES: %s line %d has CHROM %s, not chromosome %s\\n", url, NR, \$1, chrom > "/dev/stderr"
                exit 1
            }
            n = split(\$5, alts, ",")
            for (i = 1; i <= n; i++) {
                alt = alts[i]
                why = ""
                if (alt == "*") why = "star_allele"
                else if (alt == ".") why = "no_alt"
                else if ((\$4 alt) ~ /[^ACGT]/) why = "non_acgt"
                if (why != "") print dataset, c, \$2, \$3, \$4, alt, why > "skipped.tsv.partial"
                else print c, \$2, \$4, alt > "sites.tsv.partial"
            }
        }'
    wait \$digest

    got=\$(cat got.md5)
    if [[ \$got != '${md5}' ]]; then
        echo "GNOMAD_SITES: ${url} has md5 \$got; the GCS listing says ${md5}" >&2
        exit 1
    fi
    touch sites.tsv.partial skipped.tsv.partial   # a file with no site or no skip
    mv sites.tsv.partial sites.tsv
    mv skipped.tsv.partial skipped.tsv
    echo "${dataset} ${chrom}: \$(wc -l < sites.tsv) sites, \$(wc -l < skipped.tsv) skipped" >&2
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
    path md5file    // the .md5 beside the local file (or [] if there is none), or [] for 'latest'
    val chroms      // the run's chromosomes

    output:
    // sites_<chrom>.tsv for every requested chromosome: CHROM POS REF ALT. Always a list, even
    // of one file, so the workflow can flatMap it (flatten is deprecated since Nextflow 26.04).
    path 'sites_*.tsv', arity: '1..*', emit: sites
    path 'skipped.tsv', emit: skipped             // records that cannot be looked up, with the reason
    path 'clinvar_date.txt', emit: date           // the VCF's ##fileDate, for the manifest's source label
    path 'clinvar_sources.tsv', emit: sources     // what was read, its md5, and whether that was checked

    script:
    def fetch = !vcf
    """
    if ${fetch}; then
        url='${params.clinvar_url}'
        mkdir download
        curl -sSL --fail --retry 5 -o download/clinvar.vcf.gz.md5 "\$url.md5"
        curl -sSL --fail --retry 5 -D download/headers.txt -o download/clinvar.vcf.gz "\$url"
        vcf=download/clinvar.vcf.gz
        md5file=download/clinvar.vcf.gz.md5
        location=\$url
        modified=\$(awk 'tolower(\$1) == "last-modified:" { sub(/^[^:]*:[ \\t]*/, ""); sub(/\\r\$/, ""); m = \$0 }
                       END { print m }' download/headers.txt)
    else
        vcf='${vcf}'
        md5file='${md5file}'
        location=\$vcf
        modified=''
    fi

    got=\$(openssl dgst -md5 -r "\$vcf" | awk '{ print \$1 }')
    if [[ -n \$md5file ]]; then
        # NCBI's .md5: the md5, then a file name (a path on their servers)
        expected=\$(awk 'NR == 1 { print tolower(\$1) }' "\$md5file")
        if [[ \$got != "\$expected" ]]; then
            echo "CLINVAR_SITES: \$location has md5 \$got; \$md5file says \$expected" >&2
            exit 1
        fi
        check='checked'
    else
        check='not checked'
    fi

    # The rules of the README's ClinVar run: records on contigs outside the primary assembly,
    # with ALT=. or with a base other than A, C, G or T cannot be looked up. --chrom narrows
    # the skipped records too, so other contigs' records are reported only when every primary
    # chromosome is in the run. ClinVar writes one ALT per record; more are split anyway.
    for c in ${chroms.join(' ')}; do : > "sites_\$c.tsv.partial"; done
    : > skipped.tsv.partial
    gzip -dc "\$vcf" | awk -F'\\t' -v OFS='\\t' -v want='${chroms.join(',')}' -v location="\$location" '
        BEGIN {
            split("1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 X Y MT", p, " ")
            for (i in p) primary[p[i]] = 1
            split(want, w, ",")
            for (i in w) wanted[w[i]] = 1
            all = 1
            for (c in primary) if (!(c in wanted)) all = 0
        }
        /^##fileDate=/ { date = substr(\$0, 12); next }
        /^#/ { next }
        !(\$1 in primary) {
            if (all) print "clinvar", \$1, \$2, \$3, \$4, \$5, "non_primary_contig" > "skipped.tsv.partial"
            next
        }
        { on_primary++ }
        !(\$1 in wanted) { next }
        {
            n = split(\$5, alts, ",")
            for (i = 1; i <= n; i++) {
                alt = alts[i]
                why = ""
                if (alt == ".") why = "no_alt"
                else if ((\$4 alt) ~ /[^ACGT]/) why = "non_acgt"
                if (why != "") print "clinvar", \$1, \$2, \$3, \$4, alt, why > "skipped.tsv.partial"
                else print \$1, \$2, \$4, alt > ("sites_" \$1 ".tsv.partial")
            }
        }
        END {
            if (date !~ /^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]\$/) {
                printf "CLINVAR_SITES: %s has ##fileDate \\"%s\\", not YYYY-MM-DD\\n", location, date > "/dev/stderr"
                exit 1
            }
            # NCBI names contigs 21 and MT; a chr21 file would otherwise give no site at all.
            if (!on_primary) {
                printf "CLINVAR_SITES: %s has no record on a primary chromosome (named 1-22, X, Y, MT, as NCBI writes them)\\n",
                    location > "/dev/stderr"
                exit 1
            }
            print date > "clinvar_date.txt.partial"
        }'

    size=\$(wc -c < "\$vcf" | tr -d ' ')
    printf 'clinvar\\t%s\\t%s\\t%s\\t%s\\t%s\\n' "\$location" "\$size" "\${modified:--}" "\$got" "\$check" \\
        > clinvar_sources.tsv.partial
    if ${fetch}; then rm download/clinvar.vcf.gz; fi   # not an output: no need to keep 200 MB in the work dir

    for c in ${chroms.join(' ')}; do mv "sites_\$c.tsv.partial" "sites_\$c.tsv"; done
    mv skipped.tsv.partial skipped.tsv
    mv clinvar_date.txt.partial clinvar_date.txt
    mv clinvar_sources.tsv.partial clinvar_sources.tsv
    """

    stub:
    """
    for c in ${chroms.join(' ')}; do : > sites_\$c.tsv; done
    : > skipped.tsv
    echo 1970-01-01 > clinvar_date.txt
    printf 'clinvar\\tstub\\t0\\t-\\td41d8cd98f00b204e9800998ecf8427e\\tnot checked\\n' > clinvar_sources.tsv
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
    : > ucsc_sources.tsv.partial
    for table in ncbiRefSeq knownGene; do
        url='${params.ucsc_base}'/database/\$table.txt.gz
        # Last-Modified comes from the response that was read (-D), not from a separate HEAD
        # request that could see a newer file.
        curl -sSL --fail --retry 5 -D "\$table.headers" -o "\$table.txt.gz.partial" "\$url"
        # With no md5 to check, at least refuse a table cut short: it would lose windows silently.
        if ! gzip -t "\$table.txt.gz.partial"; then
            echo "FETCH_UCSC: \$url is not a whole gzip file" >&2
            exit 1
        fi
        size=\$(wc -c < "\$table.txt.gz.partial" | tr -d ' ')
        modified=\$(awk 'tolower(\$1) == "last-modified:" { sub(/^[^:]*:[ \\t]*/, ""); sub(/\\r\$/, ""); m = \$0 }
                       END { print m }' "\$table.headers")
        md5=\$(openssl dgst -md5 -r "\$table.txt.gz.partial" | awk '{ print \$1 }')
        printf 'ucsc_%s\\t%s\\t%s\\t%s\\t%s\\tnone published\\n' "\$table" "\$url" "\$size" "\${modified:--}" "\$md5" \\
            >> ucsc_sources.tsv.partial
    done
    mv ncbiRefSeq.txt.gz.partial ncbiRefSeq.txt.gz
    mv knownGene.txt.gz.partial knownGene.txt.gz
    mv ucsc_sources.tsv.partial ucsc_sources.tsv
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
    base='${params.ucsc_base}'/bigZips
    curl -sSL --fail --retry 5 -o md5sum.txt "\$base/md5sum.txt"
    # md5sum.txt lines: '<md5>  <name>' (a '*' before the name marks binary mode)
    expected=\$(awk '{ name = \$2; sub(/^\\*/, "", name) } name == "hg38.fa.gz" { print tolower(\$1) }' md5sum.txt)
    if [[ ! \$expected =~ ^[0-9a-f]{32}\$ ]]; then
        echo "FETCH_FASTA: \$base/md5sum.txt has no md5 for hg38.fa.gz" >&2
        exit 1
    fi
    curl -sSL --fail --retry 5 -o hg38.fa.gz.partial "\$base/hg38.fa.gz"
    got=\$(openssl dgst -md5 -r hg38.fa.gz.partial | awk '{ print \$1 }')
    if [[ \$got != "\$expected" ]]; then
        echo "FETCH_FASTA: \$base/hg38.fa.gz has md5 \$got; md5sum.txt says \$expected" >&2
        exit 1
    fi
    mv hg38.fa.gz.partial hg38.fa.gz
    """

    stub:
    """
    printf '' | gzip -c > hg38.fa.gz
    """
}
