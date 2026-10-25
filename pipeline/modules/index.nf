// Index processes: harvest assembly, the genome, the key shards, the index, the checks
// that gate publishing, and the report.
//
// Shared formats (TSV, no header unless said; bare chromosome names: 21, X, MT):
//   harvest    CHROM POS REF ALT CAID HGVS_GRCh38 HGVS_GRCh37, CAID empty for an allele nobody
//              registered and ERROR:<type> for a site the Registry refused
//   pairs      HGVS CAID, the index's keys and values
//   counts     chrom source measure value, as reports/coverage.tsv has them
// Sorting and joining run with LC_ALL=C: byte order is what join and build --sorted need, and
// the same in every locale. Outputs are written as *.partial and renamed once complete, so a
// task that dies leaves no output that looks finished.

// The publish gate, as bash that writes gate.txt: one line per failed check, none if all passed.
// A check fails when validate or reach exited non-zero (a row whose HGVS the genome does not
// reproduce, a key not found again) or, by Ruling R8, when a chromosome's windows enumerated no
// allele at all: that is what a Registry answer of a changed shape looks like, and it would
// otherwise leave an index of the harvested sites alone that passes every check. FINALIZE
// applies the gate and REPORT states it, from the same inputs.
//   statuses  '<validate|reach>:<chrom>:<exit status>' for every check
//   stats     the staged keys_<chrom>.tsv files of KEYS
def gateScript(statuses, stats) {
    def lines = [': > gate.txt']
    statuses.findAll { s -> !s.endsWith(':0') }.each { s ->
        def p = s.tokenize(':')   // check, chrom, status
        lines << "echo '${p[0]} ${p[1]}: exit ${p[2]} (see reports/${p[0]}_${p[1]}.txt)' >> gate.txt"
    }
    // keys_<chrom>.tsv: a header, then chrom accession enum_files enumerated harvested keys
    lines << """awk -F'\\t' 'FNR > 1 && \$3 > 0 && \$4 == 0 {
        printf "enumerate %s: %s windows file(s) gave no allele (has the Registry answer changed?)\\n", \$1, \$3
    }' /dev/null ${stats} >> gate.txt"""
    return lines.join('\n')
}

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
    # Each directory's harvest files: chunk_*.tsv (the harvester's, or an earlier run's chunks)
    # and <chrom>.tsv.gz (an earlier run's harvest/). The staged directories are links: -L.
    for d in cache*; do
        n=\$(find -L "\$d" -maxdepth 1 -type f \\( -name '*.tsv' -o -name '*.tsv.gz' \\) | wc -l)
        if [[ \$n -eq 0 ]]; then
            echo "SPLIT_CACHE: --cache directory \$(cd -P "\$d" && pwd) holds no *.tsv or *.tsv.gz file" >&2
            exit 1
        fi
    done
    find -L cache* -maxdepth 1 -type f \\( -name '*.tsv' -o -name '*.tsv.gz' \\) | LC_ALL=C sort > files.txt

    for c in ${chroms.join(' ')}; do : > "cache_\$c.tsv.partial"; done
    while IFS= read -r f; do
        # gzip -dcf passes a plain file through unchanged
        gzip -dcf "\$f" | awk -F'\\t' -v file="\$f" -v want='${chroms.join(',')}' '
            BEGIN { split(want, w, ","); for (i in w) wanted[w[i]] = 1 }
            NF != 7 {
                printf "SPLIT_CACHE: %s line %d has %d columns, not the 7 of a harvest row\\n", file, NR, NF > "/dev/stderr"
                exit 1
            }
            \$5 ~ /^CA[0-9]+\$/ && (\$1 in wanted) { print >> ("cache_" \$1 ".tsv.partial") }'
    done < files.txt
    for c in ${chroms.join(' ')}; do mv "cache_\$c.tsv.partial" "cache_\$c.tsv"; done
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
    // Every site list of one chromosome (numbered, as each gnomAD stream writes sites.tsv) with
    // the dataset of each, in the same order, and its cached answers (or [] without --cache).
    tuple val(chrom), val(datasets), path(sites, stageAs: 'sites/sites_*.tsv', arity: '0..*'), path(cache)
    val batch_size   // sites per LOOKUP task

    output:
    tuple val(chrom), path("cached_${chrom}.tsv"), emit: cached                         // answers reused from the cache
    tuple val(chrom), path("lookup_${chrom}_*.tsv", arity: '0..*'), emit: batches      // sites still to look up
    path "counts_${chrom}.tsv", emit: counts   // each dataset's sites, and the distinct sites, cached and looked up

    script:
    if (datasets.size() != sites.size()) {
        error("MERGE_SITES ${chrom}: ${datasets.size()} dataset names for ${sites.size()} site lists")
    }
    def perDataset = [datasets, sites].transpose().collect { d, f ->
        "awk -v OFS='\\t' 'END { print \"${chrom}\", \"${d}\", \"sites\", NR }' ${f}"
    }
    """
    export LC_ALL=C
    # Field 1 of every line becomes the site's CHROM:POS:REF:ALT, so join can match each site to
    # its cached answer on files sorted by it: memory stays bounded however many sites there are.
    key() { awk -F'\\t' -v OFS='\\t' '{ print \$1 ":" \$2 ":" \$3 ":" \$4, \$0 }'; }
    cat /dev/null ${sites} | key | sort -t\$'\\t' -k1,1 -u -S 2G -T . > sites.keyed
    cat /dev/null ${cache} | key | sort -t\$'\\t' -k1,1 -u -S 2G -T . > cache.keyed
    join -t\$'\\t' -o 1.2,1.3,1.4,1.5,1.6,1.7,1.8 cache.keyed sites.keyed > cached.tsv
    join -t\$'\\t' -v 2 cache.keyed sites.keyed | cut -f 2-5 > lookup.tsv

    # lookup_<chrom>_0000.tsv, ...: none when the cache answers every site.
    if [[ -s lookup.tsv ]]; then
        split -l ${batch_size} -d -a 4 lookup.tsv batch_
        for f in batch_[0-9][0-9][0-9][0-9]; do mv "\$f" "lookup_${chrom}_\${f#batch_}.tsv"; done
    fi

    distinct=\$(awk 'END { print NR }' sites.keyed)
    cached=\$(awk 'END { print NR }' cached.tsv)
    {
        ${perDataset.join('\n        ')}
        printf '%s\\tharvest\\tsites\\t%s\\n' ${chrom} "\$distinct"
        printf '%s\\tharvest\\tcached\\t%s\\n' ${chrom} "\$cached"
        printf '%s\\tharvest\\tlooked_up\\t%s\\n' ${chrom} "\$(( distinct - cached ))"
    } > counts_${chrom}.tsv.partial
    mv counts_${chrom}.tsv.partial counts_${chrom}.tsv
    mv cached.tsv cached_${chrom}.tsv
    """

    stub:
    """
    : > cached_${chrom}.tsv
    : > lookup_${chrom}_0000.tsv
    : > counts_${chrom}.tsv
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
    export LC_ALL=C
    mkdir harvest
    # By position, then REF and ALT: the same harvest whatever order the batches came back in.
    cat ${cached} ${answers} | sort -t\$'\\t' -k2,2n -k3,3 -k4,4 -S 2G -T . | gzip -nc > harvest/${chrom}.tsv.gz.partial
    mv harvest/${chrom}.tsv.gz.partial harvest/${chrom}.tsv.gz
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
    // genome writes both files under temporary names and renames them once complete.
    """
    mkdir genome
    clingen-index genome --fasta ${fasta} --contigs ${chroms.join(',')} --out genome/GRCh38.bin
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
    tuple val(chrom), val(accession), path(enums, stageAs: 'enum/*', arity: '0..*'), path(harvest)

    output:
    // <accession>.tsv: HGVS<TAB>CAID in byte order, so the shards concatenated in the byte
    // order of their names are in index order.
    path "${accession}.tsv", emit: shard
    // keys_<chrom>.tsv: header and one row: chrom accession enum_files enumerated harvested keys
    path "keys_${chrom}.tsv", emit: stats

    script:
    // The registered harvest rows as pairs, as build --chunks takes them (a CA ID and a GRCh38
    // HGVS). One HGVS with two CA IDs stays two lines, which build --sorted refuses.
    """
    export LC_ALL=C
    { cat /dev/null ${enums}; gzip -dc ${harvest} | awk -F'\\t' -v OFS='\\t' '
          \$5 ~ /^CA[0-9]+\$/ && \$6 != "" { print \$6, \$5; n++ }
          END { print n + 0 > "harvested.txt" }'; } |
        sort -u -S 2G -T . > ${accession}.tsv.partial

    enumerated=\$(cat /dev/null ${enums} | wc -l | tr -d ' ')
    keys=\$(wc -l < ${accession}.tsv.partial | tr -d ' ')
    printf 'chrom\\taccession\\tenum_files\\tenumerated\\tharvested\\tkeys\\n' > keys_${chrom}.tsv.partial
    printf '%s\\t%s\\t%s\\t%s\\t%s\\t%s\\n' ${chrom} ${accession} ${enums.size()} "\$enumerated" "\$(cat harvested.txt)" "\$keys" \\
        >> keys_${chrom}.tsv.partial
    mv keys_${chrom}.tsv.partial keys_${chrom}.tsv
    mv ${accession}.tsv.partial ${accession}.tsv
    """

    stub:
    """
    : > ${accession}.tsv
    printf 'chrom\\taccession\\tenum_files\\tenumerated\\tharvested\\tkeys\\n' > keys_${chrom}.tsv
    printf '%s\\t%s\\t%s\\t0\\t0\\t0\\n' ${chrom} ${accession} ${enums.size()} >> keys_${chrom}.tsv
    """
}

process BUILD_INDEX {
    label 'build'

    input:
    path shards, stageAs: 'shards/*'     // every <accession>.tsv; read in the byte order of their names
    tuple path(genome_bin, stageAs: 'genome/*'), path(genome_idx, stageAs: 'genome/*')
    path provenance, stageAs: 'provenance/*'   // ucsc_sources.tsv, clinvar_{date.txt,sources.tsv}, gnomad_files.tsv: for the sources
    // [set: region set or 'bed', date: run date, reference: Boolean (--include_reference),
    //  genome: absolute path of a --genome given, or null for the built one]
    val manifest

    output:
    path 'index', emit: index            // caid.fst and manifest.json

    script:
    // A String 'false' (the command line's, or a params file's) is true in a condition.
    if (!(manifest.reference instanceof Boolean)) {
        error("BUILD_INDEX: reference must be a Boolean, not ${manifest.reference.getClass().simpleName} '${manifest.reference}'")
    }
    // A built genome is recorded relative to the index (../genome/GRCh38.bin), as FINALIZE
    // publishes the two side by side; a --genome the user gave, by its own absolute path rather
    // than by the link staged in this work directory.
    def genome = manifest.genome ? "--genome '${manifest.genome}'" : "--genome ${genome_bin} --genome-relative"
    """
    # LC_ALL=C also orders the shards/*.tsv glob by byte: accession order, which is key order.
    export LC_ALL=C

    # The manifest's sources: the Registry's region set and run date, then each dataset's release.
    sources=(--source 'registry_${manifest.set}_${manifest.date}')
    ${manifest.reference ? 'sources+=(--source reference_alleles)' : ''}
    if [[ -e provenance/clinvar_date.txt ]]; then
        sources+=(--source "clinvar_\$(cat provenance/clinvar_date.txt)")
    fi
    if [[ -e provenance/gnomad_files.tsv ]]; then
        # gnomad_files.tsv: dataset chrom url md5 size; the release is the <v> of the URL's /release/<v>/
        awk -F'\\t' '{
            if (!match(\$3, /\\/release\\/[^\\/]+\\//)) {
                printf "BUILD_INDEX: gnomAD URL %s has no /release/<version>/\\n", \$3 > "/dev/stderr"
                exit 1
            }
            print \$1 "_v" substr(\$3, RSTART + 9, RLENGTH - 10)
        }' provenance/gnomad_files.tsv | sort -u > gnomad_sources.txt
        while IFS= read -r label; do sources+=(--source "\$label"); done < gnomad_sources.txt
    fi

    clingen-index build --sorted shards/*.tsv --out index "\${sources[@]}" --harvest-date ${manifest.date} ${genome}
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
    # validate reads a directory of chunk_*.tsv files
    mkdir chunks
    gzip -dc ${harvest} > chunks/chunk_00000.tsv
    status=0
    clingen-index validate --chunks chunks --genome ${genome_bin} --mismatches mismatches.tsv --strict \\
        > table.txt 2> summary.txt || status=\$?
    if [[ \$status -gt 1 ]]; then
        cat summary.txt >&2
        exit \$status
    fi

    # The counts line, the table per class, and the first mismatching rows, if any.
    mismatches=\$(( \$(wc -l < mismatches.tsv) - 1 ))
    {
        cat summary.txt table.txt
        if [[ \$mismatches -gt 0 ]]; then
            printf '\\nmismatches (the first 100 of %s):\\n' "\$mismatches"
            head -n 101 mismatches.tsv
        fi
    } > validate_${chrom}.txt.partial
    mv validate_${chrom}.txt.partial validate_${chrom}.txt
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
    // The genome is given, not taken from the manifest: its ../genome/GRCh38.bin is relative to
    // BUILD_INDEX's work directory, which a cloud executor does not stage here.
    """
    status=0
    clingen-index reach --index ${index} --genome ${genome_bin} --prefix '${accession}:' \\
        > counts.txt 2> examples.txt || status=\$?
    if [[ \$status -gt 1 ]]; then
        cat examples.txt >&2
        exit \$status
    fi

    # The counts and the per-class table, then example keys of each outcome but reachable.
    {
        cat counts.txt
        if [[ -s examples.txt ]]; then
            printf '\\nexamples:\\n'
            cat examples.txt
        fi
    } > reach_${chrom}.txt.partial
    mv reach_${chrom}.txt.partial reach_${chrom}.txt
    """

    stub:
    """
    : > reach_${chrom}.txt
    status=0
    """
}

process FINALIZE {
    label 'small'
    // A failed gate is deliberate: another attempt would fail the same way.
    errorStrategy 'terminate'
    publishDir "${params.outdir}", mode: 'copy'

    input:
    path index
    path genome, stageAs: 'genome/*'     // the built genome, published beside the index; [] for a --genome given
    val statuses                         // '<validate|reach>:<chrom>:<exit status>' for every check
    path stats, stageAs: 'keys/*'        // keys_<chrom>.tsv: a chromosome with windows but no enumerated key fails
    val allow_mismatches                 // publish despite failed checks
    // REPORT's summary.md, unread: taken so that FINALIZE runs after REPORT, and a run the gate
    // stops still leaves its reports.
    path report, stageAs: 'report/*'

    output:
    // The staged inputs themselves, published once every check passed.
    path 'index', includeInputs: true, emit: index
    path 'genome/*', includeInputs: true, arity: '0..*', emit: genome

    script:
    // A String 'false' (the command line's, or a params file's) is true in a condition.
    if (!(allow_mismatches instanceof Boolean)) {
        error("FINALIZE: allow_mismatches must be a Boolean, not ${allow_mismatches.getClass().simpleName} '${allow_mismatches}'")
    }
    """
    ${gateScript(statuses, stats)}
    if [[ -s gate.txt ]]; then
        if ${allow_mismatches}; then
            echo 'FINALIZE: publishing despite failed checks (--allow_mismatches):' >&2
            sed 's/^/  /' gate.txt >&2
        else
            {
                echo "FINALIZE: not publishing index/: \$(wc -l < gate.txt | tr -d ' ') check(s) failed:"
                sed 's/^/  /' gate.txt
                echo 'reports/summary.md says more; --allow_mismatches publishes anyway.'
            } >&2
            exit 1
        fi
    fi
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
    path provenance, stageAs: 'provenance/*'     // ucsc_sources.tsv, clinvar_{date.txt,sources.tsv}, gnomad_files.tsv
    path regions_summary, stageAs: 'regions_summary.tsv'
    path counts, stageAs: 'counts/*'             // counts_<chrom>.tsv of MERGE_SITES
    val statuses                                 // as FINALIZE gets them
    // [date:, chroms:, regions: set or BED file name, windowing:, datasets:, include_reference:,
    //  allow_mismatches:]
    val info

    output:
    path 'reports/coverage.tsv', emit: coverage
    path 'reports/summary.md', emit: summary
    path 'reports/sources.tsv', emit: sources
    path 'reports/skipped.tsv.gz', emit: skipped

    script:
    // The rows of coverage.tsv for each chromosome, as source:measure.
    def measures = ['registry:windows', 'registry:core_bp', 'registry:query_bp', 'registry:alleles'] +
        info.datasets.collectMany { d -> ["${d}:sites", "${d}:skipped"] } +
        ['harvest:sites', 'harvest:cached', 'harvest:looked_up', 'harvest:registered', 'index:keys']
    def w = info.windowing
    def regions = w.set == 'bed' ? "${info.regions} (BED)" : info.regions
    def failedGate = info.allow_mismatches
        ? 'FINALIZE publishes index/ anyway (--allow_mismatches).'
        : 'FINALIZE does not publish index/; rerun with --allow_mismatches to publish it anyway.'
    """
    export LC_ALL=C
    mkdir reports
    ${gateScript(statuses, stats)}

    # Every measure as chrom source measure value: MERGE_SITES' counts, REGIONS' summary (with a
    # header), KEYS' stats (with a header) and the skipped alleles.
    {
        cat /dev/null ${counts}
        awk -F'\\t' -v OFS='\\t' 'FNR > 1 {
            print \$1, "registry", "windows", \$4; print \$1, "registry", "core_bp", \$5; print \$1, "registry", "query_bp", \$6
        }' regions_summary.tsv
        awk -F'\\t' -v OFS='\\t' 'FNR > 1 {
            print \$1, "registry", "alleles", \$4; print \$1, "harvest", "registered", \$5; print \$1, "index", "keys", \$6
        }' /dev/null ${stats}
        awk -F'\\t' -v OFS='\\t' '{ n[\$2 OFS \$1]++ } END { for (k in n) print k, "skipped", n[k] }' /dev/null ${skipped}
    } > measures.tsv

    # coverage.tsv: every measure of every chromosome, 0 where a source gave none.
    awk -F'\\t' -v OFS='\\t' -v chroms='${info.chroms.join(' ')}' -v measures='${measures.join(' ')}' '
        { v[\$1 OFS \$2 OFS \$3] += \$4 }
        END {
            print "chrom", "source", "measure", "value"
            nc = split(chroms, c, " ")
            nm = split(measures, m, " ")
            for (i = 1; i <= nc; i++) {
                for (j = 1; j <= nm; j++) {
                    split(m[j], sm, ":")
                    print c[i], sm[1], sm[2], v[c[i] OFS sm[1] OFS sm[2]] + 0
                }
            }
        }' measures.tsv > reports/coverage.tsv.partial

    # sources.tsv: what each download read. UCSC and ClinVar as their processes recorded it;
    # gnomAD from RESOLVE_GNOMAD's listing, whose md5 (base64 there) GNOMAD_SITES checked.
    {
        printf 'source\\turl\\tsize\\tlast_modified\\tmd5\\tmd5_check\\n'
        for f in provenance/ucsc_sources.tsv provenance/clinvar_sources.tsv; do
            if [[ -e \$f ]]; then cat "\$f"; fi
        done
        if [[ -e provenance/gnomad_files.tsv ]]; then
            while IFS=\$'\\t' read -r dataset chrom url md5 size; do
                hex=\$(printf '%s' "\$md5" | openssl base64 -d -A | od -An -v -tx1 | tr -d ' \\n')
                printf '%s_%s\\t%s\\t%s\\t-\\t%s\\tchecked\\n' "\$dataset" "\$chrom" "\$url" "\$size" "\$hex"
            done < provenance/gnomad_files.tsv
        fi
    } > reports/sources.tsv.partial

    cat /dev/null ${skipped} > skipped.tsv
    { printf 'dataset\\tCHROM\\tPOS\\tID\\tREF\\tALT\\treason\\n'; cat skipped.tsv; } | gzip -nc > reports/skipped.tsv.gz.partial

    # Each chromosome's checks as table cells: validate's rows compared and matched (the table
    # after its 'class' header, up to a blank line), and reach's outcomes (before its 'class' header).
    awk -F'\\t' -v OFS='\\t' '
        FNR == 1 {
            n = split(FILENAME, path, "/")
            split(path[n], p, "[_.]")   # validate_<chrom>.txt, reach_<chrom>.txt
            check = p[1]; chrom = p[2]; table = 0; seen[chrom] = 1
        }
        \$1 == "class" { table = 1; next }
        check == "validate" && table && NF == 4 { compared[chrom] += \$2; matched[chrom] += \$3; next }
        check == "validate" && NF != 4 { table = 0 }
        check == "reach" && !table && NF == 2 { reach[chrom, \$1] = \$2 }
        END {
            for (c in seen) {
                bad = compared[c] - matched[c]
                v = bad ? "**" bad " of " compared[c] " rows mismatch**" : matched[c] " of " compared[c] " rows match"
                failed = reach[c, "wrong_ca"] + reach[c, "not_found"] + reach[c, "ref_mismatch"] + reach[c, "unsupported"]
                checked = reach[c, "keys"] - reach[c, "reference"] - reach[c, "unparseable"]
                r = failed ? "**" failed " of " checked " keys unreachable**" : reach[c, "reachable"] " of " checked " keys reachable"
                if (reach[c, "reference"] > 0) r = r ", " reach[c, "reference"] " reference"
                if (reach[c, "unparseable"] > 0) r = r ", " reach[c, "unparseable"] " unparseable"
                print c, v, r
            }
        }' /dev/null ${checks} > checks.tsv

    {
        printf '# CA-ID index regeneration, %s\\n\\n' '${info.date}'
        printf -- '- Chromosomes: %s\\n' '${info.chroms.size() == 25 ? 'all (1-22, X, Y, MT)' : info.chroms.join(', ')}'
        printf -- '- Regions: %s; pad %s, merge gap %s, max window %s, flank %s bp\\n' \\
            '${regions}' ${w.pad} ${w.merge_gap} ${w.max_window} ${w.flank}
        printf -- '- Datasets: %s\\n' '${info.datasets.join(', ')}'
        printf -- '- Reference alleles: %s\\n' '${info.include_reference ? 'kept (--include_reference)' : 'left out'}'

        printf '\\n## Gate\\n\\n'
        if [[ -s gate.txt ]]; then
            printf 'Gate: **failed**: %s check(s).\\n\\n' "\$(wc -l < gate.txt | tr -d ' ')"
            sed 's/^/- /' gate.txt
            printf '\\n%s\\n' '${failedGate}'
        else
            printf 'Gate: passed. Every validate row matched, every reach key that names a variant was found\\n'
            printf 'again (reference and unparseable keys are only counted), and every chromosome with windows\\n'
            printf 'enumerated alleles; FINALIZE publishes index/.\\n'
        fi

        printf '\\n## Per chromosome\\n\\n'
        printf '| chrom | windows | enumerated | harvest sites | registered | keys | validate | reach |\\n'
        printf '|---|---:|---:|---:|---:|---:|---|---|\\n'
        awk -F'\\t' -v chroms='${info.chroms.join(' ')}' '
            FILENAME == "checks.tsv" { v[\$1] = \$2; r[\$1] = \$3; next }
            FNR > 1 { m[\$1, \$2 ":" \$3] = \$4 }
            END {
                nc = split(chroms, c, " ")
                for (i = 1; i <= nc; i++) {
                    x = c[i]
                    printf "| %s | %s | %s | %s | %s | %s | %s | %s |\\n", x, m[x, "registry:windows"], m[x, "registry:alleles"],
                        m[x, "harvest:sites"], m[x, "harvest:registered"], m[x, "index:keys"], v[x], r[x]
                }
            }' checks.tsv reports/coverage.tsv.partial

        printf '\\n## Skipped alleles\\n\\n'
        if [[ -s skipped.tsv ]]; then
            printf '| dataset | reason | alleles |\\n|---|---|---:|\\n'
            awk -F'\\t' '{ n[\$1 " | " \$7]++ } END { for (k in n) printf "| %s | %s |\\n", k, n[k] }' skipped.tsv | sort
        else
            printf 'None.\\n'
        fi

        printf '\\n## Limits\\n\\n'
        printf -- '- The Registry region query returns only alleles lying wholly inside a window, whose span reaches\\n'
        printf '  ${w.flank} bp past its core: a longer allele that crosses a window edge is not enumerated.\\n'
        printf -- '- Outside the windows, the index holds only the registered alleles of the harvested datasets.\\n'
        printf -- '- Enumeration is not incremental: the region query cannot ask for alleles registered since an\\n'
        printf '  earlier run, so every run asks for every window again (--cache saves only the lookups).\\n'

        printf '\\nThe other reports: coverage.tsv (every measure per chromosome and source), sources.tsv (each\\n'
        printf 'download and its md5), skipped.tsv.gz (each allele not looked up, and why), and\\n'
        printf 'validate_<chrom>.txt and reach_<chrom>.txt (the checks).\\n'
    } > reports/summary.md.partial

    for f in coverage.tsv sources.tsv skipped.tsv.gz summary.md; do mv "reports/\$f.partial" "reports/\$f"; done
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
