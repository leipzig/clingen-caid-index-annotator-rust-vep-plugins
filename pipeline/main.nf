// Regenerates the ClinGen CA-ID index (caid.fst + manifest.json) for one chromosome or all:
// every allele the Registry holds in the query windows, plus the registered answers for the
// harvested datasets' sites, checked by validate and reach before FINALIZE publishes it.
// `nextflow run pipeline --help` lists the parameters (nextflow_schema.json).

include { validateParameters; paramsHelp; paramsSummaryLog } from 'plugin/nf-schema'
include { RESOLVE_GNOMAD; GNOMAD_SITES; CLINVAR_SITES; FETCH_UCSC; FETCH_FASTA } from './modules/sources'
include { REGIONS; ENUMERATE; LOOKUP } from './modules/registry'
include { SPLIT_CACHE; MERGE_SITES; COLLECT; BUILD_GENOME; KEYS; BUILD_INDEX; VALIDATE; REACH; FINALIZE; REPORT } from './modules/index'

workflow {
    // Under the strict parser, command-line values are Strings: a bare --help is 'true' and
    // `--help chrom` is 'chrom'; from config or a params file they keep their types.
    def help = params.help.toString()
    def helpFull = params.helpFull.toString() == 'true'
    if (help != 'false' || helpFull) {
        // paramsHelp casts its options with `as Boolean`, and 'false' as Boolean is true,
        // so it is handed real Booleans. println, not log.info: help is the output asked
        // for, and -quiet silences log.info.
        println paramsHelp(
            parameter: help in ['true', 'false'] ? null : help,
            fullHelp: helpFull,
            showHidden: params.showHidden.toString() == 'true'
        )
    }
    else {
        validateParameters()
        def run = settings()
        log.info paramsSummaryLog(afterText: "\nchromosomes : ${run.chroms.join(',')}\n")
        REGENERATE(run)
    }
}

// The data flow of the design (docs/superpowers/specs/2026-10-24-index-regeneration-pipeline-design.md).
// Every list of files handed to a task is sorted: a task's hash covers its inputs in order,
// so a list in completion order would make -resume run BUILD_INDEX and the rest again.
workflow REGENERATE {
    take:
    run     // settings()

    main:
    def chroms = run.chroms

    // Query windows and Registry enumeration. ENUMERATE runs one task per windows file;
    // its pairs are gathered per chromosome as soon as that chromosome's files are done.
    def regionsInput = channel.empty()
    def ucscSources = channel.empty()
    if (run.bed) {
        regionsInput = channel.value(run.bed)
    }
    else {
        FETCH_UCSC()
        regionsInput = FETCH_UCSC.out.tables
        ucscSources = FETCH_UCSC.out.sources
    }
    REGIONS(regionsInput, chroms, run.windowing)
    def windowCounts = REGIONS.out.windows.flatMap { files ->
        chroms.collect { c -> [c, files.count { f -> chromOf(f) == c }] }
    }
    ENUMERATE(
        REGIONS.out.windows.flatMap { files -> files.collect { f -> [chromOf(f), f] } },
        run.paging,
        run.include_reference
    )
    def enumerated = gatherByChrom(ENUMERATE.out.pairs, windowCounts)

    // Sites to look up: one list per dataset and chromosome, merged per chromosome once all
    // of its lists are in (one from ClinVar, one per gnomAD file RESOLVE_GNOMAD lists for it).
    // Each list travels as [dataset, file], so MERGE_SITES can count each dataset's sites.
    def siteFiles = channel.empty()
    def skipped = channel.empty()
    def provenance = ucscSources
    def gnomadRows = channel.value([])
    if ('clinvar' in run.datasets) {
        CLINVAR_SITES(run.clinvar_vcf, run.clinvar_md5, chroms)
        siteFiles = siteFiles.mix(CLINVAR_SITES.out.sites.flatMap { files -> files.collect { f -> [chromOf(f), ['clinvar', f]] } })
        skipped = skipped.mix(CLINVAR_SITES.out.skipped)
        provenance = provenance.mix(CLINVAR_SITES.out.date, CLINVAR_SITES.out.sources)
    }
    def gnomadDatasets = run.datasets.findAll { d -> d.startsWith('gnomad_') }
    if (gnomadDatasets) {
        RESOLVE_GNOMAD(run.gnomad_version, chroms, gnomadDatasets)
        gnomadRows = RESOLVE_GNOMAD.out.files.map { f ->
            f.readLines().findAll { line -> line }.collect { line -> line.split('\t') as List }
        }
        GNOMAD_SITES(gnomadRows.flatMap { rows -> rows.collect { r -> [r[0], r[1], r[2], r[3]] } })
        siteFiles = siteFiles.mix(GNOMAD_SITES.out.sites.map { dataset, chrom, f -> [chrom, [dataset, f]] })
        skipped = skipped.mix(GNOMAD_SITES.out.skipped)
        provenance = provenance.mix(RESOLVE_GNOMAD.out.files)
    }
    def withClinvar = 'clinvar' in run.datasets
    def siteCounts = gnomadRows.flatMap { rows ->
        chroms.collect { c -> [c, (withClinvar ? 1 : 0) + rows.count { r -> r[1] == c }] }
    }
    def sites = gatherByChrom(siteFiles, siteCounts)

    // Answers from earlier harvests, split per chromosome; [] for each without --cache.
    def cached = channel.fromList(chroms).map { c -> [c, []] }
    if (run.cache) {
        SPLIT_CACHE(run.cache, chroms)
        cached = SPLIT_CACHE.out.cache.flatMap { files -> files.collect { f -> [chromOf(f), f] } }
    }

    // Look up what the cache does not answer, in batches, and collect each chromosome's harvest.
    // The joins fail rather than drop a chromosome missing on one side, which would leave it
    // out of the index without a word.
    MERGE_SITES(
        sites.join(cached, failOnMismatch: true, failOnDuplicate: true)
            .map { chrom, lists, cache -> [chrom, lists.collect { l -> l[0] }, lists.collect { l -> l[1] }, cache] },
        run.batch_size
    )
    LOOKUP(MERGE_SITES.out.batches.flatMap { chrom, files -> files.collect { f -> [chrom, f] } })
    def lookupCounts = MERGE_SITES.out.batches.map { chrom, files -> [chrom, files.size()] }
    def answers = gatherByChrom(LOOKUP.out.answers, lookupCounts)
    COLLECT(MERGE_SITES.out.cached.join(answers, failOnMismatch: true, failOnDuplicate: true))

    // The genome: the one given, or built from UCSC hg38 for the run's chromosomes.
    def genome = channel.empty()
    def genomeToPublish = channel.value([])
    if (run.genome) {
        genome = channel.value(run.genome)
    }
    else {
        FETCH_FASTA()
        BUILD_GENOME(FETCH_FASTA.out.fasta, chroms)
        genome = BUILD_GENOME.out.genome
        genomeToPublish = BUILD_GENOME.out.genome
    }

    // The manifest's dates, from the UTC day each Registry task recorded: the registry source
    // takes the earliest ENUMERATE task's, the harvest date the earliest ENUMERATE or LOOKUP
    // task's. A task a -resume reuses keeps the day it ran, so a resumed run is dated by the
    // answers it holds, not by its own start. One reduce gives both: min is deprecated since
    // Nextflow 26.04, and reduce yields a value, which BUILD_INDEX needs to emit its index to
    // every REACH task (combining two values would yield a one-item queue). There is always an
    // ENUMERATE task (REGIONS writes at least one windows file), so registry is never null. A
    // run stopped before any Registry task ended reduces to null, hence d?. below: such a run
    // has already failed, and a null date would fail BUILD_INDEX's check by name.
    def dates = ENUMERATE.out.day.map { d -> [registry: d, harvest: d] }
        .mix(LOOKUP.out.day.map { d -> [registry: null, harvest: d] })
        .reduce { a, b ->
            [registry: [a.registry, b.registry].findAll { d -> d }.min(), harvest: [a.harvest, b.harvest].min()]
        }

    // One byte-sorted key shard per chromosome, then the index, then the checks. The manifest
    // records the sources (the region set and the Registry's date; the datasets' releases come
    // from the provenance files), the harvest date, and the genome: a built one relative to the
    // index, a given one by path.
    KEYS(
        enumerated.join(COLLECT.out.harvest, failOnMismatch: true, failOnDuplicate: true)
            .map { chrom, enums, harvest -> [chrom, accession(chrom), enums, harvest] },
        run.include_reference
    )
    def manifest = dates.map { d ->
        [
            set: run.windowing.set,
            registry_date: d?.registry,
            harvest_date: d?.harvest,
            reference: run.include_reference,
            genome: run.genome ? run.genome[0].toString() : null,
        ]
    }
    BUILD_INDEX(KEYS.out.shard.toSortedList(), genome, provenance.toSortedList(), manifest)
    VALIDATE(COLLECT.out.harvest, genome)
    REACH(channel.fromList(chroms).map { c -> [c, accession(c)] }, BUILD_INDEX.out.index, genome)

    // Report, then publish only if every check passed (or --allow_mismatches). FINALIZE waits
    // for REPORT's summary, so a run the gate stops has its reports all the same.
    def statuses = VALIDATE.out.result.map { chrom, _report, status -> "validate:${chrom}:${status}" }
        .mix(REACH.out.result.map { chrom, _report, status -> "reach:${chrom}:${status}" })
        .toSortedList()
    REPORT(
        VALIDATE.out.result.mix(REACH.out.result).map { _chrom, report, _status -> report }.toSortedList(),
        KEYS.out.stats.toSortedList(),
        skipped.toSortedList(),
        provenance.toSortedList(),
        REGIONS.out.summary,
        MERGE_SITES.out.counts.toSortedList(),
        statuses,
        dates.map { d ->
            [
                date: run.date,
                registry_date: d?.registry,
                harvest_date: d?.harvest,
                chroms: chroms,
                regions: run.bed ? run.bed.name : run.windowing.set,
                windowing: run.windowing,
                datasets: run.datasets,
                include_reference: run.include_reference,
                allow_mismatches: run.allow_mismatches,
            ]
        }
    )
    FINALIZE(
        BUILD_INDEX.out.index,
        genomeToPublish,
        statuses,
        KEYS.out.stats.toSortedList(),
        run.allow_mismatches,
        REPORT.out.summary
    )
}

// Parameters as the workflow uses them, each converted once. nf-schema has checked them,
// but only on a typed copy: params themselves keep the command line's Strings.
def settings() {
    def sets = ['cds', 'pc_exons', 'exons', 'genes']
    def regions = params.regions.toString()
    def bed = null
    if (!(regions in sets)) {
        bed = file(regions)
        if (!bed.exists()) {
            error("--regions ${regions}: no such BED file (or use one of ${sets.join(', ')})")
        }
    }

    def clinvarVcf = []
    def clinvarMd5 = []
    def clinvar = params.clinvar_vcf.toString()
    if (clinvar != 'latest') {
        clinvarVcf = file(clinvar)
        if (!clinvarVcf.exists()) {
            error("--clinvar_vcf ${clinvar}: no such file")
        }
        // NCBI's .md5 beside it is checked when it is there; without it CLINVAR_SITES records
        // the md5 as not checked, as a file kept for an exact rerun may have lost it.
        def md5 = file("${clinvar}.md5")
        clinvarMd5 = md5.exists() ? md5 : []
    }

    // BUILD_INDEX hands build the genome's path as the launcher sees it, for the manifest, and
    // build opens it there. That works only for a file on a disk the tasks share with the
    // launcher: not an object-store URL, and not with a work directory in object storage, whose
    // tasks see no launcher path. Refused here, rather than after the whole enumeration. The
    // scheme is read from the text, so an s3:// URL is not touched (nor its plugin loaded).
    def genome = null
    if (params.genome) {
        def given = params.genome.toString()
        def scheme = given ==~ /^[A-Za-z][A-Za-z0-9+.-]*:\/\/.*/ ? given.tokenize(':')[0].toLowerCase() : 'file'
        def workScheme = workflow.workDir.scheme
        if (scheme != 'file' || workScheme != 'file') {
            error("--genome ${given}: BUILD_INDEX opens the genome by this path, so it must be a local or shared " +
                "file, with a local or shared work directory (genome: ${scheme}, work directory: " +
                "${workflow.workDir.toUriString()}); leave --genome out and BUILD_GENOME builds the genome")
        }
        def bin = file(given)
        if (!bin.isFile()) {
            error("--genome ${given}: ${bin.exists() ? 'not a file' : 'no such file'}")
        }
        def idx = file("${given}.idx")
        if (!idx.exists()) {
            error("--genome ${given}: no ${idx.name} beside it (clingen-index genome writes both)")
        }
        genome = [bin, idx]
    }

    def cache = []
    if (params.cache) {
        cache = params.cache.toString().tokenize(',').collect { d ->
            def dir = file(d.trim())
            if (!dir.isDirectory()) {
                error("--cache ${d}: not a directory")
            }
            dir
        }
    }

    return [
        chroms: chromosomes(params.chrom.toString()),
        // The day the run started, for the heading of reports/summary.md. The manifest's dates
        // are the days the Registry tasks ran, which a -resume keeps for the tasks it reuses.
        date: workflow.start.toLocalDate().toString(),
        bed: bed,
        windowing: [
            set: bed ? 'bed' : regions,
            pad: wholeNumber('pad'),
            merge_gap: wholeNumber('merge_gap'),
            max_window: wholeNumber('max_window'),
            flank: wholeNumber('flank'),
            per_task: wholeNumber('windows_per_task'),
        ],
        paging: [
            page_size: wholeNumber('page_size'),
            split_overlap: wholeNumber('split_overlap'),
        ],
        batch_size: wholeNumber('batch_size'),
        datasets: params.datasets.toString().tokenize(',').unique(),
        include_reference: params.include_reference.toString().toBoolean(),
        gnomad_version: params.gnomad_version.toString(),
        clinvar_vcf: clinvarVcf,
        clinvar_md5: clinvarMd5,
        genome: genome,
        cache: cache,
        allow_mismatches: params.allow_mismatches.toString().toBoolean(),
    ]
}

// The chromosomes --chrom names, in bare GRCh38 form: '21', 'chr21' -> ['21'];
// 'chrM', 'chrMT', 'MT' -> ['MT']; 'all' -> 1-22, X, Y, MT.
def chromosomes(String value) {
    def primary = grch38Accessions().keySet() as List
    if (value == 'all') {
        return primary
    }
    def chrom = value.replaceFirst(/^chr/, '')
    if (chrom ==~ /\d+/) {
        chrom = chrom.toInteger().toString()   // '021' passed the schema as the integer 21
    }
    if (chrom == 'M') {
        chrom = 'MT'
    }
    if (!(chrom in primary)) {
        error("--chrom ${value}: not a GRCh38 primary chromosome (use 1-22, X, Y, MT or all)")
    }
    return [chrom]
}

// A whole-number parameter as an Integer: '20' from the command line, 20 from config.
def wholeNumber(String name) {
    def value = params[name].toString()
    if (!(value ==~ /\d+/)) {
        error("--${name} ${value}: not a whole number")
    }
    return value.toInteger()
}

// GRCh38 RefSeq accession of each primary chromosome, in index order (the order its keys
// sort in), as in crates/clingen-allele/src/contig.rs.
def grch38Accessions() {
    return [
        '1': 'NC_000001.11', '2': 'NC_000002.12', '3': 'NC_000003.12', '4': 'NC_000004.12',
        '5': 'NC_000005.10', '6': 'NC_000006.12', '7': 'NC_000007.14', '8': 'NC_000008.11',
        '9': 'NC_000009.12', '10': 'NC_000010.11', '11': 'NC_000011.10', '12': 'NC_000012.12',
        '13': 'NC_000013.11', '14': 'NC_000014.9', '15': 'NC_000015.10', '16': 'NC_000016.10',
        '17': 'NC_000017.11', '18': 'NC_000018.10', '19': 'NC_000019.10', '20': 'NC_000020.11',
        '21': 'NC_000021.9', '22': 'NC_000022.11', 'X': 'NC_000023.11', 'Y': 'NC_000024.10',
        'MT': 'NC_012920.1',
    ]
}

def accession(String chrom) {
    return grch38Accessions()[chrom]
}

// The chromosome in a per-chromosome file name: windows_<chrom>_<NNNN>.tsv, sites_<chrom>.tsv,
// cache_<chrom>.tsv (no chromosome name holds an underscore).
def chromOf(f) {
    return f.name.replaceFirst(/\.tsv$/, '').tokenize('_')[1]
}

// Groups [chrom, file] items into [chrom, [sorted files]], given [chrom, n] counts of the
// files each chromosome expects: a chromosome is emitted as soon as its n files are in, and
// as [chrom, []] when it expects none. groupBy (groupTuple is deprecated since Nextflow 26.04)
// takes the size from each [key, size, value] tuple, and fails the run when a group ends with
// fewer or more files than its count, rather than emitting a short list. An item may also be
// [label, file] (the site lists); items sort by their text, the path or label and path.
def gatherByChrom(items, counts) {
    def grouped = items
        .combine(counts, by: 0)
        .map { chrom, f, n -> [chrom, n, f] }
        .groupBy()
        .map { chrom, files -> [chrom, files.toSorted { f -> f.toString() }] }
    def none = counts.filter { _chrom, n -> n == 0 }.map { chrom, _n -> [chrom, []] }
    return grouped.mix(none)
}
