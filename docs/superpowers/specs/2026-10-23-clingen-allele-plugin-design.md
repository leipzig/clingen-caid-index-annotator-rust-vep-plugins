# ClinGen Allele Registry plugin for vep-rs and fastVEP — design

Date: 2026-10-23
Status: draft for review

## Goal

Annotate variants with ClinGen Allele Registry canonical allele IDs (CA IDs) in
[vep-rs](https://github.com/natera-open-source/vep-rs) and
[fastVEP](https://github.com/Huang-lab/fastVEP), fully offline, from a local
database. Lookups are keyed on the Registry's canonical genomic HGVS, not on
CHROM/POS/REF/ALT, so every representation of an indel (left-aligned,
right-aligned, padded) resolves to the same allele.

Success: an annotation run makes zero network calls and returns the same CA ID
the Registry would for every variant present in the local database.

## Scope

In scope:

- GRCh38, primary assembly contigs (1–22, X, Y, MT).
- A one-time harvester that fills the local database from the Registry.
- A shared Rust library and index-building CLI.
- One adapter per tool, behind an optional `clingen` cargo feature.

Out of scope:

- Any network access at annotation time, including fallback to the Registry.
- Registering new alleles. The harvester only ever looks alleles up.
- GRCh37 annotation. GRCh37 HGVS is harvested and kept, but no GRCh37 index is
  built yet.
- Patch and alt contigs, and symbolic or structural alleles.

## Facts this design rests on

Each of these was checked against the live Registry on 2026-10-23.

- No public full dump of the Registry was found. Bulk access is through the API,
  with batches of about 1M variants recommended.
- `POST https://reg.genome.network/alleles?file=vcf&fields=…` is a lookup.
  Unregistered alleles come back as `"@id": "_:CA"`; no ID is minted.
  Registration is a separate, authenticated `PUT`.
- The VCF sent must name contigs without `chr` (`21`) and carry
  `##contig=<ID=21,length=…,assembly=GRCh38>`.
- REF mismatches come back inline as `errorType: IncorrectReferenceAllele`, one
  result per input line, in input order.
- The Registry's HGVS is fully normalized:

  | VCF input | Registry HGVS |
  |---|---|
  | `21:33000166 TA>T`, `21:33000171 AA>A`, `21:33000166 TAAAAAA>TAAAAA` | `NC_000021.9:g.33000172del` |
  | `21:33000166 T>TA`, `21:33000172 A>AA` | `NC_000021.9:g.33000172dup` |
  | `21:33000127 C>CTC` | `NC_000021.9:g.33000126_33000127dup` |
  | `21:33000166 T>TAG` | `NC_000021.9:g.33000167_33000168insGA` |
  | `21:33000017 CA>TG` | `NC_000021.9:g.33000017_33000018delinsTG` (not `inv`) |
  | `MT:3243 A>G` | `NC_012920.1:m.3243A>G` → `CA120560` (`m.`, not `g.`) |
  | `21:5030578 C>T` | `NC_000021.9:g.5030578C>T` → `CA749862939` |

- [vareffect](https://docs.rs/vareffect) 0.6 (Apache-2.0) provides
  `hgvs_g::format_hgvs_g(chrom, pos0, ref, alt, &FastaReader) -> Result<Option<String>>`.
  It needs only the genome:
  - It verifies REF, trims the shared suffix then the shared prefix, shifts 3′,
    rotates insertions, and detects `dup`. It does not shift `delins`.
  - By hand trace, it matches every example above except the `m.` prefix.
  - It requires `chr`-prefixed or `NC_` contig names and 0-based positions.
  - It reads vareffect's own memory-mapped genome binary (`GRCh38.bin` plus
    `.bin.idx`, about 3.1 GB), built with `vareffect-cli`.
  - It requires edition 2024 and Rust 1.94.1 or newer.
- Neither tool produces Registry-form HGVSg today:
  - vep-rs accepts `--hgvsg` but never computes it.
  - fastVEP writes `21:g.…`, does not shift 3′, and never emits `dup`.
- mehari and hgvs-rs were considered and rejected:
  - mehari's `g.` is not canonical, and it pulls in rocksdb, tokio and actix.
  - hgvs-rs writes `inv` for reverse-complement substitutions and pulls in
    postgres and seqrepo.

## Architecture

```
       one-time (network)                                  every run (offline)
gnomAD sites ─► harvest/query_registry.py ─► chunk TSVs ─► clingen-index build ─► caid.fst + manifest.json
                (POST lookups only)          CHROM POS REF ALT CAID HGVS38 HGVS37

VCF record ─► vep-rs / fastVEP adapter ─► clingen_allele::Annotator
                                           ├─ vareffect::format_hgvs_g (GRCh38.bin) → canonical HGVS
                                           └─ caid.fst lookup (HGVS → CA number)    → CA ID
```

### Repository layout (this repo)

```
harvest/                     Python 3.9, standard library only
  extract_gnomad_sites.sh    gnomAD v4.1 genomes sites VCF → CHROM POS REF ALT
  query_registry.py          sites TSV → chunk TSVs (resumable, lookup only)
crates/
  clingen-allele/            library: hgvs, index, Annotator
  clingen-index/             CLI: build, validate
data/  logs/                 gitignored
```

## Components

### Harvester (`harvest/`)

- `extract_gnomad_sites.sh <chrom> <out.tsv>` streams the gnomAD v4.1 genomes
  sites VCF for one chromosome and keeps CHROM (with `chr` stripped), POS, REF
  and ALT. The file is not stored.
- `query_registry.py <sites.tsv> <out_dir> [--chunk-size 250000]`:
  - posts chunks to the lookup endpoint;
  - writes `chunk_NNNNN.tsv` with columns
    `CHROM POS REF ALT CAID HGVS_GRCh38 HGVS_GRCh37`;
  - writes CAID as an empty string when the allele is unregistered, and as
    `ERROR:<type>` on a per-line error;
  - checks that the result count matches the chunk size;
  - retries with exponential backoff;
  - skips chunks already written, so it can be re-run to resume.
- The first run covers gnomAD chr21. Later runs add more chromosomes and more
  indel sources (dbSNP, ClinVar) through the same scripts. The "every possible
  SNV" pass is a separate decision, made after chr21 shows the run time and the
  registration rate.

### `clingen-allele` (library)

- **`hgvs` module** wraps `vareffect::hgvs_g::format_hgvs_g`:
  - It maps the input contig name (`21`, `chr21`, `NC_000021.9`, `MT`, `chrM`)
    to the name vareffect expects, and converts the 1-based VCF position to
    0-based.
  - It rewrites the `NC_012920.1:g.` prefix to `NC_012920.1:m.`.
  - It returns `None` without calling vareffect for:
    - a symbolic ALT (`<…>`, `*`, breakends);
    - an empty or multi-allelic ALT (callers pass one ALT at a time);
    - non-ACGT bases in REF or ALT.
- **`index` module**:
  - The index is an `fst::Map` from the HGVS string (bytes) to a `u64` CA
    number. `CA749862939` is stored as `749862939` and printed back with the
    `CA` prefix.
  - Only registered alleles are stored.
  - `manifest.json` records assembly (`GRCh38`), source files, harvest date,
    entry count and the vareffect version.
  - The index is memory-mapped.
- **`Annotator`**:
  - `Annotator::open(index_dir, genome_bin) -> Result<Annotator>` checks that
    the manifest's assembly is GRCh38.
  - `Annotator::lookup(&self, chrom, pos1, ref, alt) -> Lookup`.
  - `Lookup { hgvs: Option<String>, caid: Option<String>, status: Status }`,
    where `Status` is one of `Found`, `NotInIndex`, `RefMismatch`,
    `Unsupported`.
  - It is `Send + Sync`. Each worker thread gets its own genome reader via
    `FastaReader::try_clone`.
  - `Annotator::stats()` returns per-status counts for the end-of-run summary.

### `clingen-index` (CLI)

- **`build --chunks DIR... --out INDEX_DIR`**:
  - reads harvest TSVs and keeps rows with a CA ID;
  - sorts by HGVS key and removes duplicates (the same HGVS with two different
    CA IDs is a hard error, reported in full);
  - writes `caid.fst` and `manifest.json`.
- **`validate --chunks DIR... --genome GRCh38.bin [--sample N]`**:
  - recomputes HGVS with `clingen_allele::hgvs` for every row that is not an
    error, and compares it to `HGVS_GRCh38`;
  - reports the match rate by type (substitution, del, ins, dup, delins);
  - writes every mismatch to a TSV.

### vep-rs adapter

- This is a new built-in plugin, `ClinGen`, in `crates/vep-builtins/src/plugins/clingen.rs`,
  registered in `create_builtin()` (`crates/vep-builtins/src/registry.rs`). It is
  compiled only with `--features clingen`, because vareffect needs Rust 1.94.1
  or newer and vep-rs's minimum is 1.88.
- Usage: `--plugin ClinGen,index=DIR,genome=PATH/GRCh38.bin`.
- `header_info()` declares two fields:
  - `ClinGen_CAID`: ClinGen Allele Registry canonical allele ID;
  - `ClinGen_HGVSg`: ClinGen canonical genomic HGVS.
- Its lookups are variant-level and use the CADD plugin's two-phase
  `prefetch`/`annotate` pattern:
  - `prefetch` computes the keys for the batch and sorts them, so the index
    pages are read roughly in order;
  - `annotate` writes `variant.plugin_data`.
- The VCF-form REF/ALT is rebuilt from `InputVariant`'s `original_start` and
  `original_allele_string`, because vep-rs trims alleles internally.
- `set_assembly` rejects any assembly other than GRCh38.
- This lives on a branch of a fork, with the intent of an upstream pull request.

### fastVEP adapter

- This is a new `ClinGenProvider` implementing
  `fastvep_cache::annotation::AnnotationProvider`, compiled only with the
  `clingen` feature.
  - `json_key`: `clingen`.
  - `match_by_allele`: true.
  - `annotate_position(chrom, pos, ref, alt)` receives VCF-form alleles and
    calls `Annotator::lookup`. It returns
    `AnnotationValue::Json({"caid": "...", "hgvsg": "..."})`, or `None` when
    there is no HGVS.
- Loading: `load_sa_providers` (`crates/fastvep-annotate/src/lib.rs`) loads a
  `ClinGenProvider` when a directory under `--sa-dir` contains a ClinGen
  `manifest.json`. The manifest gives the genome path; `--clingen-genome`
  overrides it.
- Output:
  - one `VCF_PROJECTION_SPECS` entry (`crates/fastvep-io/src/output.rs`) that
    produces `##INFO=<ID=FV_CLINGEN,…>` with the value `CAID|HGVSg`, plus a tab
    column;
  - JSON under `clingen`;
  - a new row and pipe format in `docs/SUPPLEMENTARY_ANNOTATIONS.md`, required by
    fastVEP's CI documentation check.
- Lookups go through the same provider call in both the CLI loop and the shared
  library loop. The adapter must check that the library and web path pass
  VCF-form alleles too: an earlier exploration suggested that path may pass
  trimmed alleles. If it does, the adapter rebuilds the VCF form instead of
  silently missing indels.
- This lives on a branch of a fork, with the intent of an upstream pull request.

## Error handling

| Case | Result |
|---|---|
| HGVS computed, CA ID found | both fields set (`Found`) |
| HGVS computed, not in index | HGVS set, CAID empty (`NotInIndex`) |
| REF does not match the genome | both empty (`RefMismatch`), counted |
| Patch or alt contig, symbolic or IUPAC allele, insertion at a contig edge | both empty (`Unsupported`), counted |
| Index or genome missing, or manifest assembly not GRCh38 | startup error, run aborts |
| Tool assembly not GRCh38 | startup error, run aborts |

At the end of a run, each adapter prints the per-status counts to stderr.

## Performance

- Both the genome and the index are memory-mapped. A lookup is one vareffect
  call (a few small reads, plus 3′-shift reads in repeats) and one FST lookup
  (microseconds).
- Lookups need no locks, and they parallelize across the tools' existing batch
  threads.
- The index is expected to take a few bytes per key. The chr21 build gives the
  real figure, which will be recorded in the manifest and the README.

## Testing

1. **Unit tests in `clingen-allele`:**
   - a tiny genome built with `vareffect::fasta::write_genome_binary`;
   - every example in "Facts this design rests on";
   - the contig-name forms (`21`, `chr21`, `NC_000021.9`, `MT`);
   - REF mismatch, symbolic ALT, non-ACGT bases;
   - FST build/lookup round trip, duplicate-conflict error, manifest assembly check.
2. **Conformance against the Registry (`clingen-index validate`):**
   - **Data:** all harvested chr21 rows.
   - **Acceptance:** 100% match on substitutions. Every indel mismatch is either
     fixed in the `hgvs` wrapper or listed with its reason in
     `docs/known-differences.md`.
   - **Fixture:** a 10k-row sample (at least 2k indels) committed as
     `crates/clingen-allele/tests/data/chr21_sample.tsv`, run in CI against a
     chr21-only genome binary built by the test setup.
3. **Representation invariance:** each indel in the sample is rewritten
   left-aligned, right-aligned and padded with extra reference bases. Every form
   must give the same HGVS and CA ID.
4. **Adapter tests, in each repo's own style:**
   - **vep-rs:**
     - an in-module test modelled on the CADD plugin's fixture tests;
     - a `crates/vep-cli/tests` integration test of `--plugin ClinGen` that
       checks the CSQ subfields and the header.
   - **fastVEP:**
     - a provider-loading test next to
       `crates/fastvep-annotate/tests/sa_provider_loading.rs`;
     - an output test for the `FV_CLINGEN` header and value;
     - its INFO-field documentation check must pass.
   - Both run with network access disabled.
5. **End-to-end:** run 100k chr21 gnomAD variants through each tool and compare
   the CA IDs to the harvest. Every row the harvest has an ID for must match.

## Risks

- **vareffect's genome binary is a second copy of GRCh38** (about 3.1 GB) next to
  each tool's FASTA. Mitigation: an upstream vareffect pull request making
  `format_hgvs_g` accept any sequence source. That is a follow-up, not part of
  this work.
- **Complex alleles in repeats:** `delins` is not shifted, so the Registry may
  choose a different form. The conformance run measures this.
- **Coverage:** CA IDs exist only for alleles in the harvested sites. Novel
  variants get HGVS but no CA ID, by design.
- **Load on the Registry:** harvesting beyond gnomAD is a large job on a free
  public service. Contact ClinGen before running the genome-wide pass.
