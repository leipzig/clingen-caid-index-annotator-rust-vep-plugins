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
| `clingen-index` CLI (`build`, `validate`) | **works**; on local `main` |
| vep-rs plugin `--plugin ClinGen` | **works** offline; merged into a local vep-rs `main`, not pushed |
| fastVEP provider (`FV_CLINGEN`) | **works** offline; merged into a local fastVEP `master`, not pushed |
| chr21 conformance with the Registry | all 10,958,898 rows match the Registry's HGVS ([details](docs/known-differences.md)) |
| ClinVar 2026-10-19, every chromosome | all 4,571,909 harvested rows match the Registry's HGVS, 22,417 `delins` and 3,134 chrM among them; both tools give the Registry's CA ID for all 4,595,314 registered ClinVar alleles ([ClinVar coverage](#clinvar-coverage)) |
| End-to-end check, first 100,000 chr21 sites | both tools: all 100,000 CA IDs agree with the harvest ([below](#end-to-end-check)) |
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

A whole-genome index needs vareffect's full GRCh38 genome instead, about
3.1 GB, built once with `vareffect-cli` (`cargo install vareffect-cli`, then
`vareffect init` and `vareffect setup`, as vareffect's README describes; not
run here).

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
one, and a build that fails before the rename leaves it in place. `build` holds every row in memory
(see [Known limitations](#known-limitations)). [chr21 index](#chr21-index)
has its size.

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
It was written with a one-off converter around vareffect's
`write_genome_binary`; the regeneration pipeline adds this step to
`clingen-index`. The rest, with `offline` as [above](#end-to-end-check):

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

## Known limitations

- **chr21-only demo data.** The genome and index here cover chr21 alone. A
  variant anywhere else (the toy's `melas`, on chrM) gets nothing and is
  counted `unsupported`. Whole-genome use needs the full GRCh38 genome and a
  harvest of every chromosome, built into one index.
- **`delins` and chrM rest on ClinVar alone.** The chr21 gnomAD harvest has
  no `delins` and no `m.` rows. ClinVar 2026-10-19 has 22,417 and 3,134, and
  every one matches the Registry ([ClinVar coverage](#clinvar-coverage)), but
  vareffect does not shift a `delins` 3′, so a harvest with `delins` ClinVar
  lacks could still differ; such a difference shows up as misses
  (`not_in_index`), not as wrong CA IDs. Validate every new harvest.
- **`clingen-index build` holds every row in memory.** It keeps and sorts all
  harvested (HGVS, CA ID) pairs in memory, about 68 bytes per row: the chr21
  build (10,958,898 rows) peaks at about 0.75 GB resident
  ([chr21 index](#chr21-index)). That is about 6.8 GB per 100 million rows,
  so a whole-genome build needs an external sort first, which `build` does
  not have yet.
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
harvest/     Registry harvester (Python, standard library only)
scripts/     chr21 genome build, conformance sample, end-to-end cross-check
examples/    toy VCF and lookup recipe
docs/        design spec, implementation plan, known differences
data/ logs/  harvest output, genome, index and run outputs (gitignored)
```
