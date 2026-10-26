// index/metadata.json: how a published index was made, from what, and the command that
// rebuilds it byte for byte from the key shards published beside it in keys/. Assembled on the
// launcher, which holds the run's settings and reads the tasks' small outputs (as main.nf reads
// RESOLVE_GNOMAD's listing); the checksums of the large files come from DIGEST, which runs in
// the task image. FINALIZE writes the document into index/ once the gate has passed, so it is
// published with the index and the shards, or not at all.
//
// Rerunning the pipeline regenerates an index rather than reproducing it: the Registry gains
// alleles and cannot be asked what it held on an earlier day. Only a rebuild from keys/ gives
// the same bytes, which is why the document records the rebuild command and its expected md5s.

// Version of this document's layout; a change that removes or renames a field raises it.
def metadataFormatVersion() {
    return 1
}

// The pipeline's own files, relative to projectDir, in byte order: every file a run reads from
// there (main.nf, its modules, the config with its profiles, and the schema nf-schema checks
// the parameters against). Tests and the AWS template are not read by a run, so not counted.
def sourceFiles(projectDir) {
    def dir = file(projectDir.toString())
    def found = ['main.nf', 'nextflow.config', 'nextflow_schema.json'].findAll { name -> dir.resolve(name).exists() }
    ['modules', 'conf'].each { sub ->
        def pattern = sub == 'modules' ? ~/.*\.nf/ : ~/.*\.config/
        def subdir = dir.resolve(sub)
        if (subdir.isDirectory()) {
            subdir.eachFileMatch(pattern) { f -> found << "${sub}/${f.name}".toString() }
        }
    }
    // String order is byte order for these ASCII names, as LC_ALL=C sort gives it.
    return found.toSorted()
}

// The digest of the pipeline's source, computable without .git, so a copy made with rsync can
// still be matched to a commit: the sha256 of the text sha256sum prints for sourceFiles() in
// that order ('<sha256>  <path>' lines), which is what
//   cd pipeline && sha256sum main.nf modules/*.nf nextflow.config conf/*.config nextflow_schema.json \
//       | LC_ALL=C sort -k2 | sha256sum
// prints. Returns [digest: hex, files: [[path:, sha256:], ...]].
def sourceRecord(projectDir) {
    def dir = file(projectDir.toString())
    def files = sourceFiles(projectDir).collect { name ->
        [path: name, sha256: hexDigest('SHA-256', dir.resolve(name).bytes)]
    }
    def manifest = files.collect { f -> "${f.sha256}  ${f.path}\n" }.join('')
    return [digest: hexDigest('SHA-256', manifest.getBytes('UTF-8')), files: files]
}

// The inputs the task image's clingen-index is built from, in the repository that holds
// projectDir (its parent directory): the Dockerfile and what its build context admits, which
// are Cargo.toml, Cargo.lock, rust-toolchain.toml and every file under crates/. The digest is
// made as source_digest is, so it can be computed for any copy of the repository:
//   find Cargo.toml Cargo.lock rust-toolchain.toml Dockerfile crates -type f | LC_ALL=C sort \
//       | xargs sha256sum | sha256sum
// These are the launcher's files, which describe the image only when it was built from them;
// DIGEST's sha256 of the binary is what names the one that ran. null when that directory has no
// Cargo.lock or crates/, as for a copy of pipeline/ alone.
// Returns [root:, digest: hex, files: [[path:, sha256:], ...]] or null.
def buildSourceRecord(projectDir) {
    def root = file(projectDir.toString()).parent
    if (root == null || !root.resolve('Cargo.lock').exists() || !root.resolve('crates').isDirectory()) {
        return null
    }
    def names = ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'Dockerfile'].findAll { name -> root.resolve(name).exists() }
    root.resolve('crates').eachFileRecurse(groovy.io.FileType.FILES) { f -> names << root.relativize(f).toString() }
    // String order is byte order for these ASCII paths, as LC_ALL=C sort gives it.
    def files = names.toSorted().collect { name ->
        [path: name, sha256: hexDigest('SHA-256', root.resolve(name).bytes)]
    }
    def manifest = files.collect { f -> "${f.sha256}  ${f.path}\n" }.join('')
    return [root: root.toString(), digest: hexDigest('SHA-256', manifest.getBytes('UTF-8')), files: files]
}

// The commit of the repository holding projectDir and whether its working tree differs from it
// (git status, untracked files included, ignored ones not), both null when projectDir is not
// in a git checkout or git cannot be run: an rsync'd copy has no .git, and source_digest
// identifies it instead.
def gitRecord(projectDir) {
    def commit = gitOutput(projectDir, ['rev-parse', 'HEAD'])
    if (commit == null || !(commit.trim() ==~ /[0-9a-f]{40}([0-9a-f]{24})?/)) {
        return [commit: null, dirty: null]
    }
    def status = gitOutput(projectDir, ['status', '--porcelain'])
    return [commit: commit.trim(), dirty: status == null ? null : !status.trim().isEmpty()]
}

// git's standard output for `git -C <projectDir> <args>`, or null when it fails or cannot start.
def gitOutput(projectDir, List args) {
    try {
        def proc = (['git', '-C', projectDir.toString()] + args).execute()
        def out = new StringBuilder()
        def err = new StringBuilder()
        proc.waitForProcessOutput(out, err)
        return proc.exitValue() == 0 ? out.toString() : null
    }
    catch (IOException _e) {
        return null
    }
}

def hexDigest(String algorithm, byte[] bytes) {
    def md = java.security.MessageDigest.getInstance(algorithm)
    md.update(bytes)
    return md.digest().encodeHex().toString()
}

// A word as a POSIX shell reads it back unchanged: as it is when it holds only characters the
// shell gives no meaning, otherwise in single quotes (a quote inside written '\'').
def shellQuote(String word) {
    if (word ==~ /[A-Za-z0-9_.\/:=+,@%-]+/) {
        return word
    }
    return "'" + word.replace("'", "'\\''") + "'"
}

// The rows of a TSV file, each split into its columns (trailing empty ones kept); blank lines
// are left out, so an empty stub output gives no row.
def tsvRows(f) {
    return f.text.readLines().findAll { line -> line }.collect { line -> line.split('\t', -1) as List }
}

// The rows of a TSV file with a header line, each as a map from column name to value.
def tsvRecords(f) {
    def rows = tsvRows(f)
    if (!rows) {
        return []
    }
    def header = rows[0]
    return rows.drop(1).collect { row -> [header, row].transpose().collectEntries { k, v -> [k, v] } }
}

// A count as a Long, or the text as it is when it is not one (a stub's '-', say).
def number(value) {
    def text = value.toString()
    return text ==~ /-?\d+/ ? text.toLong() : text
}

// A digest.tsv 'file' row as [name:, size:, md5:].
def fileRecord(List row) {
    return [name: row[2], size: number(row[3]), md5: row[4]]
}

// The digest.tsv 'file' rows of one role, as fileRecord gives them.
def filesOf(List rows, String role) {
    return rows.findAll { r -> r[1] == role }.collect { r -> fileRecord(r) }
}

// The sum of a column of tsvRecords, counting only whole numbers.
def columnTotal(List records, String column) {
    return records.sum(0L) { r -> number(r[column]) instanceof Long ? number(r[column]) : 0L }
}

// The rebuild: the build command that gives caid.fst and manifest.json again from the shards of
// keys/, gunzipped, with the arguments BUILD_INDEX gave it. manifest.json holds the sources in
// the order given, the harvest date and the genome as build recorded it, so the command is read
// back from it: a relative genome (../genome/GRCh38.bin, the built one) is passed as
// genome/GRCh38.bin with --genome-relative and --out index, an absolute one (a --genome given)
// by that path. shardFiles: the <accession>.tsv.gz names, in any order.
def rebuildRecord(Map manifest, List shardFiles, Map expected) {
    // Byte order of the accessions is index order, as BUILD_INDEX reads shards/*.tsv under LC_ALL=C.
    def shards = shardFiles.collect { f -> "keys/${f.replaceFirst(/\.gz$/, '')}".toString() }.toSorted()
    def argv = ['clingen-index', 'build', '--sorted'] + shards + ['--out', 'index']
    (manifest.sources ?: []).each { s -> argv += ['--source', s.toString()] }
    if (manifest.harvest_date != null) {
        argv += ['--harvest-date', manifest.harvest_date.toString()]
    }
    def genome = manifest.genome?.toString()
    def genomeNote
    def relative = genome != null && !genome.startsWith('/')
    if (genome == null) {
        genomeNote = 'The manifest records no genome, so the rebuild needs none.'
    }
    else if (relative) {
        // build records --genome relative to --out after resolving the genome's directory, so
        // genome/ must be a real directory beside index/ (holding the two files or links to
        // them), not a link to another directory.
        def given = genome.startsWith('../') ? genome.substring(3) : genome
        argv += ['--genome', given, '--genome-relative']
        genomeNote = "Built by the pipeline and published in genome/. In the rebuild directory, make genome/ a real " +
            "directory holding GRCh38.bin and GRCh38.bin.idx (copies, or links to the published files): build " +
            "resolves the genome's directory and records the file relative to index/, which gives ${genome} " +
            "again. A genome/ that is itself a link to another directory would be recorded by that directory's path."
    }
    else {
        argv += ['--genome', genome]
        genomeNote = "Given with --genome and recorded by its absolute path, which build checks exists: the rebuild " +
            "needs the file at ${genome} (any genome file there gives the same caid.fst, as build only records the " +
            "path; validate and reach are what read it)."
    }
    return [
        directory: 'A new, empty directory (rebuild/ in steps), the working directory of the command. It holds keys/, ' +
            'every shard of <outdir>/keys/ gunzipped under its name without .gz' +
            (relative ? ', and genome/ as described under genome.' : '.') +
            ' <outdir> is the run\'s --outdir or a local copy of it; the first step makes it absolute before the ' +
            'second changes directory, so a relative <outdir> is read from where the steps start.',
        steps: [
            'out=$(cd <outdir> && pwd -P)',
            'mkdir rebuild && cd rebuild',
            'mkdir keys && for f in "$out"/keys/*.tsv.gz; do n=$(basename "$f" .gz); gzip -dc "$f" > "keys/$n"; done',
        ] + (relative ? ['mkdir genome && ln -s "$out"/genome/GRCh38.bin "$out"/genome/GRCh38.bin.idx genome/'] : []) + [
            argv.collect { w -> shellQuote(w) }.join(' '),
            'md5sum index/caid.fst index/manifest.json   # or: md5 -r on macOS',
        ],
        genome: [recorded: genome, relative: relative, note: genomeNote],
        argv: argv,
        command: argv.collect { w -> shellQuote(w) }.join(' '),
        expected_md5: expected,
        manifest_identical: true,
        manifest_note: 'manifest.json holds no time, host or path other than the genome, so the rebuild writes the ' +
            'same bytes whenever build records the same genome string (see genome). build refuses a --genome that ' +
            'does not exist; rebuilt with another genome path, only the manifest\'s genome line differs, and ' +
            'caid.fst is the same.',
        tool_note: 'Run the clingen-index whose sha256 is tool.clingen_index_sha256 (see tool.note for how to build ' +
            'it); another build may lay out caid.fst differently, and the md5s are what decide. ' +
            'pipeline/tests/check_rebuild.sh <outdir> runs these steps and compares the md5s.',
    ]
}

// The whole document, as pretty-printed JSON text.
//   launch  [pipeline: map of the launcher's facts (main.nf's launchRecord), build_source:
//           buildSourceRecord() or null, params: every resolved parameter]
//   run     settings() of main.nf
//   parts   [index: BUILD_INDEX's index directory, digest: DIGEST's digest.tsv, shards: SHARD's
//           shard_<accession>.tsv rows, stats: KEYS' keys_<chrom>.tsv, sources and coverage:
//           REPORT's reports/sources.tsv and coverage.tsv, regions: REGIONS' summary.tsv,
//           provenance: the provenance files BUILD_INDEX and REPORT read, dates: [registry:,
//           harvest:] of the Registry tasks, statuses: the checks' '<check>:<chrom>:<status>']
def metadataDocument(Map launch, Map run, Map parts) {
    def manifestText = parts.index.resolve('manifest.json').text
    def manifest = manifestText.trim() ? new groovy.json.JsonSlurper().parseText(manifestText) : [:]

    // digest.tsv: tool <name> <value> | file <role> <name> <size> <md5> | gate <failed check>
    def digest = tsvRows(parts.digest)
    def tool = digest.findAll { r -> r[0] == 'tool' }.collectEntries { r -> [r[1], r[2]] }
    def files = digest.findAll { r -> r[0] == 'file' }
    def gateFailures = digest.findAll { r -> r[0] == 'gate' }.collect { r -> r[1] }

    def sourcesRows = tsvRecords(parts.sources)

    // The shards: one row per chromosome from SHARD.
    def shards = parts.shards.collectMany { f -> tsvRecords(f) }.collect { r ->
        [file: r.file, md5: r.gz_md5, size: number(r.gz_size), tsv_md5: r.tsv_md5, tsv_size: number(r.tsv_size), lines: number(r.lines)]
    }.toSorted { s -> s.file }
    def indexFiles = filesOf(files, 'index').collectEntries { f -> [f.name, [size: f.size, md5: f.md5]] }

    // The regions REGIONS cut: its summary per chromosome, and the totals.
    def summary = tsvRecords(parts.regions)
    def perChromRegions = summary.collectEntries { r ->
        [r.chrom, r.findAll { k, _v -> k != 'chrom' }.collectEntries { k, v -> [k, number(v)] }]
    }

    def w = run.windowing
    def bed = run.bed ? [name: run.bed.name, path: run.bed.toUriString(), md5: hexDigest('MD5', run.bed.bytes)] : null

    // The genome: built from hg38.fa.gz (its md5 from DIGEST, which FETCH_FASTA checked against
    // UCSC's md5sum.txt) or given with --genome.
    def genomeFiles = filesOf(files, 'genome')
    def genome = run.genome
        ? [built: false, path: run.genome[0].toUriString(), files: genomeFiles]
        : [built: true, contigs: run.chroms, fasta: (filesOf(files, 'fasta') ? filesOf(files, 'fasta')[0] + [url: "${launch.params.ucsc_base}/bigZips/hg38.fa.gz".toString(), md5_check: 'checked against bigZips/md5sum.txt'] : null), files: genomeFiles]

    def clinvarDate = parts.provenance.find { f -> f.name == 'clinvar_date.txt' }
    def gnomadListing = parts.provenance.find { f -> f.name == 'gnomad_files.tsv' }
    def gnomadHex = sourcesRows.findAll { r -> r.source?.startsWith('gnomad_') }.collectEntries { r -> [r.url, r.md5] }
    def gnomad = gnomadListing
        ? [
            version: launch.params.gnomad_version,
            mirror: launch.params.gnomad_mirror,
            files: tsvRows(gnomadListing).collect { r ->
                [dataset: r[0], chrom: r[1], url: r[2], md5_base64: r[3], md5: gnomadHex[r[2]], size: number(r[4])]
            },
        ]
        : null

    // The --cache directories, numbered in --cache order as DIGEST staged them (cache1, cache2,
    // ...; a single one as cache). The combined digest covers names and md5s only, so a copy of
    // the directories elsewhere (a shared filesystem, S3) gives the same value.
    def cacheDirs = run.cache.withIndex().collect { dir, i ->
        def entries = files.findAll { r -> r[1] == "cache${i + 1}".toString() || (run.cache.size() == 1 && r[1] == 'cache') }
            .collect { r -> fileRecord(r) }.toSorted { f -> f.name }
        [path: dir.toUriString(), files: entries]
    }
    def cacheManifest = cacheDirs.collectMany { d -> d.files.collect { f -> "${f.md5}  ${f.name}\n" } }.join('')
    def cache = [
        directories: cacheDirs,
        digest: run.cache ? hexDigest('SHA-256', cacheManifest.getBytes('UTF-8')) : null,
        digest_note: 'sha256 of the lines \'<md5>  <name>\' of every file, directory by directory in --cache order, ' +
            'files in byte order of name; the files are those SPLIT_CACHE reads (*.tsv and *.tsv.gz, not in subdirectories).',
        note: 'Answers taken from the cache were fetched before this run, by the harvest that wrote them; the ' +
            'manifest\'s harvest_date covers only the answers this run fetched (its earliest ENUMERATE or LOOKUP day).',
    ]

    // The per-chromosome counts of the gate's inputs and of the report.
    def coverage = tsvRecords(parts.coverage)
    def perChromCounts = [:]
    coverage.each { r -> perChromCounts.get(r.chrom, [:])["${r.source}:${r.measure}".toString()] = number(r.value) }
    def keysStats = parts.stats.collectMany { f -> tsvRecords(f) }.collectEntries { r ->
        [r.chrom, r.findAll { k, _v -> k != 'chrom' }.collectEntries { k, v -> [k, k == 'accession' ? v : number(v)] }]
    }

    def doc = [
        format_version: metadataFormatVersion(),
        about: 'How this index was made, and how to rebuild caid.fst and manifest.json byte for byte from the shards ' +
            'in ../keys/ (rebuild). Rerunning the pipeline regenerates rather than reproduces (regenerate). ' +
            'README.md, Reproducing an index.',
        pipeline: launch.pipeline + [written: java.time.Instant.now().toString()],
        tool: [
            clingen_index_version: tool.clingen_index_version,
            clingen_index_sha256: tool.clingen_index_sha256,
            clingen_index_path: tool.clingen_index_path,
            platform: tool.platform,
            build_source: launch.build_source,
            vareffect_version: manifest.vareffect_version,
            note: 'clingen_index_version, clingen_index_sha256, clingen_index_path and platform (uname -sm) are ' +
                'taken by DIGEST inside the task image every other task ran in. The version is the crate\'s, which is not ' +
                'raised with each change to crates/, so it does not name the binary; clingen_index_sha256 ' +
                'does, for one platform. build_source digests the image\'s build inputs (Dockerfile, Cargo.toml, ' +
                'Cargo.lock, rust-toolchain.toml, crates/) in the launcher\'s checkout, and describes the image only ' +
                'if the image was built from that checkout. To get the binary again: check out pipeline.git_commit ' +
                '(or a copy whose build_source.digest matches), docker build the image on the same platform, and ' +
                'compare the sha256 of its /usr/local/bin/clingen-index with clingen_index_sha256.',
        ],
        params: launch.params,
        regions: [
            set: w.set,
            bed: bed,
            pad: w.pad,
            merge_gap: w.merge_gap,
            max_window: w.max_window,
            flank: w.flank,
            windows_per_task: w.per_task,
            chromosomes: run.chroms,
            windows: columnTotal(summary, 'windows'),
            core_bp: columnTotal(summary, 'core_bp'),
            query_bp: columnTotal(summary, 'query_bp'),
            per_chromosome: perChromRegions,
        ],
        registry: [
            base_url: launch.params.registry_url,
            registry_date: parts.dates?.registry,
            harvest_date: parts.dates?.harvest,
            dates_note: 'registry_date: the UTC day of the earliest ENUMERATE task; harvest_date: of the earliest ' +
                'ENUMERATE or LOOKUP task. A task -resume reused keeps the day it ran.',
            page_size: run.paging.page_size,
            split_overlap: run.paging.split_overlap,
            include_reference: run.include_reference,
            lookup_batch_size: run.batch_size,
        ],
        sources: [
            downloads: sourcesRows.collect { r -> r.collectEntries { k, v -> [k, k == 'size' ? number(v) : v] } },
            clinvar: clinvarDate ? [release: clinvarDate.text.trim(), vcf: launch.params.clinvar_vcf] : null,
            gnomad: gnomad,
            datasets: run.datasets,
            genome: genome,
        ],
        cache: cache,
        outputs: [
            index: indexFiles,
            entries: manifest.entries,
            keys: shards,
            keys_bytes: shards.sum(0L) { s -> s.size instanceof Long ? s.size : 0L },
            keys_note: 'keys/<accession>.tsv.gz: the byte-sorted HGVS<TAB>CAID pairs build --sorted read, one shard per ' +
                'chromosome, compressed with gzip -n (no name or time). md5 is the .tsv.gz\'s, tsv_md5 the ' +
                'uncompressed file\'s; another gzip may compress the same lines to other bytes, tsv_md5 stays.',
        ],
        checks: [
            gate: [
                passed: gateFailures.isEmpty(),
                failures: gateFailures,
                allow_mismatches: run.allow_mismatches,
                published: true,
            ],
            statuses: parts.statuses,
            keys: keysStats,
            per_chromosome: perChromCounts,
        ],
        rebuild: rebuildRecord(manifest, shards.collect { s -> s.file }, [
            'caid.fst': indexFiles['caid.fst']?.md5,
            'manifest.json': indexFiles['manifest.json']?.md5,
        ]),
        regenerate: [
            command_line: launch.pipeline.command_line,
            note: 'Running this command again regenerates the index rather than reproducing it: the Registry gains ' +
                'alleles over time and cannot be asked what it held on an earlier day, and the latest ClinVar, gnomAD ' +
                'and UCSC files change. Only the rebuild from keys/ gives these bytes again.',
        ],
    ]
    // prettyPrint writes an empty object or array over three lines, the middle one blank; a raw
    // newline is never inside a JSON string, so these can only be empty containers.
    return groovy.json.JsonOutput.prettyPrint(groovy.json.JsonOutput.toJson(doc))
        .replaceAll(/\{\n *\n *\}/, '{}')
        .replaceAll(/\[\n *\n *\]/, '[]')
}
