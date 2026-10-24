# Index regeneration pipeline — design

Date: 2026-10-24. Status: approved in conversation section by section; this
document is the written spec for review.

## Goal

A Nextflow pipeline that regenerates the ClinGen CA-ID index
(`caid.fst` + `manifest.json`) from current sources, for one chromosome or
for every chromosome, with the same run on a workstation, a SLURM cluster or
AWS Batch. The first full run covers **every allele the Registry has
registered in protein-coding exons ±20 bp**, plus ClinVar genome-wide, and
proves before publishing that the tools find every key of the index.

## What the user asked for, and the decisions taken

| Ask / question | Decision |
|---|---|
| Regenerate the index with the latest gnomAD and ClinVar | gnomAD resolves to the newest release in the public bucket (v4.1.1 on 2026-10-24); ClinVar is NCBI's weekly `clinvar.vcf.gz`. |
| Subset one chromosome or do everything | `--chrom` is one of `1`–`22`, `X`, `Y`, `MT` (a `chr` prefix accepted) or `all`; it narrows every source. |
| Run examples with both profiles; they differ only in the runner | Profiles `local`, `slurm`, `awsbatch` share one task image and all resource settings; each sets the executor, the container engine and, for AWS, an S3 work directory. |
| Are we missing Registry alleles outside gnomAD and ClinVar? | Yes: measured 60% of non-reference Registry alleles in random chr21 windows and 85% inside chr21 protein-coding exons. The Registry's region query (below) returns them. |
| Focus on all possibilities within genes/exome first | The Registry is enumerated by region over a gene/exome region set; the first run uses protein-coding exons ±20 bp. Every possible substitution is not looked up separately: the Registry can only give a CA ID to a registered allele, and enumeration returns every registered allele (66% of all possible SNVs in sampled exons, plus indels). |
| Approach | A: Nextflow orchestrates; all logic that needs testing is in `clingen-index` (Rust). |
| Python vs Rust | All Rust; no Python in the pipeline. Python is not too slow (it parses Registry JSON at 402,000 alleles/s against the Registry's ~1,600/s), but one binary is simpler to provision on every node and shares code with `clingen-allele`. |
| Requirements, Dockerfile, AWS Batch | Launcher requirements pinned in the repo; one Dockerfile for the task image; an AWS Batch profile, a CloudFormation template and a setup guide. |

## Measured facts the design rests on

- **Registry region query.** `GET https://reg.genome.network/alleles?refseq=<NC_…>&begin=<b>&end=<e>&fields=…&limit=<L>&skip=<S>`
  returns the canonical alleles lying wholly inside the window: start > `b`
  and end ≤ `e` (40 of 14,134 corpus alleles in sampled windows were absent,
  every one crossing a window edge). Without `limit` it returned all 57,093
  alleles of a 50 kb window, so no server cap was seen below that.
- **Cost per request.** ≈ 0.71 s fixed plus ≈ 0.10 ms per allele (1 kb:
  1,502 alleles in 0.86 s; 50 kb: 57,093 in 6.57 s). Dropping
  `externalRecords` saves bytes but not time (964 vs 907 alleles/s).
- **Density.** Protein-coding exons: 2,671 alleles/kb (2,152 non-reference);
  random chr21 windows: ~1,140/kb. About a fifth to a third of alleles are
  reference-only (`g.N=`), which no tool ever looks up.
- **Region sets** (RefSeq ∪ GENCODE from UCSC, primary contigs): CDS ±20 bp
  49.5 Mb; protein-coding exons ±20 bp 148.2 Mb in 254,888 intervals; all
  exons ±20 bp 271.7 Mb; protein-coding gene bodies 1,343 Mb.
- **Windows.** Merging padded protein-coding exons within 5 kb and splitting
  at 50 kb gives 60,328 requests over 406.7 Mb, an estimated **~31 h** with
  one request stream and no fixed pause (vs ~5 days for one request per
  exon), and roughly 550M non-reference keys (~6.5 GB index).
- **Lookup harvest.** POST lookups ran at ~2,100 sites/s (chr21, ClinVar).
- **Conformance so far.** 0 HGVS mismatches over 10,958,898 gnomAD chr21 rows
  and 4,571,909 ClinVar rows (every class, including 22,417 `delins` and 3,134
  `m.`); CA IDs are zero-padded to six digits (fixed in `506ccba`).

## Architecture

```
UCSC tables ─► REGIONS ─► windows (per chrom, batched) ─► ENUMERATE ──────────────┐
ClinVar ─► CLINVAR_SITES ─┐                                                        │
gnomAD (opt) ─► GNOMAD_SITES ─┼─► MERGE_SITES (per chrom) ─► LOOKUP (batch) ─► COLLECT ─► harvest/<chrom>.tsv.gz
--cache ─► SPLIT_CACHE ───┘            └─ cached answers ─────────────────────────┘          │
                                                                                              ▼
hg38.fa.gz ─► BUILD_GENOME ──────────────────────────────► VALIDATE (per chrom, harvest rows)
                                    KEYS (per chrom: enumerated + registered harvest, sort -u)
                                      └─► BUILD_INDEX (build --sorted) ─► REACH (per chrom) ─► FINALIZE ─► index/, genome/
                                                                                       └────► REPORT ─► reports/
```

Nextflow owns staging of downloads, batching (`splitText`/`splitCsv`),
parallelism, retries, `-resume`, publishing, parameter checking and the run
reports. Each process runs one `clingen-index` subcommand or a short shell
pipeline (`curl`, `gunzip`, `awk`, `sort`, `gzip`, `openssl`).

### Sources

- **`registry`** (always on): `REGIONS` builds the region set and windows;
  `ENUMERATE` runs one batch of windows (one chromosome per batch) and emits
  `HGVS\tCAID` for every non-reference allele. The windows are queried with
  200 bp flanks so alleles crossing a core interval's edge are kept;
  duplicates from overlapping flanks are removed by `KEYS`.
- **`clinvar`** (default on): genome-wide, as in the ClinVar coverage run:
  records on non-primary contigs, with `ALT=.` or with a non-ACGT base are
  written to `skipped` with a reason; the rest are looked up.
- **`gnomad_genomes`, `gnomad_exomes`** (opt-in via `--datasets`): streamed
  per chromosome, never stored; `*`, `.` and non-ACGT alleles skipped.
  gnomAD v4 has no chrM, so `MT` uses v3.1's chrM file. Inside the regions,
  enumeration already returns every gnomAD allele the Registry has.

The index holds every enumerated non-reference allele plus every registered
harvested row, for the requested chromosomes. Harvested rows also serve as
the conformance set (`VALIDATE`).

### Index build at scale

Registry HGVS keys start with the chromosome's accession (`NC_000001.11:`,
…, `NC_000024.10:`, `NC_012920.1:`), so per-chromosome shards sorted with
`LC_ALL=C sort -u` and concatenated in accession order are globally sorted.
`build --sorted` streams them into the FST with constant memory and checks
the order itself, so a wrong locale cannot produce a bad index.

### Publishing

`FINALIZE` runs only after every `VALIDATE` and `REACH` task passed, and
copies `index/` (and `genome/` when the pipeline built it) to `--outdir`.
With a built genome the manifest records it as `../genome/GRCh38.bin`,
relative to the index directory, so the pair can be moved together; a
`--genome` the user supplied is recorded as an absolute path and not copied.

## Components

### `clingen-index` subcommands (Rust)

`clingen-allele` stays free of network code; the new dependencies (`ureq`
with `rustls` for HTTPS, `flate2` for gzip) go into `clingen-index` only.

1. **`genome --fasta <PATH|-> [--contigs chr21,…] --out <GRCh38.bin>`** —
   writes the 25 GRCh38 primary contigs (UCSC or Ensembl names) or the ones
   requested to a vareffect genome binary and `.idx`; refuses a contig whose
   length is not GRCh38's (the table in `contig.rs`). Replaces the
   `fasta_to_bin` example; `scripts/build_chr21_genome.sh` calls it.
2. **`build --sorted <PAIRS>… --out <DIR> [--genome <BIN> [--genome-relative]]`** —
   streams byte-sorted `HGVS\tCAID` files, in the order given, into the index;
   skips exact repeats; a key with two CA IDs is `Error::DuplicateConflict`;
   an out-of-order key is an error naming file and line. Same atomic replace
   and manifest fields as `build --chunks`, which stays for small runs.
3. **`reach --index <DIR> [--genome <BIN>] [--prefix <NC_…:>]`** — walks every
   key (or one accession's keys), converts its HGVS to a VCF record with the
   genome (`>`, `del`, `dup`, `ins`, `delins` on `g.` and `m.`), runs
   `Annotator::lookup` and requires the key's own CA ID back. Prints counts by
   class (reachable, wrong CA, not found, unparseable) with examples; exits 1
   if any key is not reachable.
4. **`regions --refseq <ncbiRefSeq.txt.gz> --gencode <knownGene.txt.gz> --set <cds|pc_exons|exons|genes> | --bed <BED>`**
   `[--pad 20] [--merge-gap 5000] [--max-window 50000] [--flank 200] [--chrom <C>]` —
   writes `windows.tsv` (chrom, query begin, query end, core begin, core end)
   and a summary of bases covered.
5. **`registry enumerate --windows <TSV> --out <TSV>`** — queries each window
   (`fields=none+@id+genomicAlleles.hgvs+genomicAlleles.referenceGenome`),
   pages with `limit=50000` and `skip` while a page is full, drops `=`
   alleles, keeps the GRCh38 `NC_` HGVS, and writes `HGVS\tCAID`. Retries
   network errors, 5xx and 429 with exponential backoff (6 attempts, 30 s
   doubling); any other 4xx fails.
6. **`registry lookup --sites <TSV> --out <TSV> [--batch 250000]`** — the
   Rust counterpart of `harvest/query_registry.py` (read-only `POST /alleles`,
   same seven output columns, same retries). The Python script stays for the
   README's toy recipe.

### Pipeline (`pipeline/`)

```
pipeline/
  main.nf                  workflow, parameter checks
  modules/sources.nf       RESOLVE_GNOMAD, GNOMAD_SITES, CLINVAR_SITES, FETCH_*
  modules/registry.nf      REGIONS, ENUMERATE, LOOKUP
  modules/index.nf         SPLIT_CACHE, MERGE_SITES, COLLECT, BUILD_GENOME, KEYS,
                           BUILD_INDEX, VALIDATE, REACH, FINALIZE, REPORT
  nextflow.config          params, labels, profiles, manifest (nextflowVersion)
  conf/{local,slurm,awsbatch,host}.config
  nextflow_schema.json     nf-schema: checked params and --help
  environment.yml          launcher requirements (Java, Nextflow, nf-test), pinned
  tests/                   nf-test
  aws/clingen-batch.yaml   CloudFormation for the AWS Batch setup
Dockerfile, .dockerignore  task image (repo root, so the build sees crates/)
docs/aws-batch.md          AWS Batch setup guide
```

**Parameters** (`--help` from the schema):

| Param | Default | Meaning |
|---|---|---|
| `--chrom` | required | `1`–`22`, `X`, `Y`, `MT`, or `all` |
| `--regions` | `pc_exons` | `cds`, `pc_exons`, `exons`, `genes`, or a BED path |
| `--pad`, `--merge_gap`, `--max_window`, `--flank` | 20, 5000, 50000, 200 | window construction |
| `--datasets` | `clinvar` | any of `clinvar`, `gnomad_genomes`, `gnomad_exomes` |
| `--gnomad_version` | `latest` | or a pinned version such as `4.1.1` |
| `--gnomad_mirror` | `gcs` | `gcs` or `aws` (AWS Open Data, us-east-1); md5s always from GCS metadata |
| `--clinvar_vcf` | `latest` | or a local path, for an exact rerun |
| `--genome` | built | prebuilt vareffect genome; otherwise built from UCSC `hg38.fa.gz` |
| `--cache` | none | comma-separated earlier `harvest/` directories; registered answers reused |
| `--windows_per_task`, `--batch_size` | 500, 2,000,000 | task sizes |
| `--allow_mismatches` | false | publish despite validate/reach failures |
| `--container` | `clingen-index:0.1.0` | task image |
| `--slurm_queue`, `--network_queue` | — | SLURM partitions (`network` label may go to a transfer partition) |
| `--aws_queue`, `--aws_region` | —, `us-east-1` | AWS Batch |
| `--outdir` | `results` | local path or `s3://…` |

**Labels.** `network` (Registry, downloads, gnomAD streams), `sort`
(4 CPUs, 16 GB), `genome` (8 GB), `build` (2 GB), `small`. `ENUMERATE` and
`LOOKUP` each have `maxForks 1`. Every process has a `stub:` block.

**Outputs** in `--outdir`: `index/` (`caid.fst`, `manifest.json` with
sources such as `registry_pc_exons_2026-10-24`, `clinvar_2026-10-19`,
`gnomad_genomes_v4.1.1`, and the chromosome), `genome/` (if built),
`harvest/<chrom>.tsv.gz` (the next run's `--cache`), `reports/`
(`sources.tsv`, `skipped.tsv.gz`, `validate_<chrom>.txt`,
`reach_<chrom>.txt`, `coverage.tsv`, `summary.md`), and Nextflow's
`report.html`, `timeline.html`, `trace.txt`.

### Requirements in the repo

- `pipeline/environment.yml` (conda-forge + bioconda): `openjdk` 21,
  `nextflow`, `nf-test`, each pinned to a release at least two weeks old when
  written. `nextflow.config` sets `manifest.nextflowVersion` to the same
  floor and pins the `nf-schema` plugin version.
- The task image carries everything a task runs; the host needs only Java,
  Nextflow and a container engine (Docker, Apptainer, or AWS Batch).
- `rust-toolchain.toml` (1.94.1) already pins Rust for building
  `clingen-index` outside Docker.

### Dockerfile

Multi-stage. Build stage: `rust:1.94.1-bookworm`, `cargo build --release
--locked -p clingen-index`. Runtime stage: `debian:bookworm-slim` with
`bash`, `coreutils`, `gawk`, `gzip`, `curl`, `ca-certificates`, `procps`
(Nextflow task metrics), `openssl` (md5), the AWS CLI v2 (pinned installer,
needed by Nextflow's S3 staging on AWS Batch; harmless elsewhere) and
`/usr/local/bin/clingen-index`. No entrypoint, so Nextflow's bash launcher
runs. Architecture-neutral; the guide shows `--platform linux/amd64` and
`linux/arm64` builds. `.dockerignore` excludes `data/`, `target/`, `.git/`.

### Profiles and run examples

All profiles share labels, retries and the image; they differ in executor,
container engine and work directory.

| Profile | Executor | Container | Work dir |
|---|---|---|---|
| `local` | `local` | Docker (`docker.enabled`) | `./work` |
| `slurm` | `slurm` (`--slurm_queue`; `network` label → `--network_queue` if set) | Apptainer (`docker://` image from a registry) | shared filesystem |
| `awsbatch` | `awsbatch` (`--aws_queue`) | the image in ECR | `s3://…/work` |
| `host` (add-on) | — | none: `clingen-index`, `curl`, `awk`… from `PATH` | — |

```bash
# one chromosome on this machine
nextflow run pipeline -profile local --chrom 21 --cache data/chr21_chunks,data/clinvar/20261019/chunks --outdir results/chr21

# protein-coding exons everywhere, on a SLURM cluster
nextflow run pipeline -profile slurm --chrom all --slurm_queue normal --network_queue transfer \
  --container ghcr.io/<you>/clingen-index:0.1.0 --outdir /scratch/clingen/pc_exons -resume

# the same on AWS Batch
nextflow run pipeline -profile awsbatch --chrom all --aws_queue clingen-spot --gnomad_mirror aws \
  --container <account>.dkr.ecr.us-east-1.amazonaws.com/clingen-index:0.1.0 \
  -work-dir s3://<bucket>/work --outdir s3://<bucket>/results/pc_exons -resume
```

### AWS Batch setup (`docs/aws-batch.md` + `pipeline/aws/clingen-batch.yaml`)

The CloudFormation template creates: an S3 bucket (work and results, with a
lifecycle rule expiring `work/` after 30 days), an ECR repository, the
instance role (S3 access to that bucket, `AmazonEC2ContainerServiceforEC2Role`,
ECR read-only) and instance profile, a launch template enlarging the EBS
volume for sort scratch, a managed EC2 compute environment (Spot or
on-demand, `optimal` instances, max vCPUs a parameter) in subnets the user
names (they need internet egress: the Registry, NCBI, UCSC, GCS), and a job
queue. Outputs: bucket, queue and ECR URI. The guide covers: prerequisites
(AWS CLI v2, credentials), deploying the stack, building and pushing the
image, the IAM permissions the Nextflow launcher needs (the Batch, EC2/ECS
describe and ECR actions in the Nextflow AWS docs), where to run the
launcher for a ~31 h run (a small EC2 instance in `tmux`, or a Batch job),
cost notes, using `--gnomad_mirror aws` in us-east-1, and teardown. Fusion
is mentioned as an alternative to the AWS CLI in the image.

## Error handling

- Registry: at most one enumerate and one lookup stream; client backoff as
  above; Nextflow `errorStrategy 'retry'`, `maxRetries 2`, then the run
  stops and `-resume` continues from the first unfinished task.
- Gates: any `validate` mismatch or unreachable key fails before
  `FINALIZE`, unless `--allow_mismatches`.
- Integrity: md5 checked for ClinVar (NCBI `.md5`), hg38 (UCSC
  `md5sum.txt`) and gnomAD (GCS `md5Hash`, also for the AWS mirror); UCSC
  transcript tables have none, so their size and `Last-Modified` go into
  `sources.tsv`.
- Limits stated in `summary.md`: alleles longer than the flank that cross a
  window edge are not returned; outside the windows, coverage comes only
  from the harvested datasets; enumeration is not incremental across runs
  (the region query has no "registered since").
- `--chrom` narrows every source; an `MT` run with gnomAD uses v3.1 chrM.

## Testing

- **Rust**, per new subcommand: `regions` (tiny UCSC fixtures: sets, padding,
  merge gap, window split, flanks, `--chrom`, BED input); `registry` against
  an in-process mock HTTP server (paging, backoff on 5xx/429, failing 4xx,
  truncated JSON, `=` filtering, `_:CA` lookups); `build --sorted` (order,
  repeats, conflicts, atomic replace, relative genome); `reach` (each HGVS
  form on the tiny genome, position-1 edge, unparseable key, wrong CA);
  `genome` (tiny FASTA, contig subset, wrong-length refusal).
- **Real data**: `reach` over all 15,528,839 keys of `data/index_corpus`
  must report every key reachable; `registry enumerate` on chr21 windows must
  reproduce the probe counts.
- **nf-test**: a stub test for every process and the workflow; a real
  workflow test with `--chrom 21`, a BED of a few exons, a local ClinVar
  subset and a tiny genome, snapshotting the outputs.
- **Profiles**: `local` runs the real chr21 test in Docker (if a container
  engine is available here); `slurm` runs the stub workflow through
  `sbatch`/`squeue`/`scancel` shims that execute jobs locally; `awsbatch` is
  checked with `nextflow config -profile awsbatch` and `cfn-lint` on the
  template. A real cluster or AWS deployment is untested and said so.
- **First real run**: `-profile local --chrom 21 --regions pc_exons`
  (~600 windows, about 20 minutes of enumeration), then both tools
  cross-checked end to end on ClinVar chr21. The ~31 h exome run starts only
  when the user says so.

## Docs

README: a "Regenerating the index (Nextflow)" section with requirements,
the three run examples, outputs, measured timings and limits; links to
`docs/aws-batch.md`. The ClinVar coverage section points to the pipeline for
the whole-genome binary.

## Out of scope

Whole-genome enumeration (~3.4B alleles; needs the Registry team's
agreement); gnomAD genome-wide as a default; incremental enumeration;
publishing images or deploying AWS resources (documented, not done).
