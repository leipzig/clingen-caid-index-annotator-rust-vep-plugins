# clingen

Offline ClinGen Allele Registry annotation for
[vep-rs](https://github.com/natera-open-source/vep-rs) and
[fastVEP](https://github.com/Huang-lab/fastVEP).

The [ClinGen Allele Registry](https://reg.clinicalgenome.org) gives every
genetic variant a canonical allele identifier, the **CA ID** (e.g.
`CA120560`). This project:

1. **Harvests** CA IDs and canonical HGVS from the Registry once, using its
   read-only lookup API, for a defined set of sites (starting with gnomAD v4.1).
2. **Indexes** them locally as an HGVS → CA ID map.
3. **Annotates** VCFs inside vep-rs and fastVEP from that index, with **no
   network calls**. Each variant is first converted to the Registry's canonical
   genomic HGVS by [vareffect](https://docs.rs/vareffect), then looked up.

The key is HGVS rather than CHROM/POS/REF/ALT because HGVS is normalized. The
same indel can be written several ways in a VCF (left-aligned, right-aligned,
padded), but it has exactly one canonical HGVS, and so one CA ID.

## What you can do with CA IDs

- **Name a variant the same way everywhere.** One CA ID covers the allele on
  GRCh38, on GRCh37, and in its transcript (`c.`) forms. Protein-level changes
  get their own `PA` IDs, linked from the CA record. The Registry returns the
  equivalent forms together (e.g. `NC_000021.9:g.5030578C>T` and
  `NC_000021.8:g.…` for the same CA ID). Join datasets from different
  assemblies, callers and normalizers on one key.
- **Deduplicate variant calls.** Two VCF records that describe the same allele
  differently get the same CA ID. See `ins_left` and `ins_right` in the toy
  run below.
- **Reach ClinGen curation.** Expert-panel classifications in the
  [ClinGen Evidence Repository](https://erepo.clinicalgenome.org) and evidence in
  the ClinGen Linked Data Hub are indexed by CA ID. Look up
  `https://reg.clinicalgenome.org/allele/<CAID>` for the allele's record.
- **Cross-reference other databases.** Registry records link a CA ID to dbSNP
  rsIDs, ClinVar variation IDs, gnomAD and other external records. That makes a
  CA ID a convenient bridge between resources that each use their own
  identifier.
- **Share data without ambiguity.** Exchanging CA IDs instead of raw VCF lines
  removes the "which normalization did you use?" question between labs.

## Status

| Piece | State |
|---|---|
| Harvester (`harvest/`): Registry lookup for a sites list | **works**; the [lookup recipe](#harvest-step-look-up-a-toy-vcf-in-the-live-registry) runs it |
| gnomAD v4.1 chr21 harvest | **done** (2026-10-23): 10,958,898 sites, all registered |
| `clingen-allele` crate (vareffect HGVS + local index) | **works**; on local `main` |
| `clingen-index` CLI (`genome`, `build [--sorted]`, `regions`, `registry enumerate\|lookup`, `validate [--strict]`, `reach`) | **works**; `build` and `validate` on local `main`; the rest, with `build --sorted` and `validate --strict`, on `feature/regeneration-pipeline` |
| vep-rs plugin `--plugin ClinGen` | **works** offline; merged into a local vep-rs `main`, not pushed |
| fastVEP provider (`FV_CLINGEN`) | **works** offline; merged into a local fastVEP `master`, not pushed |
| chr21 conformance with the Registry | all 10,958,898 rows match the Registry's HGVS ([details](docs/known-differences.md)) |
| ClinVar 2026-10-19, every chromosome | all 4,571,909 harvested rows match the Registry's HGVS, 22,417 `delins` and 3,134 chrM among them; both tools give the Registry's CA ID for all 4,595,314 registered ClinVar alleles ([ClinVar coverage](#clinvar-coverage)) |
| End-to-end check, first 100,000 chr21 sites | both tools: all 100,000 CA IDs agree with the harvest ([below](#end-to-end-check)) |
| Registry region enumeration (`clingen-index registry enumerate`) | **works**; it never pages with `skip`, which loses alleles on the live Registry ([Enumerating the Registry by region](#enumerating-the-registry-by-region)) |
| Index regeneration pipeline (`pipeline/`, Nextflow) | **works** locally under Docker: chr21 protein-coding exons plus ClinVar on 2026-10-24, 5,212,187 keys, gate passed, both tools give the Registry's CA ID for all 51,692 registered ClinVar chr21 alleles ([Regenerating the index](#regenerating-the-index-nextflow)); SLURM tested only through shims, AWS Batch documented but not deployed, the full exome run not run |
| Demo data | chr21 only, genome and index (see [Known limitations](#known-limitations)) |

Design: [`docs/superpowers/specs/2026-10-23-clingen-allele-plugin-design.md`](docs/superpowers/specs/2026-10-23-clingen-allele-plugin-design.md).
Implementation plan: [`docs/superpowers/plans/2026-10-23-clingen-allele-plugin.md`](docs/superpowers/plans/2026-10-23-clingen-allele-plugin.md).

## Annotating a VCF offline

Four steps: a genome, an index, a tool built with ClinGen support, and the
run. Setting up uses the network (the harvest, the genome download, cargo's
crate downloads); annotating does not.

**Requirements.**

- Rust 1.94.1 or newer, which `clingen-allele` and vareffect need. This repo's
  `rust-toolchain.toml` pins 1.94.1.
- The three checkouts side by side in one directory, this one named `clingen`:

  ```
  clingen/   this repo, branch feature/clingen-allele
  vep-rs/    branch clingen-allele-plugin
  fastVEP/   branch clingen-allele-plugin
  ```

  **LOCAL-ONLY:** both adapters depend on `clingen-allele` by the path
  `../clingen/crates/clingen-allele`, because the crate is not published yet.
  Cargo reads a path dependency's manifest even when the feature is off, so
  those branches do not build at all without this checkout next to them.
- The harvest in `data/chr21_chunks` (see
  [Harvesting a chromosome](#harvesting-a-chromosome)).

All commands below run from `clingen/`. Everything they write goes under
`data/`, which is gitignored.

### 1. The genome (once)

The index is keyed by canonical HGVS, which vareffect computes from the
reference genome in its flat binary format (a `.bin` with its `.bin.idx`). The
demo uses chr21 alone, 46.7 MB:

```bash
scripts/build_chr21_genome.sh data/genome_chr21   # downloads UCSC hg38 chr21.fa.gz
```

A whole-genome index needs all 25 primary contigs instead, about 3.1 GB,
which `clingen-index genome` writes from UCSC's `hg38.fa.gz`
([ClinVar coverage](#clinvar-coverage)).

### 2. The index (once per harvest)

First validate the harvest. `validate` recomputes every harvested allele's
HGVS from the genome and compares it with the Registry's, class by class:

```bash
cargo run -p clingen-index --release -- validate --chunks data/chr21_chunks \
    --genome data/genome_chr21/chr21.bin --mismatches data/chr21_mismatches.tsv
```

Run it on every new harvest before building. It exits 1 only when a
substitution mismatches, so read the `total` and `match` columns of every
class: mismatching indels would miss (`not_in_index`), and a class whose
`total` is 0 has not been checked at all. A mismatch pattern goes in
[`docs/known-differences.md`](docs/known-differences.md). The chr21 result is
in [chr21 index](#chr21-index).

Then build:

```bash
cargo run -p clingen-index --release -- build --chunks data/chr21_chunks \
    --out data/index_chr21 --source gnomad41_genomes_chr21 \
    --genome "$PWD/data/genome_chr21/chr21.bin" --harvest-date 2026-10-23
```

`build` records `--genome` in `manifest.json` as an absolute path, and both
tools use it when they are not given a genome. Rebuilding into the same
`--out` replaces the index by rename: a running tool keeps reading the old
one, and a build that fails before the rename leaves it in place. `build
--chunks` holds every row in memory; `build --sorted` reads (HGVS, CA ID)
pairs already in byte order and runs in constant memory, which is how the
regeneration pipeline builds (see [Known limitations](#known-limitations)).
[chr21 index](#chr21-index) has its size.

### 3. The tools

ClinGen support is an optional cargo feature in both, off by default. vep-rs
builds with your default toolchain, which must be 1.94.1 or newer for the
feature; fastVEP's `rust-toolchain.toml` selects `stable`.

```bash
(cd ../vep-rs && cargo build --release -p vep-cli --features clingen --bin vep)
(cd ../fastVEP && cargo build --release -p fastvep-cli --features clingen)
```

- **vep-rs:** `--plugin ClinGen,index=DIR[,genome=PATH]` adds the CSQ
  subfields `ClinGen_CAID` and `ClinGen_HGVSg`. `genome=` defaults to the
  manifest's. With `--vcf` output it needs no FASTA and no cache; without
  `--json_cache` every allele is reported `intergenic_variant`, which is
  enough for the ClinGen fields. See vep-rs `docs/plugins/clingen.md`.
- **fastVEP:** the index directory goes, under any name, inside the directory
  given to `--sa-dir`, one index per `--sa-dir`. It adds INFO
  `FV_CLINGEN=ALLELE|CAID|HGVSG`, one entry per ALT (in JSON, `clingen` with
  `caid` and `hgvsg`). `--clingen-genome PATH` overrides the manifest's
  genome, and `--sa-only` skips consequence prediction, so no GFF3 or FASTA is
  needed. See fastVEP `docs/SUPPLEMENTARY_ANNOTATIONS.md`.

For fastVEP, a symlink puts the index in an `--sa-dir`:

```bash
mkdir -p data/sa_clingen && ln -sfn ../index_chr21 data/sa_clingen/clingen_chr21
```

At the end of a run both tools report one tally,
`ClinGen: found=… not_in_index=… ref_mismatch=… unsupported=…`, which counts
each allele once. `found` and `not_in_index` count alleles with an HGVS, with
and without a CA ID; `ref_mismatch` counts a REF the genome disagrees with;
`unsupported` counts every other allele, including the ones a tool skips
(symbolic, `*`, REF equal to ALT). fastVEP prints the line on stderr. vep-rs
logs it at info level, after its log's time, level and source, so `--quiet`
hides it ([toy run](#4-run-the-toy-vcf)). The two tallies agree, except that
fastVEP counts an ALT repeated within one record (`C` → `T,T`) once.

### 4. Run the toy VCF

[`examples/toy.vcf`](examples/toy.vcf) (GRCh38):

```
##fileformat=VCFv4.2
##reference=GRCh38
##contig=<ID=chr21,length=46709983>
##contig=<ID=chrM,length=16569>
#CHROM	POS	ID	REF	ALT	QUAL	FILTER	INFO
chr21	5030578	snv	C	T	.	PASS	.
chr21	13189782	ins_left	C	CA	.	PASS	.
chr21	13189788	ins_right	A	AA	.	PASS	.
chr21	33000166	del_novel	TA	T	.	PASS	.
chrM	3243	melas	A	G	.	PASS	.
chr21	10000000	bad_ref	A	G	.	PASS	.
```

On macOS, `sandbox-exec` runs a command with all network access denied, which
shows that annotating makes no network calls. Elsewhere, leave `offline` out.

```bash
offline() { sandbox-exec -p '(version 1)(allow default)(deny network*)' "$@"; }
offline curl -sS https://example.com   # curl: (6) Could not resolve host: example.com
```

**vep-rs:**

```bash
mkdir -p data/toy
offline ../vep-rs/target/release/vep -i examples/toy.vcf -o data/toy/vep-rs.vcf \
    --vcf --offline --assembly GRCh38 --force_overwrite --no_stats --quiet \
    --plugin ClinGen,index=data/index_chr21
grep -v '^#' data/toy/vep-rs.vcf | cut -f 3,8
```

Output (stderr, then the ID and INFO columns), 2026-10-23. The last two CSQ
subfields are `ClinGen_CAID` and `ClinGen_HGVSg`:

```
clingen-allele: genome lookup failed for contig 'chrM' (reference genome error: chromosome 'chrM' not found in genome index); variants on this contig are reported Unsupported
snv	CSQ=T|intergenic_variant|MODIFIER|||||||||||||||||||||CA749862939|NC_000021.9:g.5030578C>T
ins_left	CSQ=A|intergenic_variant|MODIFIER|||||||||||||||||||||CA636937543|NC_000021.9:g.13189788dup
ins_right	CSQ=A|intergenic_variant|MODIFIER|||||||||||||||||||||CA636937543|NC_000021.9:g.13189788dup
del_novel	CSQ=-|intergenic_variant|MODIFIER||||||||||||||||||||||NC_000021.9:g.33000172del
melas	CSQ=G|intergenic_variant|MODIFIER||||||||||||||||||||||
bad_ref	CSQ=G|intergenic_variant|MODIFIER||||||||||||||||||||||
```

The warning comes from `clingen-allele` itself. vep-rs's tally is not there,
because `--quiet` keeps only errors in vep-rs's log. To see it, run without
`--quiet`; `NO_COLOR=1` keeps colour codes out of the log:

```bash
NO_COLOR=1 offline ../vep-rs/target/release/vep -i examples/toy.vcf -o data/toy/vep-rs.vcf \
    --vcf --offline --assembly GRCh38 --force_overwrite --no_stats \
    --plugin ClinGen,index=data/index_chr21 2>&1 | grep 'ClinGen:'
```

```
2026-10-23T23:16:25.622574Z  INFO vep_builtins::plugins::clingen: ClinGen: found=3 not_in_index=1 ref_mismatch=1 unsupported=1
```

**fastVEP:**

```bash
offline ../fastVEP/target/release/fastvep annotate -i examples/toy.vcf \
    -o data/toy/fastvep.vcf --sa-dir data/sa_clingen --sa-only --no-progress
grep -v '^#' data/toy/fastvep.vcf | cut -f 3,8
```

Output (stderr, then the ID and INFO columns), 2026-10-23:

```
Annotating: examples/toy.vcf -> data/toy/fastvep.vcf
Loaded 1 supplementary annotation source(s) from data/sa_clingen in 0.0s
clingen-allele: genome lookup failed for contig 'chrM' (reference genome error: chromosome 'chrM' not found in genome index); variants on this contig are reported Unsupported
ClinGen: found=3 not_in_index=1 ref_mismatch=1 unsupported=1
snv	FV_CLINGEN=T|CA749862939|NC_000021.9:g.5030578C>T
ins_left	FV_CLINGEN=CA|CA636937543|NC_000021.9:g.13189788dup
ins_right	FV_CLINGEN=AA|CA636937543|NC_000021.9:g.13189788dup
del_novel	FV_CLINGEN=T||NC_000021.9:g.33000172del
melas	.
bad_ref	.
```

Both tools give the same answer for every row:

- **`snv`**: found. CA ID `CA749862939` and HGVS `NC_000021.9:g.5030578C>T`.
- **`ins_left` / `ins_right`**: found, and the same allele. The one-base
  insertion in a run of A's, written left-aligned and right-aligned, becomes
  `NC_000021.9:g.13189788dup` both times, so both get `CA636937543`. This is
  the representation independence the HGVS key is for. (vep-rs's CSQ
  `Allele` is VEP's trimmed `A`; fastVEP's `ALLELE` is the VCF ALT.)
- **`del_novel`**: HGVS but no CA ID. The deletion is valid, so
  `NC_000021.9:g.33000172del` is computed, but the Registry has never
  registered it, so the CA ID is empty (`not_in_index=1`).
- **`melas`**: nothing. The demo genome and index are chr21 only, so chrM
  cannot be converted to HGVS; `clingen-allele` warns once for the contig
  and counts it `unsupported`. With a genome that has chrM and an index
  harvested from chrM sites it would be looked up like the others; the live
  Registry gives it `CA120560` (below).
- **`bad_ref`**: nothing. REF `A` disagrees with the genome (the GRCh38 base
  is `C`), so no HGVS is built (`ref_mismatch=1`). vep-rs still writes its
  CSQ entry, with both ClinGen fields empty; fastVEP writes no `FV_CLINGEN`.

For the four chr21 rows with a valid REF, these CA IDs and HGVS are exactly
those of the live lookup below.

## End-to-end check

The first 100,000 rows of the chr21 harvest (chr21:5,029,882–5,310,755,
10,304 of them indels) as a VCF, annotated by each tool offline, and every
CA ID compared with the harvest's:

```bash
mkdir -p data/e2e
{
  printf '##fileformat=VCFv4.2\n##contig=<ID=21,length=46709983,assembly=GRCh38>\n'
  printf '#CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\n'
  head -n 100000 data/chr21_chunks/chunk_00000.tsv |
    awk -F'\t' -v OFS='\t' '{print $1, $2, ".", $3, $4, ".", "PASS", "."}'
} > data/e2e/chr21_100k.vcf

offline ../vep-rs/target/release/vep -i data/e2e/chr21_100k.vcf -o data/e2e/vep-rs.vcf \
    --vcf --offline --assembly GRCh38 --force_overwrite --no_stats --quiet \
    --plugin ClinGen,index=data/index_chr21
offline ../fastVEP/target/release/fastvep annotate -i data/e2e/chr21_100k.vcf \
    -o data/e2e/fastvep.vcf --sa-dir data/sa_clingen --sa-only --no-progress

python3 -I scripts/e2e_crosscheck.py --harvest data/chr21_chunks \
    --vcf-out data/e2e/vep-rs.vcf --tool vep-rs
python3 -I scripts/e2e_crosscheck.py --harvest data/chr21_chunks \
    --vcf-out data/e2e/fastvep.vcf --tool fastvep
```

Result (2026-10-23; clingen `feature/clingen-allele` at `2d0347c`, vep-rs
`c3a9af6`, fastVEP `46dce28`). The two annotation runs print this on stderr,
all of it from fastVEP:

```
Annotating: data/e2e/chr21_100k.vcf -> data/e2e/fastvep.vcf
Loaded 1 supplementary annotation source(s) from data/sa_clingen in 0.0s
ClinGen: found=100000 not_in_index=0 ref_mismatch=0 unsupported=0
```

vep-rs prints nothing, because of `--quiet`. Run without it, and with
`NO_COLOR=1` as in the [toy run](#4-run-the-toy-vcf), its log ends with:

```
2026-10-23T23:11:46.034190Z  INFO vep_builtins::plugins::clingen: ClinGen: found=100000 not_in_index=0 ref_mismatch=0 unsupported=0
```

The vep-rs check, then the fastVEP check (both exit 0):

```
vcf_alleles=100000 not_in_harvest=0 no_caid=0 excluded=0
checked=100000 agree=100000 disagree=0 missing=0
vcf_alleles=100000 not_in_harvest=0 no_caid=0 excluded=0
checked=100000 agree=100000 disagree=0 missing=0
```

`e2e_crosscheck.py` reads `ClinGen_CAID` from the CSQ, by the position its
`Format:` header gives it, or the CA ID field of `FV_CLINGEN`. It checks only
the alleles the harvest gives a CA ID, exempts the rows listed in
[`docs/known-differences.md`](docs/known-differences.md) (none), and exits 1
if any CA ID disagrees or is missing. On stderr it lists the first ten
problems and counts the alleles it did not check. It checks the alleles in the
tool's output, so `checked` should equal the input's record count: a record a
tool dropped would lower `checked` (and `vcf_alleles`), not count as missing.
Its tests: `python3 -I scripts/test_e2e_crosscheck.py`.

The HGVS fields (`ClinGen_HGVSg`, and the last `FV_CLINGEN` field) also equal
the harvest's `HGVS_GRCh38` for all 100,000 alleles, checked separately.

### Throughput

Wall clock for the 100,000-record VCF: the median of 10 runs after two
warm-up runs, with a warm page cache, release builds, and process start and
`sandbox-exec` included. Measured on 2026-10-23 with the commits above, on an
Apple M6 (12 cores: 2 Super, 4 Performance, 6 Efficiency), 32 GB, macOS
27.0.1, while other work kept the load average near 5. A second set of 10
runs came within 0.008 s of every median. Neither tool predicts consequences
in these runs (vep-rs has no `--json_cache`, fastVEP runs `--sa-only`), so
the times are VCF parsing, the ClinGen lookups and writing, without the
consequence prediction a full run adds.

| Run | default threads (12) | 1 thread |
|---|---:|---:|
| vep-rs, `--plugin ClinGen` | 0.081 s | 0.129 s |
| vep-rs, no plugin | 0.033 s | 0.077 s |
| fastVEP `--sa-only`, ClinGen index | 0.139 s | 0.170 s |
| fastVEP `--sa-only`, empty `--sa-dir` | 0.063 s | 0.052 s |

Threads are vep-rs's `--fork 0` (auto, 12 here) or `--fork 1`, and fastVEP's
rayon default (12) or `RAYON_NUM_THREADS=1`. ClinGen adds 0.048–0.052 s per
100,000 variants to vep-rs (about 0.5 µs each) and 0.076–0.118 s to fastVEP
(0.8–1.2 µs each). On a one-record VCF every run takes 6–7 ms, so opening the
index and the genome costs next to nothing. With ClinGen, vep-rs annotates
about 1.2 million variants a second here, fastVEP about 0.7 million.

## Harvest step: look up a toy VCF in the live Registry

This is the lookup the harvester makes to build the offline index, run on the
toy VCF against the live Registry, so it needs network access. Requirements:
`bash`, `curl`, `awk` and Python ≥ 3.9 (standard library only).

```bash
examples/lookup_vcf.sh examples/toy.vcf /tmp/toy_out | column -t -s $'\t'
```

Result (2026-10-23):

```
chunk 0: 6 sites in 0s {'registered': 4, 'unregistered': 1, 'error': 1}
ID         CHROM  POS       REF  ALT  CAID                            HGVS_GRCh38
snv        21     5030578   C    T    CA749862939                     NC_000021.9:g.5030578C>T
ins_left   21     13189782  C    CA   CA636937543                     NC_000021.9:g.13189788dup
ins_right  21     13189788  A    AA   CA636937543                     NC_000021.9:g.13189788dup
del_novel  21     33000166  TA   T    -                               NC_000021.9:g.33000172del
melas      MT     3243      A    G    CA120560                        NC_012920.1:m.3243A>G
bad_ref    21     10000000  A    G    ERROR:IncorrectReferenceAllele  -
```

What each row shows:

- **`snv`**: a registered SNV gets its CA ID and canonical HGVS.
- **`ins_left` / `ins_right`**: the same one-base insertion in a run of A's,
  written left-aligned and right-aligned. Both become
  `NC_000021.9:g.13189788dup` (shifted 3′ and written as a duplication, as HGVS
  requires) and both get `CA636937543`.
- **`del_novel`**: a valid deletion the Registry has never registered. The
  canonical HGVS is still computed, but there is no CA ID. Lookups never
  register new alleles.
- **`melas`**: mitochondrial m.3243A>G, the classic MELAS variant. The
  mitochondrial genome uses the `m.` prefix.
- **`bad_ref`**: REF doesn't match GRCh38 (the reference base is `C`). The
  Registry rejects the line, and the offline tools leave it unannotated
  ([toy run](#4-run-the-toy-vcf)).

Full output, including the GRCh37 HGVS column, is in
`/tmp/toy_out/chunks/chunk_00000.tsv`.

## Harvesting a chromosome

```bash
harvest/extract_gnomad_sites.sh 21 data/gnomad41_chr21_sites.tsv     # stream gnomAD, keep CHROM POS REF ALT
python3 -I harvest/query_registry.py data/gnomad41_chr21_sites.tsv data/chr21_chunks
```

`query_registry.py`:

- only calls `POST /alleles` (lookup) and never registers alleles;
- sends 250k sites per request;
- retries with backoff;
- skips chunks already written, so re-running it resumes;
- writes one TSV per chunk with columns
  `CHROM POS REF ALT CAID HGVS_GRCh38 HGVS_GRCh37`.

The Registry is a free public service. Please contact ClinGen before
harvesting beyond gnomAD-scale site lists.

## Enumerating the Registry by region

A site list only finds the alleles you ask about. The Registry also answers
`GET /alleles?refseq=<NC_…>&begin=<b>&end=<e>` with every allele it holds in
a window, and `clingen-index registry enumerate` uses that to fill the index
for gene and exome regions (windows from `clingen-index regions`). Measured
against the live Registry on 2026-10-24
([`docs/registry-region-query.md`](docs/registry-region-query.md)):

- a window returns the alleles lying wholly inside `(begin, end]`, so
  `regions` widens every window by a 200 bp flank;
- each request costs about 0.71 s plus 0.10 ms per allele, so exons are
  merged into windows of up to 50 kb (60,328 requests for protein-coding
  exons ±20 bp, about 31 hours);
- **paging with `limit`/`skip` silently loses alleles**: one 50 kb window in
  APP, fetched in two pages, returned 13 of its 40,636 alleles twice and 13
  others not at all, because the Registry's order shifts between requests.
  `enumerate` never pages: it asks for each window once with a large limit
  and, if the answer is full, splits the window into overlapping halves and
  asks again. Fetched both ways, that window now gives the same 40,636
  alleles;
- **reference alleles are dropped by default**: about a fifth to a third of
  what a window returns is reference-only (`NC_000021.9:g.5030577G=`), which
  no annotation tool looks up. A few are written as a `delins` that puts back
  the reference's own bases (`NC_012920.1:m.3105_3106delinsAC`, 13 of them in
  chrM 3009-5731); `enumerate` asks for each allele's coordinates and drops
  those too, as their allele equals the reference allele.
  `enumerate --keep-reference` writes them all,
  as ordinary `HGVS<TAB>CAID` rows, and the regeneration pipeline's
  `--include_reference` parameter (default `false`) sets that for a whole
  run. The `reference=` count on stderr includes them either way.

## Regenerating the index (Nextflow)

`pipeline/` is a Nextflow workflow that rebuilds the index from current
sources, for one chromosome or all of them, on a workstation, a SLURM cluster
or AWS Batch. It:

1. cuts a region set (by default protein-coding exons ±20 bp, from UCSC's
   RefSeq and GENCODE tables) into query windows and asks the Registry for
   every allele it holds in each
   ([Enumerating the Registry by region](#enumerating-the-registry-by-region));
2. looks up ClinVar's sites (and, with `--datasets`, gnomAD's) in the
   Registry, reusing the registered answers of earlier harvests given as
   `--cache`;
3. builds one index from the enumerated alleles and the registered harvest
   rows (`build --sorted`, in constant memory);
4. checks it before publishing: `validate` recomputes every harvested row's
   HGVS from the genome, and `reach` turns every index key back into a VCF
   record, looks it up as the tools do and requires the key's own CA ID.
   `index/` is published only if both pass, together with `keys/`, the
   sorted key shards it was built from, and `index/metadata.json`, which
   records how it was made and the command that rebuilds it from `keys/`
   byte for byte ([Reproducing an index](#reproducing-an-index)).

Every task runs one `clingen-index` subcommand or a short shell pipeline, in
one container image; there is no Python in the pipeline. Design:
[`docs/superpowers/specs/2026-10-24-index-regeneration-pipeline-design.md`](docs/superpowers/specs/2026-10-24-index-regeneration-pipeline-design.md).

**Requirements.**

- The launcher: Java, Nextflow and nf-test, pinned in
  `pipeline/environment.yml` (openjdk 21.0.10, nextflow 26.04.7, nf-test
  0.9.5, and cfn-lint 1.57.2 for the AWS template). With micromamba (conda
  works the same way):

  ```bash
  micromamba create -y -p ~/micromamba/envs/clingen-nf -f pipeline/environment.yml
  export JAVA_HOME=~/micromamba/envs/clingen-nf/lib/jvm PATH=~/micromamba/envs/clingen-nf/bin:$PATH
  ```

  `micromamba create` does not activate the environment, and Java lives in
  its `lib/jvm`, hence the two variables.
- A container engine for the tasks, running the image the repo's
  `Dockerfile` builds (`docker build -t clingen-index:0.1.0 .`): Docker
  for `-profile local`, Apptainer on a SLURM cluster (`-profile slurm`, the
  image pulled from a registry you push it to), the image in ECR on AWS Batch
  (`-profile awsbatch`). Docker Desktop and Colima share only part of the
  host with containers (Colima: `$HOME`), so the work directory and every
  input must be there.
- Or no container: add the `host` profile last (`-profile local,host`), and
  the tasks run `clingen-index`, `curl`, `gawk`, `gzip`, `openssl` and
  coreutils from `PATH`. For a checkout, `cargo build --release -p
  clingen-index` and put `target/release` on `PATH`.
- Network access from the tasks labelled `network`: the Registry, UCSC (gene
  tables, and `hg38.fa.gz` unless `--genome` is given), NCBI (ClinVar, unless
  `--clinvar_vcf` is a local file) and, for gnomAD, Google Cloud Storage:
  the file listing and the md5 sums always come from there, so with
  `--gnomad_mirror aws` the tasks need AWS Open Data (where the files are
  downloaded from) as well as Google Cloud Storage.

**Running it.** The spec's three examples, as the pipeline takes them:

```bash
# one chromosome on this machine (the run measured below)
nextflow run pipeline -profile local --chrom 21 --regions pc_exons \
  --cache data/chr21_chunks,data/clinvar/20261019/chunks \
  --genome "$PWD/data/genome_grch38/GRCh38.bin" \
  --clinvar_vcf "$PWD/data/clinvar/download/clinvar.vcf.gz" --outdir results/chr21

# protein-coding exons everywhere, on a SLURM cluster
nextflow run pipeline -profile slurm --chrom all --slurm_queue normal --network_queue transfer \
  --container ghcr.io/<you>/clingen-index:0.1.0 --outdir /scratch/clingen/pc_exons -resume

# the same on AWS Batch
nextflow run pipeline -profile awsbatch --chrom all --aws_queue clingen \
  --container <account>.dkr.ecr.us-east-1.amazonaws.com/clingen-index:0.1.0 \
  -work-dir s3://<bucket>/work --outdir s3://<bucket>/results/pc_exons -resume
```

They differ from the spec's examples where the pipeline or this checkout
needs it. The first spells out the default `--regions pc_exons` and adds
`--genome` and `--clinvar_vcf`: the whole-genome binary and the ClinVar
2026-10-19 VCF of [ClinVar coverage](#clinvar-coverage) are already here,
so the run reuses them rather than building hg38 and downloading the latest
ClinVar, and its ClinVar sites are those the `--cache` answered. The AWS one drops
`--gnomad_mirror aws`, which matters only with a gnomAD dataset in
`--datasets`, and names the queue `clingen`, the template's default. On
SLURM, launch from a directory on the shared filesystem, or give `-work-dir`
there: `./work` must be visible to every node. Every task there gets the
partition's default time limit except `GNOMAD_SITES`, which asks for 24
hours (`time` in `pipeline/conf/slurm.config`), as it streams a multi-GB
gnomAD VCF per chromosome. [`docs/aws-batch.md`](docs/aws-batch.md) sets up
AWS Batch (bucket, queue, ECR repository, launcher permissions) and covers
costs and teardown; nothing there has been deployed.

**`-resume` continues one regeneration; it does not start a new one.** In
the two long runs it carries on after a stop: Nextflow reuses every task
that finished and runs only those that did not. It reuses the downloads too:
`--clinvar_vcf latest`, gnomAD's `latest` and the UCSC tables have no input
that changes when the remote file does, and `ENUMERATE` asks about the same
windows, so a `-resume` next month from the same launch directory would
hand back this month's index, its manifest still dated this month. For a new
regeneration (a newer ClinVar, gnomAD or UCSC release, alleles registered
since), launch from a fresh directory, or leave out `-resume`. A new
`-work-dir` alone is not enough: `-resume` finds the earlier tasks through
`.nextflow/` in the launch directory, wherever their work files are (a stub
run from the same directory with a new `-work-dir` and `-resume` reported
`cached=15`).

**Local resources.** `-profile local` caps every task at 2 CPUs and 12 GB
(`process.resourceLimits` in `pipeline/conf/local.config`): Nextflow's local
executor refuses a task that asks for more than the machine has, and the
sorting steps ask for 4 CPUs and 16 GB. The chr21 run's largest task peaked
at 1.9 GB, so a 16 GB, 2-CPU machine runs it. On a larger machine, raise the
caps with a config file given with `-c`:

```bash
echo 'process.resourceLimits = [cpus: 8, memory: 48.GB]' > bigger.config
nextflow run pipeline -profile local -c bigger.config --chrom 21 --outdir results/chr21
```

The main parameters (`nextflow run pipeline --help` lists them, `--helpFull`
with details and `--showHidden` with the hidden ones too, such as
`--page_size` and the endpoints; a misspelt one stops the run):

| Parameter | Default | Meaning |
|---|---|---|
| `--chrom` | required | `1`–`22`, `X`, `Y`, `MT` (or `chr21`, `chrM`), or `all` |
| `--regions` | `pc_exons` | `cds`, `pc_exons`, `exons`, `genes`, or a BED file |
| `--datasets` | `clinvar` | comma-separated: `clinvar`, `gnomad_genomes`, `gnomad_exomes` |
| `--cache` | none | comma-separated harvest directories (`chunk_*.tsv`, or an earlier run's `harvest/`); their registered answers are not asked again |
| `--genome` | built | a genome binary with its `.idx`, as a local or shared path (an `s3://` one, or any with an `s3://` work directory, is refused at launch); without it `BUILD_GENOME` builds one from UCSC `hg38.fa.gz` for the run's chromosomes |
| `--clinvar_vcf` | `latest` | or a local `clinvar.vcf.gz`; NCBI's `.md5` beside it is checked when it is there |
| `--include_reference` | `false` | also index the Registry's reference alleles |
| `--allow_mismatches` | `false` | publish the index even if a check fails |
| `--outdir` | `results` | a local path or `s3://…` |

**Outputs** in `--outdir`:

- `index/`: `caid.fst`, `manifest.json` and `metadata.json`, published only
  if every check passed. `metadata.json` records how the index was made and
  how to rebuild it ([Reproducing an index](#reproducing-an-index)). The
  manifest's sources name the region set and the day the
  Registry was asked (`registry_pc_exons_<date>`, the UTC day of the earliest
  `ENUMERATE` task) and each dataset's release (`clinvar_<date>`,
  `gnomad_genomes_v<version>`); its `harvest_date` is the UTC day of the
  earliest `ENUMERATE` or `LOOKUP` task. A task that `-resume` reuses keeps
  the day it ran, so a resumed run's manifest is dated by the answers it
  holds;
- `keys/<accession>.tsv.gz`: one per chromosome, the byte-sorted
  `HGVS<TAB>CAID` pairs `build --sorted` read, compressed with `gzip -n`;
  published with `index/` or not at all;
- `genome/`: `GRCh38.bin` and its `.idx`, when the pipeline built them. The
  manifest then records the genome as `../genome/GRCh38.bin`, so keep the two
  directories side by side; a `--genome` you gave is recorded by its path and
  not copied;
- `harvest/<chrom>.tsv.gz`: the seven harvest columns for every site looked
  up or reused, the next run's `--cache`;
- `reports/`: `summary.md` (the parameters, the gate's result, one row per
  chromosome, the skipped alleles, the limits), `coverage.tsv` (every count
  in long format: `chrom source measure value`), `sources.tsv` (each download,
  its size and md5, and whether the md5 was checked), `skipped.tsv.gz` (each
  allele not looked up, and why), `validate_<chrom>.txt` and
  `reach_<chrom>.txt`;
- `pipeline_info/`: Nextflow's `trace.txt`, `timeline.html` and `report.html`.

`FINALIZE`, which publishes `index/`, `keys/` and `genome/`, waits for
`REPORT`, so the reports are there even when the gate stops a run.

### Reproducing an index

Rerunning the pipeline regenerates an index; it does not reproduce one. The
Registry gains alleles over time and cannot be asked what it held on an
earlier day, and `latest` ClinVar, gnomAD and UCSC files change, so the same
command next month gives a different index. What reproduces an index is a
rebuild from the shards it was built from, which every published run keeps
beside it:

- `keys/<accession>.tsv.gz`: each chromosome's `KEYS` output, the exact input
  of `clingen-index build --sorted`, compressed with `gzip -n` (no file name
  or time in the header, so the same lines give the same bytes with the same
  `gzip`; another `gzip` may give other bytes for the same lines, which is why
  the md5 of the unpacked file is recorded too).
- `index/metadata.json` (`"format_version": 1`), written by `FINALIZE` after
  the gate and published with `index/` and `keys/`, or not at all:
  - `pipeline`: the commit of the checkout holding `pipeline/` and whether
    its working tree was dirty (both `null` for a copy without `.git`), a
    `source_digest` that identifies the pipeline's files without `.git`,
    Nextflow's version, the command line and config files, profile, run
    name, session, launch, project and work directories, start time, the
    time the document was written, and the container and engine as
    configured;
  - `tool`: what `DIGEST` finds inside the task image: `clingen-index
    --version`, the sha256 and path of that binary and the platform (`uname
    -sm`); `build_source`, a digest of the image's build inputs in the
    launcher's checkout; and the manifest's vareffect version;
  - `params`: every parameter as resolved;
  - `regions`: the region set (or the BED file's name and md5), pad, merge
    gap, max window, flank, chromosomes, and windows, core and query bp from
    `REGIONS`' summary, per chromosome and in total;
  - `registry`: the base URL, the registry and harvest dates as the manifest
    has them, page size, split overlap, `include_reference`;
  - `sources`: every row of `reports/sources.tsv`, the ClinVar release, the
    gnomAD files with their md5s, and the genome: for a built one the
    `hg38.fa.gz` URL and md5 and the md5 and size of `GRCh38.bin` and its
    `.idx`, for a `--genome` given its path and the same md5s;
  - `cache`: each `--cache` directory with every file `SPLIT_CACHE` reads
    (name, size, md5) and one digest over them all, which counts names and
    md5s only, so a copy of the directory elsewhere gives the same digest.
    Answers taken from a cache were fetched before the run; the manifest's
    `harvest_date` covers only the answers the run fetched;
  - `outputs`: md5 and size of `caid.fst` and `manifest.json`, the entry
    count, and for each shard its md5 packed and unpacked, size and line
    count;
  - `checks`: the gate's result (a run published with `--allow_mismatches`
    says which checks failed), every check's status and each chromosome's
    counts from `coverage.tsv` and the `KEYS` stats;
  - `rebuild`: the `build` command that gives `caid.fst` and `manifest.json`
    again from the gunzipped shards, as an `argv` array and a shell string,
    with the steps around it and the expected md5 of both files;
  - `regenerate`: the command line, and the note that running it again
    regenerates.

`source_digest` is the sha256 of what `sha256sum` prints for the pipeline's
files, which can be computed for any copy of `pipeline/`:

```bash
cd pipeline && sha256sum main.nf modules/*.nf nextflow.config conf/*.config nextflow_schema.json \
  | LC_ALL=C sort -k2 | sha256sum
```

**The rebuild.** `manifest.json` records the sources in the order given, the
harvest date and the genome, and holds no time or host, so the rebuild's
command repeats `BUILD_INDEX`'s: the same `--source` labels in the same
order, the same `--harvest-date`, and the same genome argument. A built
genome is recorded as `../genome/GRCh38.bin`, relative to `index/`; `build`
resolves the genome's directory before it makes the path relative, so the
rebuild directory needs a real `genome/` directory beside `index/` holding
the two files or links to them (a `genome/` that is itself a link would be
recorded by its target's path). A `--genome` given is recorded by its
absolute path, which must exist when the rebuild runs: `build` refuses a
missing one, though it does not read it, so any file there gives the same
bytes. With the genome recorded the same way, `manifest.json` comes back
byte-identical too; with another genome path only its `genome` line
differs, and `caid.fst` is the same. The steps make `<outdir>` absolute
before they change directory, so it may be given relative to where they
start.

**The binary.** `tool.clingen_index_version` does not identify the
`clingen-index` that built an index: it is the crate's version, `0.1.0`,
which has stayed the same through every commit to `crates/` so far
(2026-10-25). `tool.clingen_index_sha256` does, for the platform
`tool.platform` names. To get that binary again, build the image from the checkout of
`pipeline.git_commit` (or from a copy whose `tool.build_source.digest`
matches; `build_source` is computed like `source_digest`, over the
`Dockerfile`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` and every
file under `crates/`) on the same platform, and compare the sha256 of its
`/usr/local/bin/clingen-index`:

```bash
find Cargo.toml Cargo.lock rust-toolchain.toml Dockerfile crates -type f | LC_ALL=C sort \
  | xargs sha256sum | sha256sum          # tool.build_source.digest
docker build -t clingen-index:0.1.0 .
docker run --rm clingen-index:0.1.0 sha256sum /usr/local/bin/clingen-index
```

`build_source` describes the launcher's files, which are the image's only
if the image was built from them; the binary's sha256 is taken inside the
image the run used. On 2026-10-25 a `docker build --no-cache --target build` of the
`crates/`, `Cargo.lock` and `Dockerfile` unchanged since `aa3078b` gave a
binary with the sha256 of the one in the image built from them earlier that
day (`b2acd6afd421…`, linux/arm64), so the image build repeats. A binary with
another sha256, such as a Mac build, may still write the same bytes (it did
for chr21, below); the md5 comparison is what decides.

`pipeline/tests/check_rebuild.sh <outdir>` does all of it: in a new
temporary directory it gunzips `keys/` and checks each shard's md5s and line
count against `metadata.json`, links a built genome into `genome/`, runs the
`rebuild` command, and compares the md5 of the new `caid.fst` and
`manifest.json` with `metadata.json`'s expected ones and with
`<outdir>/index/`. It names every difference and exits 1 if there is one. It
needs `jq`, takes `clingen-index` from `PATH` (set `CLINGEN_INDEX` to
another command, such as `apptainer exec clingen-index.sif clingen-index`),
and, with `openssl`, says whether that binary's sha256 is
`tool.clingen_index_sha256`. For an `s3://` outdir, copy `index/`, `keys/`
and `genome/` down first. When a `--genome` was given and its recorded path
cannot be made on this machine, `GENOME_STANDIN=<any file>` rebuilds with
that file in its place: `caid.fst` must still match byte for byte, and
`manifest.json` once its `genome` value is put back to the recorded one; the
last line says which checks were made.

### The chr21 run (2026-10-24)

The first example above, run on 2026-10-24 at clingen `13824dd` with the
image `clingen-index:0.1.0` (`123dd454650c`, built from `fe4adae`, whose
`crates/`, `Cargo.lock` and `Dockerfile` are those of `13824dd`; `crates/`
has changed since, so build the image from the branch head, which gives it
another ID), on an Apple M6 (12 cores), 32 GB, macOS 27.0.1, with Docker
29.5.2 in a Colima VM of 6 CPUs and 12 GB. The ClinVar VCF is the 2026-10-19 one of
[ClinVar coverage](#clinvar-coverage), its md5
`e649bbb1dfe11baafa9feab81a7a7cdd` checked against NCBI's `.md5`. The run
took 13 min 28 s (`/usr/bin/time`: `real 807.96`) and ended with:

```
[SUCCESS] completed=15 failed=0 cached=0
```

`reports/summary.md` gives the gate's result and one row per chromosome:

```
Gate: passed. Every validate row matched, every reach key that names a variant was found
again (reference and unparseable keys are only counted), and every chromosome with windows
enumerated alleles; FINALIZE publishes index/.
```

| chrom | windows | enumerated | harvest sites | registered | keys | validate | reach |
|---|---:|---:|---:|---:|---:|---|---|
| 21 | 669 | 5215913 | 51713 | 51692 | 5212187 | 51713 of 51713 rows match | 5212183 of 5212183 keys reachable, 4 unparseable |

`reports/coverage.tsv`:

```
chrom	source	measure	value
21	registry	windows	669
21	registry	core_bp	4573512
21	registry	query_bp	4841112
21	registry	alleles	5215913
21	clinvar	sites	51713
21	clinvar	skipped	25
21	harvest	sites	51713
21	harvest	cached	51692
21	harvest	looked_up	21
21	harvest	registered	51692
21	index	keys	5212187
```

The two `ENUMERATE` tasks (500 and 169 windows) printed:

```
windows=500 requests=500 splits=0 alleles=4494691 reference=1302315 no_grch38_hgvs=0 written=3192376
windows=169 requests=169 splits=0 alleles=2792299 reference=768762 no_grch38_hgvs=0 written=2023537
```

That is 669 requests, no answer full enough to split a window, 7,286,990
alleles returned, 2,071,077 of them reference alleles left out (28%), and
5,215,913 pairs written. Counted from the work files, 3,874 of those pairs
are written twice, each time by two adjacent windows of one batch, and
`KEYS` removes the second copies, leaving 5,212,039 distinct enumerated
pairs. Of the 3,874, 3,869 lie wholly in the 400 bp where the two windows'
query spans overlap (core ± 200 bp flank). The other five end in that
overlap but start before the second window's span, which returned them all
the same: the deletions `NC_000021.9:g.44310291_44312789del`
(`CA3268150738`) and `g.44311490_44312789del` (`CA3268150740`), and three
indels at 46,254,003-46,254,006 (`CA2392578422`, `CA749836761`,
`CA920321252`). Of the 51,692 registered ClinVar chr21 alleles, 51,544 were
enumerated too; the other 148 lie outside the windows or cross their edges
and are in the index from the lookup alone. The cache answered 51,692 of the 51,713 ClinVar sites; the 21
it had no CA ID for were asked again, and are still unregistered
(`LOOKUP`: `sites=21 requests=1 registered=0 unregistered=21 error=0 written=21`).

`reports/validate_21.txt`:

```
rows=51713 compared=51713 skipped=0 malformed=0
class	total	match	pct
substitution	47989	47989	100.0
del	2261	2261	100.0
dup	984	984	100.0
ins	240	240	100.0
delins	239	239	100.0
m	0	0	0.0
other	0	0	0.0
```

`reports/reach_21.txt`:

```
keys	5212187
reachable	5212183
wrong_ca	0
not_found	0
ref_mismatch	0
unsupported	0
reference	0
unparseable	4
class	keys	reachable
substitution	4544278	4544274
del	340083	340083
dup	119019	119019
ins	169929	169929
delins	38878	38878
m	0	0
other	0	0

examples:
unparseable: NC_000021.9:g.[33579447T>G;33583699T>C;33588560T>G] CA645373284 (position is not a positive integer)
unparseable: NC_000021.9:g.[43068519C>G;43068592G>C] CA2742038401 (position is not a positive integer)
unparseable: NC_000021.9:g.[44889397G>A;44900489G>T] CA2499306185 (position is not a positive integer)
unparseable: NC_000021.9:g.[46131981G>A;46132019C>T] CA042778 (position is not a positive integer)
```

The four unparseable keys are multi-variant alleles (`g.[…;…]`), which the
region query returns like any other allele and no VCF record names, so
`reach` counts them and the gate ignores them. Every other key, 38,878
`delins` among them, came back with its own CA ID from a VCF record written
from its HGVS.

`index/manifest.json` (`caid.fst` is 54,440,992 bytes, about 10.4 bytes per
entry):

```json
{
  "format_version": 1,
  "assembly": "GRCh38",
  "sources": [
    "registry_pc_exons_2026-10-24",
    "clinvar_2026-10-19"
  ],
  "harvest_date": "2026-10-24",
  "entries": 5212187,
  "vareffect_version": "0.6.0",
  "genome": "/Users/leipzig/Documents/dev/clingen/data/genome_grch38/GRCh38.bin"
}
```

Task times from the first run's `pipeline_info/trace.txt`, kept as
`data/e2e_pipeline/run1/pipeline_info/trace.txt` (the one in
`results/chr21/pipeline_info/` is the deletion rerun's, below); every other
task took under a second:

| Task | Duration | Peak RSS |
|---|---:|---:|
| `ENUMERATE (windows_21_0000)` | 8m 24s | 309.8 MB |
| `ENUMERATE (windows_21_0001)` | 4m 46s | 439.5 MB |
| `MERGE_SITES (21)` | 41.3s | 1.9 GB |
| `SPLIT_CACHE` (the 63 cache files) | 25.4s | 6.9 MB |
| `CLINVAR_SITES` | 6.2s | 12.6 MB |
| `FETCH_UCSC` | 6.1s | 14.6 MB |
| `REACH (21)` | 3.2s | 91.3 MB |
| `KEYS (21)` | 2.9s | 768.4 MB |
| `BUILD_INDEX` | 1.5s | 15.8 MB |

Enumeration was 13 min 10 s of the run: 1.18 s per request, about 9,200
alleles a second, faster than the cost measured before (0.71 s plus 0.10 ms
per allele) predicts, which is about 20 minutes for these windows.

**Resume.** The same command with `-resume` ran nothing: all 15 tasks are
`CACHED` in `trace.txt`, and it took 2.78 s.

```
[SUCCESS] completed=0 failed=0 cached=15
```

Then the second `ENUMERATE` task's output (`enum_21_0001.tsv`) was deleted
from its work directory and the command run with `-resume` again. Of the
Registry tasks, only that one ran again (169 requests, 4m 47s), in a new
work directory, as Nextflow does for a cached task whose output is gone.
The five tasks downstream of it (`KEYS`, `BUILD_INDEX`, `REACH`,
`REPORT`, `FINALIZE`) ran again after it; the other nine, the first
`ENUMERATE`, `LOOKUP` and `VALIDATE` among them, stayed cached:

```
[SUCCESS] completed=6 failed=0 cached=9
```

The new `enum_21_0001.tsv` is byte-identical to the deleted one (md5
`dc1ceb4c667ac09527ca5f97d4d9b25e`), and so are the rebuilt `caid.fst` (md5
`fee1e8364418752fb8f8fc2f2f1384ed`), `manifest.json` and every report. All
three runs started on 2026-10-24, so the manifest kept its date. That code
dated the manifest by the day the run started, in the launcher's time zone
(MDT here); the pipeline now dates it by the UTC day of its Registry tasks,
which a `-resume` keeps for the tasks it reuses. These ran from 22:31 MDT on
2026-10-24, already 2026-10-25 in UTC, so the same runs would now record
`registry_pc_exons_2026-10-25` and a `harvest_date` of 2026-10-25.

**Metadata and rebuild (2026-10-25).** The pipeline has since gained
`keys/` and `index/metadata.json` ([Reproducing an index](#reproducing-an-index)).
A `-resume` of this run (session `1f25d9d8`) cannot reuse its Registry
tasks: `ENUMERATE` and `LOOKUP` have changed since (each records the UTC day
it ran, from `47cd3b7`), so their cached tasks no longer match and Nextflow
would ask the Registry again. To check the new outputs on real data without a
Registry request, the first example above was resumed with `-resume
1f25d9d8-644f-488d-8c73-a86f8bec05e6 --outdir results/chr21-meta` and
`--registry_url` pointing at a stand-in on the launcher (an ad-hoc Python
server, not in the repository) that replays this run's own answers: the
5,212,039 distinct pairs of its two `enum_21_*.tsv` work files, each returned
for every window its span overlaps, and its 21 `LOOKUP` rows. Five tasks
were cached (`SPLIT_CACHE`, `FETCH_UCSC`, `CLINVAR_SITES`, `REGIONS`,
`MERGE_SITES`) and twelve ran, in 2 min 19 s:

```
[SUCCESS] completed=12 failed=0 cached=5
```

The stand-in answered 669 region requests and one lookup. The index came
back the same: `caid.fst` md5 `fee1e8364418752fb8f8fc2f2f1384ed`, 5,212,187
keys, every check passed. `manifest.json` differs only in its dates
(`registry_pc_exons_2026-10-26` and a `harvest_date` of 2026-10-26, the UTC
day the replayed tasks ran), and `registry:alleles` counts 5,215,954 pairs
written rather than 5,215,913: the stand-in returns 41 more alleles from two
adjacent windows each, and `KEYS` removes the second copies. A first try,
whose stand-in returned an allele only to the windows its start lies in,
gave 1,065 keys fewer: the live Registry also returned alleles that start
before a window and end inside it (`NC_000021.9:g.10511963_10521699del`,
from the window starting at 10,521,332).

After `tool` gained the binary's sha256 and `build_source`, the same
command (session `1f25d9d8`, the stand-in's `--registry_url` again, with
nothing listening there) was resumed twice more on 2026-10-25, first from
the uncommitted change (only `DIGEST` and `FINALIZE` ran: `completed=2
failed=0 cached=15`, 12 s), then at `f997c55` with a clean tree (only
`FINALIZE` ran: `completed=1 failed=0 cached=16`, 4 s). Every `ENUMERATE`
and `LOOKUP` task came back cached both times. `caid.fst` is still
`fee1e8364418752fb8f8fc2f2f1384ed`, and `metadata.json` records the image's
binary as sha256 `b2acd6afd421…` on `Linux aarch64`, `git_commit`
`f997c55` and `git_dirty` false.

`results/chr21-meta/index/metadata.json` is 35,950 bytes (27,350 before
`tool` gained `build_source`'s file list), and
`keys/NC_000021.9.tsv.gz` 34,530,254 bytes (209,452,251 unpacked, 5,212,187
lines). `bash pipeline/tests/check_rebuild.sh results/chr21-meta`, with
`clingen-index` built on the Mac from the same commit, rebuilt `caid.fst`
and `manifest.json` byte for byte, twice, in 2.4 s each: the image's Linux
build and the Mac build write the same bytes. The image's `gzip -n` (Debian)
compressed the unpacked shard to the same bytes twice (md5
`3ee066a65af1945eb95383435183cd71`) and Apple's `gzip` 487.0.1 to other
bytes, which is why `metadata.json` records the unpacked md5 as well.

**End to end.** The 51,738 chr21 records of the ClinVar 2026-10-19 VCF,
annotated offline by both tools with this index (vep-rs `b03dd5f`, fastVEP
`cb0a33c`, both rebuilt against this checkout), then checked against the
run's own harvest. `offline` is the `sandbox-exec` wrapper of the
[toy run](#4-run-the-toy-vcf):

```bash
e=data/e2e_pipeline
mkdir -p $e/harvest $e/sa
{
  printf '##fileformat=VCFv4.2\n##contig=<ID=21,length=46709983,assembly=GRCh38>\n'
  printf '#CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\n'
  gunzip -c data/clinvar/download/clinvar.vcf.gz |
    awk -F'\t' -v OFS='\t' '!/^#/ && $1 == "21" {print $1, $2, $3, $4, $5, ".", ".", "."}'
} > $e/clinvar_chr21.vcf
gunzip -c results/chr21/harvest/21.tsv.gz > $e/harvest/chunk_00000.tsv
ln -sfn ../../../results/chr21/index $e/sa/clingen_chr21

offline ../vep-rs/target/release/vep -i $e/clinvar_chr21.vcf -o $e/vep-rs.vcf \
    --vcf --offline --assembly GRCh38 --force_overwrite --no_stats --quiet \
    --plugin ClinGen,index=results/chr21/index
offline ../fastVEP/target/release/fastvep annotate -i $e/clinvar_chr21.vcf \
    -o $e/fastvep.vcf --sa-dir $e/sa --sa-only --no-progress

python3 -I scripts/e2e_crosscheck.py --harvest $e/harvest --vcf-out $e/vep-rs.vcf --tool vep-rs
python3 -I scripts/e2e_crosscheck.py --harvest $e/harvest --vcf-out $e/fastvep.vcf --tool fastvep
```

fastVEP's stderr, then vep-rs's tally from a second run without `--quiet`
and with `NO_COLOR=1` (vep-rs logs in UTC; it was 22:50 on 2026-10-24
locally):

```
Annotating: data/e2e_pipeline/clinvar_chr21.vcf -> data/e2e_pipeline/fastvep.vcf
Loaded 1 supplementary annotation source(s) from data/e2e_pipeline/sa in 0.0s
ClinGen: found=51692 not_in_index=21 ref_mismatch=0 unsupported=25
2026-10-25T04:50:49.942319Z  INFO vep_builtins::plugins::clingen: ClinGen: found=51692 not_in_index=21 ref_mismatch=0 unsupported=2
```

The vep-rs check, then the fastVEP check (both exit 0):

```
vcf_alleles=51715 not_in_harvest=2 no_caid=21 excluded=0
checked=51692 agree=51692 disagree=0 missing=0
vcf_alleles=51738 not_in_harvest=25 no_caid=21 excluded=0
checked=51692 agree=51692 disagree=0 missing=0
```

Both tools give the Registry's CA ID for every one of the 51,692 registered
ClinVar chr21 alleles. The 21 `not_in_index` are the sites still
unregistered, and the `unsupported` ones are the 25 skipped records; vep-rs
drops the 23 with `ALT=.` before any plugin sees them, as in
[ClinVar coverage](#clinvar-coverage).

### SNVs in the exome, by source

The SNVs the index gets in the exome from ClinVar, from gnomAD exomes and
from both, counted on 2026-10-25 from the files of
[the chr21 run](#the-chr21-run-2026-10-24), which used `--datasets clinvar`,
and from gnomAD v4.1's chr21 exome VCF, downloaded for this count. The exome
here is the default region set, `--regions pc_exons` with `--pad 20`: the
whole exons, UTRs included, of every transcript with a coding range in
UCSC's `ncbiRefSeq` and `knownGene` tables (the two files the run used, md5s
in its `sources.tsv`), each padded by 20 bp on both sides, taken as a union.
UTR exons make up about two-thirds of it (the padded coding parts alone are
49.5 Mb genome-wide), and it takes in predicted RefSeq models (`XM_`) and
every GENCODE coding isoform, so it is several times a capture kit's exome.
On chr21 it is 2,727 intervals and 1,588,416 bp, or 4,765,248 possible SNVs
at three per base; on the 25 primary contigs, 254,888 intervals and
148,150,221 bp, or 444,450,663 possible SNVs (59 of those bases are N in
`GRCh38.bin`, so 444,450,486 real substitutions; chr21's exome has no N).
The windows are larger again (4,573,512 bp of cores on chr21, 406.7 Mb
genome-wide), as `--merge_gap` joins padded exons no more than 5 kb apart
and takes in the intron between them. An SNV is one base (A, C, G or T)
replaced by another, counted once per position and allele. It is registered
if it has a CA ID, and only registered SNVs can be index keys.

The table counts chr21: ClinVar from its 2026-10-19 VCF, gnomAD v4.1 exomes
from `data/gnomad41_exomes_chr21/gnomad.exomes.v4.1.sites.chr21.vcf.bgz`
(2,250,319,809 bytes, md5 `7ca6d51a42425b857eddb46d7bc5832d`, the
`fKbVGkJCW4V+3bRte8WDLQ==` that Google Cloud Storage lists for it; every
record, whatever its `FILTER`, as `GNOMAD_SITES` keeps them) and the
Registry from the run's enumeration of 2026-10-24. The last column is the
registered SNVs as a share of the Registry's:

| chr21, in the exome | SNVs | registered | of the Registry's |
|---|---:|---:|---:|
| ClinVar 2026-10-19 alone (`--datasets clinvar`) | 46,086 | 46,083 | 2.0% |
| gnomAD v4.1 exomes alone (`--datasets gnomad_exomes`) | 772,265 | 772,265 | 34.0% |
| ClinVar and gnomAD v4.1 exomes (`--datasets clinvar,gnomad_exomes`) | 784,469 | 784,466 | 34.6% |
| Registry enumeration, 2026-10-24 (any `--datasets`) | 2,268,617 | 2,268,617 | 100% |

Every registered dataset SNV in the exome is in the enumeration, so in the
exome the index holds the same 2,268,617 SNVs whatever `--datasets` says:
the enumeration together with every registered SNV of ClinVar, gnomAD
exomes and gnomAD genomes is still 2,268,617, 47.6% of the 4,765,248
possible. The datasets matter outside the windows, where they are the
index's only source (the run's index has 4,544,274 SNV keys: the 4,544,144
enumerated, about half of them in introns and flanks, and 130 registered
ClinVar SNVs outside the windows), and their harvested rows are what
`validate` checks. Of the 1,995,826 gnomAD exome SNVs on chr21, 1,223,561
(61%) lie outside the exome: 652,769 in the windows' introns and flanks, all
of them enumerated, and 570,792 beyond the windows, most of them not PASS
and many in exons of non-coding transcripts. No lookup of gnomAD exomes has
been made, so whether those 570,792 are registered is not known, beyond the
110,658 of them that gnomAD genomes or ClinVar also has, which those
harvests found registered. Of the 772,265 in the exome, 348,035 have
`FILTER=PASS`, all registered (15.3% of the Registry's). gnomAD v4.1
genomes, which `--datasets gnomad_genomes` would bring, have 336,467 SNVs in
the chr21 exome, all registered (14.8%).

Enumeration membership agrees with the Registry's lookups wherever both
answered: the run's ClinVar harvest calls 46,083 of its 46,086 exome SNVs
registered, with the enumeration's CA IDs, and the other 3
(`g.43076233C>T`, `g.45493228A>G`, `g.46354074T>G`) unregistered and absent
from the enumeration; and the gnomAD v4.1 genomes harvest
(`data/chr21_chunks`) calls all 336,467 registered, with the same CA IDs.
No lookup covers gnomAD exomes: their registered column is enumeration
membership alone, and all 1,425,034 gnomAD exome SNVs in the windows are
enumerated.

ClinVar is the one source counted genome-wide as well, so its SNVs split
exactly into exonic and the rest. The exons alone, without the 20 bp pads,
are 2,754 intervals and 1,478,666 bp on chr21 and 258,074 intervals and
137,893,448 bp on the 25 primary contigs; the registered counts are the CA
IDs of [ClinVar coverage](#clinvar-coverage): its 2026-10-24 lookups, and
the chr21 gnomAD harvest of 2026-10-23 for the 24,244 chr21 ones it already
had (22,660 of them in the exome).

| ClinVar 2026-10-19 SNVs | chr21 | registered | genome-wide | registered |
|---|---:|---:|---:|---:|
| in exons | 40,362 | 40,361 | 3,658,015 | 3,657,907 |
| in the 20 bp pads beside them | 5,724 | 5,722 | 461,060 | 460,765 |
| in the exome (the two above) | 46,086 | 46,083 | 4,119,075 | 4,118,672 |
| outside the exome: introns, non-coding genes, intergenic | 1,903 | 1,903 | 150,257 | 150,204 |
| all, genome-wide or chromosome-wide | 47,989 | 47,986 | 4,269,332 | 4,268,876 |

So 85.7% of ClinVar's SNVs are exonic and 96.5% lie in the exome. With
`--datasets clinvar` the index holds every registered ClinVar SNV, in the
exome or not: the run's chr21 index holds all 47,986, the 46,083 in the
exome and the 1,773 outside it but inside the windows through the
enumeration, and the 130 beyond the windows through the harvest. Genome-wide
that is 4,268,876. The Registry and gnomAD exomes were counted on chr21
only. Scaled by exome size (148,150,221 / 1,588,416 bp,
93.27 times), the whole exome would hold about 211.6 million Registry SNVs,
about 72.0 million gnomAD v4.1 exome SNVs, all registered if chr21 is
typical, and about 73.2 million from ClinVar and gnomAD exomes together. The same scaling of ClinVar's 46,086 chr21 SNVs gives 4,298,402,
4.4% above the exact 4,119,075, which shows how far chr21 can be from the
genome as a whole. Counting the Registry's exome SNVs genome-wide needs the
[full exome run](#the-full-exome-run-not-run)'s enumeration; gnomAD's need
gnomAD v4.1's exome VCF for every chromosome.

### The full exome run (not run)

`--chrom all --regions pc_exons` is the run the spec plans: 60,328 windows
over 406.7 Mb of protein-coding exons ±20 bp (430.8 Mb with the flanks), in
132 batches of up to 500, so at least 60,328 region requests, one after
another on the single `ENUMERATE` stream. It has not been run. The spec
estimates about 31 hours from the cost per request; at chr21's measured
1.18 s per request it would be nearer 20 hours, though chr21 need not be
typical. Scaled from chr21 by window length, the index would hold about
460 million keys in a `caid.fst` of about 4.8 GB (the spec estimated
550 million and 6.5 GB). `keys/` would be about 3.1 GB, an estimate scaled
the same way (by 406.7 Mb of windows against chr21's 4.57 Mb, 88.9 times)
from chr21's measured shard of 34,530,254 bytes
([Metadata and rebuild](#the-chr21-run-2026-10-24)); unpacked for a rebuild
it would be about 18.6 GB. A local run (`-profile local --chrom all`) is
untested and needs more memory and disk than chr21: raise the local caps
with `-c` ([Local resources](#regenerating-the-index-nextflow)), give Docker
(or the Colima VM) the memory, and expect the work directory to hold tens of
GB. `-resume` continues a stopped run by re-running only the tasks that did
not finish, and the launcher has to stay up for the whole run.

The commands below are for that run with ClinVar only (`--datasets
clinvar`), reusing the ClinVar lookups of 2026-10-24
(`data/clinvar/20261019/chunks`, 19 chunk files, 366 MB) as `--cache`, so
only the sites they did not answer are looked up again. Without `--genome`
the run builds the genome from UCSC `hg38.fa.gz` and publishes it in
`genome/`. Replace the values in angle brackets.

On SLURM, copy the cache to the shared filesystem, then launch from a
directory there (`./work` must be visible to every node):

```bash
rsync -a data/clinvar/20261019/chunks/ <shared>/clingen/cache/clinvar-20261019/
mkdir -p <shared>/clingen/pc_exons && cd <shared>/clingen/pc_exons
nextflow run <checkout>/pipeline -profile slurm --chrom all --regions pc_exons --datasets clinvar \
  --cache <shared>/clingen/cache/clinvar-20261019 \
  --slurm_queue <partition> --network_queue <partition with internet> \
  --container ghcr.io/<you>/clingen-index:0.1.0 \
  --outdir <shared>/clingen/results/pc_exons -resume
```

On AWS Batch, with the stack, image and launcher of
[`docs/aws-batch.md`](docs/aws-batch.md) and its `BUCKET`, `QUEUE` and `REPO`
variables, upload the cache to the stack's bucket, then launch:

```bash
aws s3 sync data/clinvar/20261019/chunks/ "s3://$BUCKET/cache/clinvar-20261019/"
nextflow run pipeline -profile awsbatch --chrom all --regions pc_exons --datasets clinvar \
  --cache "s3://$BUCKET/cache/clinvar-20261019" \
  --aws_queue "$QUEUE" --container "$REPO:0.1.0" \
  -work-dir "s3://$BUCKET/work" --outdir "s3://$BUCKET/results/pc_exons" -resume
```

Either way the run makes at least 60,328 region requests on the one
`ENUMERATE` stream (about 20 to 31 hours, as above) plus the lookups of the
ClinVar sites the cache does not answer, and publishes in `--outdir`:
`index/` (`caid.fst`, about 4.8 GB, `manifest.json` and `metadata.json`),
`keys/` (25 shards, about 3.1 GB), `genome/`, `harvest/`, `reports/` and
`pipeline_info/`. Check it with `reports/summary.md` (the gate and one row
per chromosome), then rebuild it from `keys/` (on AWS, after
`aws s3 sync "s3://$BUCKET/results/pc_exons" pc_exons`):

```bash
bash pipeline/tests/check_rebuild.sh <shared>/clingen/results/pc_exons   # or: pc_exons
```

which needs about 24 GB of temporary space (`TMPDIR`) for the unpacked
shards and the new index. Checked offline on 2026-10-25: the SLURM command,
with `-profile slurm,host -stub` and `pipeline/tests/slurm.config` added and
a local copy of the cache, ran through the `sbatch`/`squeue`/`scancel` shims
of `pipeline/tests/run_slurm_shim.sh` (210 jobs, 53 of them on the network
partition: `ENUMERATE`, `LOOKUP`, `FETCH_UCSC`, `CLINVAR_SITES`,
`FETCH_FASTA`) and published `index/` with `metadata.json`, 25 shards in
`keys/`, and the rest; the AWS Batch command's parameters, resolved with
`nextflow config` through `conf/awsbatch.config`, give the `awsbatch`
executor, the queue, the ECR image and `us-east-1`. Neither has run on a
real cluster or AWS account, and an `s3://` `--cache` is untested.

### Limits

- **Alleles across a window edge or a split point.** A window returns only
  the alleles lying wholly inside it, so an allele longer than the 200 bp
  flank (`--flank`) that crosses a window edge is not enumerated, nor one
  longer than twice the hidden `--split_overlap` (1,000 bp) that crosses the
  midpoint of a window split because its answer was full
  ([details](docs/registry-region-query.md)).
- **Outside the windows,** the index holds only the registered alleles of
  the harvested datasets, and enumeration is not incremental: the region
  query cannot ask for what was registered since an earlier run, so every
  run asks for every window again (`--cache` saves only the lookups).
- **Reference alleles are opt-in.** By default the index leaves out the
  Registry's reference alleles (`…=`) and the no-op `delins` alleles that
  put back the reference's own bases, as no VCF record names either;
  `--include_reference` keeps both and adds the manifest source
  `reference_alleles`.
- **The Registry is read-only to the pipeline and gets at most one
  `ENUMERATE` request stream and one `LOOKUP` stream at a time.** `ENUMERATE`
  (region `GET`s) and `LOOKUP` (`POST` lookups) never register anything.
  Each process runs one task at a time (`maxForks 1`), whatever the executor
  could run in parallel, but they are separate processes and the two can
  overlap: in the chr21 run's `pipeline_info/trace.txt` the one `LOOKUP`
  task was submitted at `2026-10-24 22:32:37.634`, while the first
  `ENUMERATE` batch (submitted `2026-10-24 22:31:35.802`, `8m 24s`) was
  still running. With gnomAD datasets the `LOOKUP` batches (`--batch_size`,
  2,000,000 sites by default) are large, so the two streams can overlap for
  long stretches of a run. A run's length is set by the Registry, not by
  the hardware. The Registry is a free public service: contact ClinGen
  before enumerating more than the protein-coding exons.
- **A `--genome` you supply must be a local or shared path** that the
  `BUILD_INDEX` task can open by the name the launcher gave it (the manifest
  records it, and `build` checks it exists). An `s3://` URL, or any
  `--genome` with an `s3://` work directory, is refused when the run starts:
  on AWS Batch leave it out and let `BUILD_GENOME` build it.
- **`-resume` continues one regeneration; a new one needs a fresh start.**
  A resumed run reuses every finished task, the `latest` downloads and every
  `ENUMERATE` batch included, so for newer ClinVar, gnomAD or UCSC data and
  the alleles registered since, launch from a new directory or without
  `-resume` (a new `-work-dir` alone still resumes from the old tasks).
  Within one regeneration the manifest stays true: its
  `registry_<set>_<date>` source is the UTC day of the earliest `ENUMERATE`
  task and its `harvest_date` that of the earliest `ENUMERATE` or `LOOKUP`
  task, and a reused task keeps its day. `pipeline_info/trace.txt` has when
  each task was submitted.

### Tests

`nf-test test` in `pipeline/` runs 119 tests offline (every process, the
functions that assemble `index/metadata.json`, and stub and real workflow
runs against a mock Registry, whose published `keys/` the real runs rebuild
with `pipeline/tests/check_rebuild.sh`; 119 passed in 294 s on 2026-10-25).
`nf-test test tests-network/workflow_mt.nf.test` there runs the workflow on
chrM against the live Registry, checks its `metadata.json` and `keys/`, and
rebuilds its index from `keys/` (passed on 2026-10-25: 8,695 keys). `bash
pipeline/tests/run_slurm_shim.sh` runs the `slurm` profile through
`sbatch`/`squeue`/`scancel` shims that run the jobs locally, and checks each
job's partition and time limit; `bash pipeline/tests/check_profiles.sh`
checks what each profile resolves to, and that a stub run of `-profile
local,host` starts every task on a 16 GB, 2-CPU machine as the local
executor sees one. No real SLURM cluster or AWS account has run the
pipeline.

## chr21 index

Built on 2026-10-23 from the gnomAD v4.1 chr21 harvest, with the chr21-only
genome (UCSC hg38 `chr21.fa.gz` → `chr21.bin`, 46.7 MB), by the commands in
[steps 1 and 2](#annotating-a-vcf-offline) above.

- **entries:** 10,958,882 from 10,958,898 rows. 16 pairs of gnomAD rows are
  one deletion written at two positions; each pair has one HGVS and one CA ID.
- **`caid.fst`:** 129,852,406 bytes (about 11.8 bytes per entry), built in
  3.8 s with a peak of 747,388,928 bytes resident (about 68 bytes per
  harvested row; macOS `/usr/bin/time -l`). The rebuild at `2d0347c` has the
  same entry count and size.
- **conformance:** `clingen-index validate` ([step 2](#2-the-index-once-per-harvest))
  over all rows finds no mismatch in any class that has rows (4.4 s). There
  are no `delins` and no `m.` rows.
  A 10,000-row sample is checked by
  `CLINGEN_CHR21_GENOME=$PWD/data/genome_chr21/chr21.bin cargo test -p clingen-allele --test conformance --release`,
  which also rewrites each sample indel padded and left-aligned.
- **known differences:** none on chr21; the exclusion list in
  [`docs/known-differences.md`](docs/known-differences.md) is empty, and that
  file has the per-class counts.

## ClinVar coverage

ClinVar's weekly GRCh38 VCF of 2026-10-19 (`clinvar.vcf.gz`, md5
`e649bbb1dfe11baafa9feab81a7a7cdd`), every chromosome, looked up in the
Registry and then annotated by both tools. Measured on 2026-10-24.

- **records:** 4,599,326, each with one ALT. 2,060 cannot be looked up and
  are set aside with a reason: 1,341 with `ALT=.` (reference-only records,
  `g.N=`), 668 with a base other than A, C, G or T, and 51 on patch or alt
  contigs (`NT_…`, `NW_…`).
- **harvest:** 25,357 sites were already in the chr21 gnomAD harvest. The
  other 4,571,909 were looked up in 19 requests of 250,000 (about 10 minutes,
  no retries): 4,569,957 registered, 1,952 not (456 of them SNVs, presumably
  too new for the Registry), and no errors.
- **conformance:** `validate` against a whole-genome binary matches every
  one of the 4,571,909 rows: substitution 4,242,074, del 204,832, dup
  82,861, ins 16,591, `delins` 22,417 and `m` (chrM) 3,134. It took 1.6 s.
- **index:** `data/index_corpus` holds the chr21 harvest and the ClinVar
  harvest: 15,528,839 entries (the 16 chr21 pairs above are the only shared
  keys), `caid.fst` 182,447,000 bytes, built in 4.6 s with a peak of
  1,045,839,872 bytes resident.
- **end to end:** both tools annotate all 4,599,326 records offline, and
  `e2e_crosscheck.py` gives the same line for each:
  `checked=4595314 agree=4595314 disagree=0 missing=0`. The tallies are
  `found=4595314 not_in_index=1952 ref_mismatch=0`, with `unsupported=719`
  for vep-rs and `unsupported=2060` for fastVEP; vep-rs drops `ALT=.`
  records before any plugin sees them.
- **bug found:** before `506ccba`, 53,136 of these alleles came out as, say,
  `CA68622` where the Registry writes `CA068622`. The Registry zero-pads to
  six digits (all 15,528,855 harvested IDs have at least six, and only
  six-digit ones start with `0`), and `clingen-allele` now does too. chr21's
  gnomAD alleles were registered late, so none of them has fewer than seven
  digits.

The whole-genome binary holds UCSC hg38's 25 primary contigs
(`hg38.fa.gz`, md5 `1c9dcaddfa41027f17cd8f7a82c7293b`; 3,088,286,401 bytes).
It was first written with a one-off converter around vareffect's
`write_genome_binary`. `clingen-index genome` now writes it, byte for byte
the same (re-run on 2026-10-24: `contigs=25 bases=3088286401`, 10.7 s, both
files identical), and the regeneration pipeline's `BUILD_GENOME` task runs
it when no `--genome` is given
([Regenerating the index](#regenerating-the-index-nextflow)):

```bash
./target/release/clingen-index genome --fasta data/genome_grch38/download/hg38.fa.gz \
    --out data/genome_grch38/GRCh38.bin
```

The rest, with `offline` as [above](#end-to-end-check):

```bash
cv=data/clinvar/20261019
gunzip -c $cv/../download/clinvar.vcf.gz | awk -F'\t' -v OFS='\t' -v skipped=$cv/skipped.tsv '
  BEGIN { split("1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 X Y MT", p, " "); for (i in p) prim[p[i]] = 1 }
  /^#/ { next }
  { why = "" }
  !($1 in prim)                    { why = "non_primary_contig" }
  why == "" && $5 == "."           { why = "no_alt" }
  why == "" && ($4 $5) ~ /[^ACGT]/ { why = "non_acgt" }
  why != "" { print $1, $2, $3, $4, $5, why > skipped; next }
  { print $1, $2, $4, $5 }' > $cv/sites.tsv
cut -f1-4 data/chr21_chunks/chunk_*.tsv |
  awk -F'\t' 'NR==FNR {seen[$0]=1; next} !($0 in seen)' - $cv/sites.tsv > $cv/to_query.tsv
python3 -I harvest/query_registry.py $cv/to_query.tsv $cv/chunks

./target/release/clingen-index validate --chunks $cv/chunks \
    --genome data/genome_grch38/GRCh38.bin --mismatches $cv/mismatches.tsv
./target/release/clingen-index build --chunks data/chr21_chunks $cv/chunks \
    --out data/index_corpus --genome data/genome_grch38/GRCh38.bin \
    --source gnomad41_genomes_chr21 --source clinvar_20261019 --harvest-date 2026-10-24
mkdir -p data/sa_corpus && ln -sfn ../index_corpus data/sa_corpus/clingen
```

The cross-check reads one harvest directory, so it gets the ClinVar chunks
plus a `chunk_99999.tsv` of the 25,357 chr21 rows for ClinVar sites. The
tools read a VCF of ClinVar's first five columns, with a `##contig` line per
primary contig, and run exactly as in the [end-to-end
check](#end-to-end-check), with `index=data/index_corpus` and
`--sa-dir data/sa_corpus`.

### CA IDs in ClinVar lag the Registry

ClinVar's VCF has no CA ID column. A CA ID appears only as a `ClinGen:`
entry in `CLNVI`, for example `CLNVI=ClinGen:CA502008`. Those entries lag
the Registry: in the 2026-10-19 file, 1,716,460 of 4,599,326 records
(37.3%) have none, yet 1,712,474 of the 1,714,424 among them that we looked
up are already registered. They are mostly recent records (median ALLELEID
4,259,556, against 1,670,103 for records with a CA ID). In 11 records,
`CLNVI` names a CA ID other than the one the Registry returns now.

This tool closes that gap: it looks up every ClinVar site in the Registry
itself, so a record gets its CA ID whether or not ClinVar has added one yet.

## Known limitations

- **chr21-only demo data.** The genome and index of the demo cover chr21
  alone. A variant anywhere else (the toy's `melas`, on chrM) gets nothing
  and is counted `unsupported`. For every chromosome, the
  [regeneration pipeline](#regenerating-the-index-nextflow) builds the
  genome and one index with `--chrom all`; that run has not been made yet
  ([The full exome run](#the-full-exome-run-not-run)), and
  [ClinVar coverage](#clinvar-coverage) has a whole-genome binary and an
  index of every ClinVar site plus the chr21 gnomAD harvest.
- **`delins` and chrM rest on ClinVar alone.** The chr21 gnomAD harvest has
  no `delins` and no `m.` rows. ClinVar 2026-10-19 has 22,417 and 3,134, and
  every one matches the Registry ([ClinVar coverage](#clinvar-coverage)), but
  vareffect does not shift a `delins` 3′, so a harvest with `delins` ClinVar
  lacks could still differ; such a difference shows up as misses
  (`not_in_index`), not as wrong CA IDs. Validate every new harvest.
- **Enumeration misses alleles that cross a window edge.** A window returns
  only alleles lying wholly inside it, so an allele longer than the 200 bp
  flank that crosses a window edge, or longer than twice `--split-overlap`
  (2,000 bp) that crosses the midpoint of a split window, is not enumerated
  ([details](docs/registry-region-query.md)).
- **`clingen-index build --chunks` holds every row in memory.** It keeps and
  sorts all harvested (HGVS, CA ID) pairs in memory, about 68 bytes per row:
  the chr21 build (10,958,898 rows) peaks at about 0.75 GB resident
  ([chr21 index](#chr21-index)), about 6.8 GB per 100 million rows. For a
  large or whole-genome index use `build --sorted` instead, which streams
  pairs already sorted by byte (`LC_ALL=C sort`) in constant memory: on the
  15,528,839 pairs of [ClinVar coverage](#clinvar-coverage)'s index it peaked
  at 17,104,896 bytes resident, against 1,045,839,872 for `--chunks`, and
  wrote a byte-identical `caid.fst`. The regeneration pipeline sorts and
  builds this way.
- **Run `clingen-index validate` on every new harvest before building.** It
  is what checks that the HGVS `clingen-allele` computes matches the
  Registry's for that harvest; without it a mismatch shows up only as
  unexplained `not_in_index` misses. It exits 1 only for a substitution
  mismatch, so read every class's `total` and `match` columns
  ([step 2](#2-the-index-once-per-harvest)).
- **A genome of another build is refused.** When it opens the genome,
  `clingen-allele` compares the length of every primary contig the genome
  carries with GRCh38's, and both tools stop at startup on a difference, or on
  a genome with none of the primary contigs. A 1,000-base chr21 stops both
  with `genome … is not GRCh38: contig chr21 is 1000 bases long; GRCh38 chr21
  is 46709983`.
- **fastVEP refuses non-GRCh38 sources when ClinGen is loaded.** With an index
  in `--sa-dir`, a VEP cache, a GFF3 whose `#!genome-build` header names
  another build, or another `--sa-dir` database built for one stops the run
  before any annotation is written, naming the source and its build; a source
  that declares no build is not refused. `--sa-only` ignores `--gff3` and
  `--cache-dir`, so it does not check them. vep-rs refuses an `--assembly`
  other than GRCh38.
- **Nothing checks the input VCF's build.** A GRCh37 VCF run against the
  GRCh38 genome is not refused. 100,000 GRCh37 chr21 SNVs, written from the
  harvest's `HGVS_GRCh37` column with each one's own CA ID as its ID, gave
  `found=2537 not_in_index=22324 ref_mismatch=75139 unsupported=0` in both
  tools, exit 0, and each of the 2,537 CA IDs was another allele's. A large
  `ref_mismatch` share is the sign.
- **vep-rs reports a wrong REF or anchor base on VCF input as
  `ref_mismatch`.** A VCF allele is looked up with its record's own POS, REF
  and ALT, so an indel's anchor base is checked against the genome too:
  `chr21 33000166 GA G`, where the base is `T`, gets no ClinGen fields and
  counts `ref_mismatch=1`, as in fastVEP's CLI.
- **LOCAL-ONLY path dependency.** Both adapter branches reach
  `clingen-allele` by the path `../clingen/crates/clingen-allele`, so neither
  builds, with or without the feature, unless this checkout sits next to
  them. That lasts until `clingen-allele` is published.
- **vep-rs skips some non-VCF input lines.** Every VCF allele is looked up,
  but `--format ensembl` lines on strand -1, `--format region` lines other
  than insertions, and `--format hgvs` insertions, duplications and `del` or
  `delins` without their deleted bases get no ClinGen fields, because they do
  not pin down one plus-strand allele. They are counted `unsupported`.
- **fastVEP takes one ClinGen index per `--sa-dir`.** A second one stops the
  run. To cover several harvests, build one index from all of them
  (`clingen-index build` takes `--chunks` once per harvest directory).
- **fastVEP's library and web path trims indels, and so does not check an
  indel's anchor base.** `fastvep-web` and the library hand every
  supplementary provider Ensembl-style trimmed indel alleles rather than the
  VCF's. That is upstream fastVEP behaviour and affects every provider; the
  ClinGen provider handles it with `clingen-allele`'s `lookup_trimmed`, which
  takes the anchor base from the genome. A wrong anchor base therefore goes
  unnoticed on that path: `21 5311513 A AAAAAACAAGGAAATATCTTTACAC`, where the
  base is `T`, gets `CA1019413128` there, while vep-rs and fastVEP's CLI count
  it `ref_mismatch`.

## Layout

```
crates/      clingen-allele (library) and clingen-index (CLI)
pipeline/    Nextflow index regeneration: workflow, profiles, nf-test, AWS Batch template
Dockerfile   the pipeline's task image (clingen-index and the tools its tasks run)
harvest/     Registry harvester (Python, standard library only)
scripts/     chr21 genome build, conformance sample, end-to-end cross-check
examples/    toy VCF and lookup recipe
docs/        design specs, implementation plans, known differences, Registry region-query findings, AWS Batch guide
data/ logs/  harvest output, genome, index and run outputs (gitignored)
results/     pipeline output (gitignored)
```
