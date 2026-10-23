# ClinGen Allele Registry Plugin Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Annotate variants in vep-rs and fastVEP with ClinGen Allele Registry CA IDs, fully offline, keyed on the Registry's canonical genomic HGVS.

**Architecture:** This repo gains a Cargo workspace with two crates:
- `clingen-allele` (library) computes Registry-form HGVS with `vareffect::hgvs_g::format_hgvs_g` and maps HGVS to a CA ID through a memory-mapped `fst::Map`.
- `clingen-index` (CLI) builds that index from the harvester's TSVs, and validates our HGVS against the Registry's.

Each tool gets a thin adapter behind a `clingen` cargo feature, on a branch of a fork.

**Tech Stack:** Rust 1.94.1+, edition 2024, `vareffect` 0.6, `fst`, `memmap2`, `serde`/`serde_json`, `clap`, `thiserror`. The harvester is Python 3.9 (standard library only), and already exists.

**Spec:** `docs/superpowers/specs/2026-10-23-clingen-allele-plugin-design.md`

## Global Constraints

- Assembly GRCh38 only; primary contigs 1–22, X, Y, MT. Patch and alt contigs → `Unsupported`.
- No network access at annotation time, including in tests of the adapters. The harvester is the only code that touches the network, and it only ever uses `POST /alleles` (lookup), never `PUT`.
- vareffect requires Rust ≥ 1.94.1 and edition 2024. In vep-rs (minimum Rust 1.88) and fastVEP, all new code sits behind an optional, non-default `clingen` feature.
- HGVS keys are byte-identical to the Registry's GRCh38 `NC_` HGVS, e.g. `NC_000021.9:g.33000172dup`, `NC_012920.1:m.3243A>G`.
- A CA ID is stored as a `u64` without its prefix and printed as `CA{n}`.
- Output field names:
  - vep-rs: `ClinGen_CAID`, `ClinGen_HGVSg`.
  - fastVEP: JSON key `clingen` with `caid` and `hgvsg`; VCF INFO `FV_CLINGEN` in the format `ALLELE|CAID|HGVSG`. The spec said `CAID|HGVSg`; this follows fastVEP's existing `AlleleObject` convention, which always starts with `ALLELE`.
- Licensing: everything new is Apache-2.0, the license of both tools and of vareffect.

## Review Focus

1. **Lowercase or soft-masked REF/ALT in the input VCF** should be uppercased before the HGVS is built, giving the same key as uppercase input. Test: `lowercase_alleles_match_uppercase` (Task 2).
2. **Contig naming drift** (`chr21`, `21`, `NC_000021.9`, `MT`, `chrM`, `chrMT`, `M`) should all resolve to the same accession, and an unknown contig should give `Unsupported`. Test: `contig_aliases` (Task 2).
3. **A multi-allelic ALT string passed by mistake** (`A,G`) should give `Unsupported`, not a malformed key. Test: `comma_alt_is_unsupported` (Task 2).
4. **Two harvest rows with the same HGVS but different CA IDs** (for example, overlapping chunks from two sources) should fail the build and name both rows. The same HGVS with the same CA ID is a silent duplicate. Test: `conflicting_duplicate_fails` (Task 3).
5. **An index built for the wrong assembly, or a stale manifest**, should stop the run at startup, not produce empty fields. Test: `manifest_wrong_assembly_rejected` (Task 4).

---

## File Structure

```
Cargo.toml                                  workspace (members: crates/*), edition 2024, rust-version 1.94.1
rust-toolchain.toml                         channel = "1.94.1"
crates/clingen-allele/
  Cargo.toml                                deps: vareffect 0.6, fst, memmap2, serde, serde_json, thiserror
  src/lib.rs                                re-exports; Error type
  src/contig.rs                             contig name → (vareffect name, NC_ accession)
  src/hgvs.rs                               canonical_hgvs()
  src/index.rs                              Manifest, IndexBuilder, CaIndex
  src/annotator.rs                          Annotator, Lookup, Status, Stats
  tests/common/mod.rs                       tiny-genome fixture builder
  tests/data/windows.fa                     real GRCh38 windows (chr21, chrM) used by the fixture
  tests/hgvs_examples.rs                    spec examples + review-focus cases
  tests/index_roundtrip.rs
  tests/annotator.rs
  tests/conformance.rs                      chr21 sample vs Registry HGVS (needs CLINGEN_CHR21_GENOME)
  tests/data/chr21_sample.tsv               10k harvest rows (≥2k indels)
crates/clingen-index/
  Cargo.toml                                deps: clingen-allele, clap, anyhow
  src/main.rs                               build | validate subcommands
  tests/cli.rs
scripts/build_chr21_genome.sh               UCSC chr21 FASTA → vareffect .bin/.bin.idx (chr21-only, for tests)
scripts/sample_harvest.py                   writes tests/data/chr21_sample.tsv from data/chr21_chunks
README.md
```

Adapters, on fork branches named `clingen-allele-plugin`:

```
vep-rs:  crates/vep-builtins/Cargo.toml                  optional dep clingen-allele, feature `clingen`
         crates/vep-builtins/src/plugins/clingen.rs      ClinGenPlugin
         crates/vep-builtins/src/plugins/mod.rs          #[cfg(feature="clingen")] pub mod clingen;
         crates/vep-builtins/src/registry.rs:332-347     "ClinGen" arm
         crates/vep-cli/Cargo.toml                       feature `clingen` = ["vep-builtins/clingen"]
         crates/vep-cli/tests/clingen_plugin.rs
         docs/developers/plugins/clingen.md (or the plugin docs location the repo uses)
fastVEP: crates/fastvep-annotate/Cargo.toml              optional dep clingen-allele, feature `clingen`
         crates/fastvep-annotate/src/clingen.rs          ClinGenProvider
         crates/fastvep-annotate/src/lib.rs:1627-1650    load ClinGen index dirs in load_sa_providers
         crates/fastvep-io/src/output.rs:603,633+        CLINGEN_FIELDS + VCF_PROJECTION_SPECS entry
         crates/fastvep-cli/src/main.rs                  --clingen-genome flag; feature passthrough
         crates/fastvep-annotate/tests/clingen_provider.rs
         docs/SUPPLEMENTARY_ANNOTATIONS.md               table row + pipe format
```

---

### Task 1: Workspace scaffold and contig mapping

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `crates/clingen-allele/Cargo.toml`, `crates/clingen-allele/src/lib.rs`, `crates/clingen-allele/src/contig.rs`

**Interfaces:**
- Produces:
  - `pub struct Contig { pub vareffect_name: &'static str, pub accession: &'static str, pub mito: bool }`
  - `pub fn resolve_contig(name: &str) -> Option<Contig>`
  - `pub enum Error` (thiserror), with variants `Genome(vareffect::VarEffectError)`, `Io { path: PathBuf, source: std::io::Error }`, `Manifest(String)`, `Fst(fst::Error)`, `DuplicateConflict { hgvs: String, first: u64, second: u64 }`.

- [ ] **Step 1: Install the toolchain.** Run `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.94.1`. Expected: `cargo --version` prints `cargo 1.94.1`. Ask the user before running it; it installs software.
- [ ] **Step 2: Write the failing test.** This is a unit test in `contig.rs`, `resolve_contig_forms`:
  - `"21"`, `"chr21"` and `"NC_000021.9"` all → `Contig { vareffect_name: "chr21", accession: "NC_000021.9", mito: false }`;
  - `"MT"`, `"chrM"`, `"chrMT"`, `"M"` and `"NC_012920.1"` → `vareffect_name: "chrM"`, `accession: "NC_012920.1"`, `mito: true`;
  - `"X"` → `"NC_000023.11"`, and `"Y"` → `"NC_000024.10"`;
  - `"chr1_KI270706v1_random"`, `"NW_009646194.1"` and `"foo"` → `None`.
- [ ] **Step 3:** Run `cargo test -p clingen-allele resolve_contig_forms`. Expected: FAIL to compile, because `resolve_contig` does not exist.
- [ ] **Step 4: Implement `resolve_contig`.** Strip a leading `chr` (case-insensitive), map `M`/`MT` to `M`, and look the result up in a static 25-entry table for 1–22, X, Y, M. Accessions come from `vareffect::chrom` (see `vareffect/src/chrom.rs:40-44`); copy the table, don't call a private function. `NC_` inputs match the accession column.
- [ ] **Step 5:** Run `cargo test -p clingen-allele`. Expected: PASS.
- [ ] **Step 6: Commit.** `git commit -m "feat(clingen-allele): workspace scaffold and contig resolution"`

### Task 2: Canonical HGVS

**Files:**
- Create: `crates/clingen-allele/src/hgvs.rs`, `crates/clingen-allele/tests/common/mod.rs`, `crates/clingen-allele/tests/data/windows.fa`, `crates/clingen-allele/tests/hgvs_examples.rs`

**Interfaces:**
- Consumes: `resolve_contig` (Task 1).
- Produces:
  - `pub enum HgvsResult { Hgvs(String), RefMismatch, Unsupported }`
  - `pub fn canonical_hgvs(chrom: &str, pos1: u64, ref_allele: &[u8], alt_allele: &[u8], genome: &vareffect::FastaReader) -> Result<HgvsResult, Error>`
  - test helper `common::tiny_genome() -> (tempfile::TempDir, vareffect::FastaReader)`

- [ ] **Step 1: Create the fixture `tests/data/windows.fa`.** Fetch three windows from `https://api.genome.ucsc.edu/getData/sequence?genome=hg38;chrom=…;start=…;end=…` and write them as FASTA with headers `>chr21:33000001-33000300`, `>chr21:5030500-5030650`, `>chrM:3200-3300`. The API `start` is 0-based (e.g. `start=33000000;end=33000300`). Commit the file; the tests must not fetch.
- [ ] **Step 2: Write `common::tiny_genome()`.**
  - It builds `chr21` as a 33,000,400-byte buffer of `N` with both chr21 windows pasted at their 1-based offsets, and `chrM` as 16,569 `N` with its window pasted.
  - It writes both through `vareffect::fasta::write_genome_binary(&[("chr21", &chr21), ("chrM", &chrm)], "GRCh38", &bin, &idx)` into a tempdir, then calls `FastaReader::open(&bin)`.
  - Add `tempfile` as a dev-dependency.
- [ ] **Step 3: Write the failing tests in `tests/hgvs_examples.rs`.** Each one asserts `canonical_hgvs(...)` equals `HgvsResult::Hgvs(<expected>)` unless stated:

  | test | input (chrom, pos, ref, alt) | expected |
  |---|---|---|
  | `snv` | `21 5030578 C T` | `NC_000021.9:g.5030578C>T` |
  | `del_left_aligned` | `21 33000166 TA T` | `NC_000021.9:g.33000172del` |
  | `del_right_aligned` | `21 33000171 AA A` | `NC_000021.9:g.33000172del` |
  | `del_padded` | `21 33000166 TAAAAAA TAAAAA` | `NC_000021.9:g.33000172del` |
  | `dup_left` | `21 33000166 T TA` | `NC_000021.9:g.33000172dup` |
  | `dup_right` | `21 33000172 A AA` | `NC_000021.9:g.33000172dup` |
  | `dinuc_dup` | `21 33000127 C CTC` | `NC_000021.9:g.33000126_33000127dup` |
  | `ins_rotated` | `21 33000166 T TAG` | `NC_000021.9:g.33000167_33000168insGA` |
  | `delins_revcomp` | `21 33000017 CA TG` | `NC_000021.9:g.33000017_33000018delinsTG` |
  | `mito_m_prefix` | `MT 3243 A G` | `NC_012920.1:m.3243A>G` |
  | `ref_mismatch` | `21 33000166 G T` | `HgvsResult::RefMismatch` |
  | `symbolic_alt` | `21 33000166 T <DEL>`, then `T *` | `Unsupported` |
  | `iupac_alt` | `21 33000166 T R` | `Unsupported` |
  | `lowercase_alleles_match_uppercase` | `21 33000166 ta t` | same as `del_left_aligned` |
  | `contig_aliases` | `chr21`, `NC_000021.9` with the `snv` input; `chrM` with the `mito_m_prefix` input; `chr1_KI270706v1_random` | same strings, and `Unsupported` for the last |
  | `comma_alt_is_unsupported` | `21 33000166 T A,G` | `Unsupported` |

- [ ] **Step 4:** Run `cargo test -p clingen-allele --test hgvs_examples`. Expected: FAIL to compile.
- [ ] **Step 5: Implement `canonical_hgvs`.** In order:
  1. Return `Unsupported` if ALT contains any of `,` `<` `>` `*` `[` `]` `.`, or if `resolve_contig` returns `None`.
  2. Uppercase REF and ALT. Return `Unsupported` if either contains anything outside `ACGT`. This is stricter than vareffect, which allows `N`.
  3. Check REF with `genome.verify_ref` (`vareffect/src/fasta.rs:599`) → `RefMismatch`.
  4. Call `format_hgvs_g(contig.vareffect_name, pos1 - 1, &r, &a, genome)`. `None` → `Unsupported`.
  5. Replace the leading `chrN:`/UCSC name with `contig.accession`, if vareffect returned one; otherwise keep its `NC_` prefix.
  6. If `contig.mito`, replace the first `:g.` with `:m.`.
- [ ] **Step 6:** Run `cargo test -p clingen-allele`. Expected: PASS, 16 tests in `hgvs_examples`.
- [ ] **Step 7: Commit.** `git commit -m "feat(clingen-allele): canonical Registry HGVS via vareffect"`

### Task 3: FST index and manifest

**Files:**
- Create: `crates/clingen-allele/src/index.rs`, `crates/clingen-allele/tests/index_roundtrip.rs`

**Interfaces:**
- Produces:
  - `#[derive(Serialize, Deserialize)] pub struct Manifest { pub format_version: u32 /* = 1 */, pub assembly: String, pub sources: Vec<String>, pub harvest_date: String, pub entries: u64, pub vareffect_version: String }`
  - `pub struct IndexBuilder` with:
    - `new() -> Self`
    - `add(&mut self, hgvs: &str, caid: u64)`
    - `finish(self, dir: &Path, manifest: Manifest) -> Result<u64, Error>`: sorts, removes duplicates, writes `caid.fst` and `manifest.json`, and returns the entry count.
  - `pub struct CaIndex` with:
    - `open(dir: &Path) -> Result<CaIndex, Error>`, which memory-maps `caid.fst`;
    - `get(&self, hgvs: &str) -> Option<u64>`;
    - `manifest(&self) -> &Manifest`.
  - `pub fn parse_caid(s: &str) -> Option<u64>` (`"CA749862939"` → `749862939`) and `pub fn format_caid(n: u64) -> String`.

- [ ] **Step 1: Write the failing tests.**
  - `roundtrip`: add the keys from Task 2 with fixed IDs (`NC_000021.9:g.5030578C>T` → 749862939, `NC_012920.1:m.3243A>G` → 120560, two more), deliberately out of order. `finish` returns 4. After `open`, `get` returns each ID, and `get("NC_000021.9:g.1A>T")` is `None`.
  - `exact_duplicate_ok`: the same key and ID added twice → `entries == 1`.
  - `conflicting_duplicate_fails`: the same key with 1 and 2 → `Err(Error::DuplicateConflict { hgvs, first: 1, second: 2 })`.
  - `caid_parse_format`: `parse_caid("CA120560") == Some(120560)`; `parse_caid("_:CA")`, `parse_caid("")` and `parse_caid("ERROR:IncorrectReferenceAllele")` are `None`; `format_caid(120560) == "CA120560"`.
  - `manifest_persisted`: `open(dir).manifest().assembly == "GRCh38"`.
- [ ] **Step 2:** Run `cargo test -p clingen-allele --test index_roundtrip`. Expected: FAIL to compile.
- [ ] **Step 3: Implement.** `IndexBuilder` collects `(String, u64)` in a `Vec`, sorts it unstably by key, removes duplicates while checking for conflicts, and streams into `fst::MapBuilder` over a `BufWriter<File>`. `CaIndex` holds `fst::Map<memmap2::Mmap>`. The chr21 harvest is small enough to sort in memory. Build an external sort only when a genome-wide build needs it (YAGNI).
- [ ] **Step 4:** Run `cargo test -p clingen-allele`. Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "feat(clingen-allele): FST HGVS→CA index with manifest"`

### Task 4: Annotator

**Files:**
- Create: `crates/clingen-allele/src/annotator.rs`, `crates/clingen-allele/tests/annotator.rs`
- Modify: `crates/clingen-allele/src/lib.rs` (re-export `Annotator, Lookup, Status, Stats, canonical_hgvs, HgvsResult, CaIndex, IndexBuilder, Manifest, parse_caid, format_caid`)

**Interfaces:**
- Consumes: `canonical_hgvs` (Task 2), `CaIndex` (Task 3).
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Status { Found, NotInIndex, RefMismatch, Unsupported }`
  - `pub struct Lookup { pub hgvs: Option<String>, pub caid: Option<String>, pub status: Status }`
  - `pub struct Stats { pub found: u64, pub not_in_index: u64, pub ref_mismatch: u64, pub unsupported: u64 }`, which implements `Display` as `ClinGen: found=N not_in_index=N ref_mismatch=N unsupported=N`.
  - `pub struct Annotator` (`Send + Sync`) with:
    - `open(index_dir: &Path, genome_bin: &Path) -> Result<Annotator, Error>`
    - `lookup(&self, chrom: &str, pos1: u64, ref_allele: &[u8], alt_allele: &[u8]) -> Lookup`
    - `stats(&self) -> Stats`
    - `genome_path_from_manifest(index_dir: &Path) -> Option<PathBuf>`, for callers that rely on the manifest's optional `genome` field. Add `pub genome: Option<String>` to `Manifest`.

- [ ] **Step 1: Write the failing tests.**
  - Use the tiny genome plus an index holding only `NC_000021.9:g.5030578C>T` → 749862939 and `NC_000021.9:g.33000172dup` → 42.
  - `found_snv`: `lookup("21",5030578,b"C",b"T")` → `Lookup { hgvs: Some("NC_000021.9:g.5030578C>T"), caid: Some("CA749862939"), status: Found }`.
  - `found_via_any_representation`: both `33000166 T TA` and `33000172 A AA` → `caid == Some("CA42")`.
  - `not_in_index`: `33000166 TA T` → `status == NotInIndex`, `hgvs == Some("NC_000021.9:g.33000172del")`, `caid == None`.
  - `ref_mismatch_counts`: after one `RefMismatch` lookup, `stats().ref_mismatch == 1`.
  - `manifest_wrong_assembly_rejected`: an index whose manifest says `GRCh37` → `Annotator::open` is `Err(Error::Manifest(_))`, and the message contains `GRCh37`.
  - `missing_genome_is_error`: a nonexistent genome path → `Err`.
  - `send_sync`: `fn assert_send_sync<T: Send + Sync>() {}` then `assert_send_sync::<Annotator>()`.
- [ ] **Step 2:** Run `cargo test -p clingen-allele --test annotator`. Expected: FAIL to compile.
- [ ] **Step 3: Implement.** Counters are `AtomicU64` with `Relaxed` ordering. A `Genome` read error inside `lookup` counts as `Unsupported` and is logged once through `eprintln!`. `lookup` never panics and never returns `Result`, because a per-variant problem must not abort a run.

  A `FastaReader` is shared by reference: `try_clone` exists for callers that want per-thread handles, but the reader is already `Sync` (it is an `Arc` over a memory map, `fasta.rs:484-490`). The spec mentioned per-thread clones; they are not needed.
- [ ] **Step 4:** Run `cargo test -p clingen-allele`. Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "feat(clingen-allele): Annotator with status accounting"`

### Task 5: `clingen-index` CLI (build, validate)

**Files:**
- Create: `crates/clingen-index/Cargo.toml`, `crates/clingen-index/src/main.rs`, `crates/clingen-index/tests/cli.rs`

**Interfaces:**
- Consumes: `IndexBuilder`, `Manifest`, `parse_caid`, `canonical_hgvs`, `resolve_contig`.
- Produces the CLI:
  - `clingen-index build --chunks <DIR>... --out <DIR> [--source <label>]... [--genome <bin path to record in manifest>] [--harvest-date YYYY-MM-DD]`
  - `clingen-index validate --chunks <DIR>... --genome <bin> [--sample <N>] --mismatches <out.tsv>`
  - `validate` prints one line per class, `<class>\t<total>\t<match>\t<pct>`, for the classes `substitution del dup ins delins m`. It exits with code 1 if any substitution mismatches.
  - The class is taken from the Registry HGVS's suffix: `>` → substitution; endings `del`, `dup`, `ins…`, `delins…`; and `m.` for mitochondrial.

- [ ] **Step 1: Write the failing tests in `tests/cli.rs`, with `assert_cmd`.**
  - `build_from_chunks`: two chunk files in the harvester's 7-column format, including:
    - a blank-CAID row and an `ERROR:IncorrectReferenceAllele` row, both of which must be skipped;
    - two rows for the same HGVS with the same CA ID, which count once.

    Exit 0; `manifest.json` has `entries == 3` and `assembly == "GRCh38"`; `CaIndex::open` finds each ID.
  - `build_conflict_exits_nonzero`: the same HGVS with two CA IDs → nonzero exit, and stderr contains both `CA` values.
  - `validate_reports_classes`: build a tiny genome with the Task 2 helper. Make the `common` module a `pub mod test_support` behind a `test-support` feature of `clingen-allele`, so this crate can use it.

    Write chunk rows for `snv`, `dup_left` and `mito_m_prefix` with the correct Registry HGVS, plus one `del` row with a deliberately wrong HGVS. Exit 0. Stdout has `substitution\t1\t1`, `del\t1\t0`, `dup\t1\t1` and `m\t1\t1`, and the mismatch TSV has exactly one data row.
- [ ] **Step 2:** Run `cargo test -p clingen-index`. Expected: FAIL.
- [ ] **Step 3: Implement with `clap` derive.**
  - Reading chunks: split on `\t`; skip rows with an empty or non-`CA` column 5; read `HGVS_GRCh38` from column 6.
  - `vareffect_version` in the manifest comes from a `const` set by `build.rs` from `cargo metadata`, or, simpler, is hardcoded to match the `Cargo.toml` requirement `"0.6"`.
- [ ] **Step 4:** Run `cargo test --workspace`. Expected: PASS.
- [ ] **Step 5: Commit.** `git commit -m "feat(clingen-index): build and validate subcommands"`

### Task 6: chr21 conformance and representation invariance

Run this once the chr21 harvest (`logs/harvest_chr21.log`) is complete.

**Files:**
- Create: `scripts/build_chr21_genome.sh`, `scripts/sample_harvest.py`, `crates/clingen-allele/tests/data/chr21_sample.tsv`, `crates/clingen-allele/tests/conformance.rs`, `docs/known-differences.md`

**Interfaces:**
- Consumes: `canonical_hgvs`, the Task 5 CLI, and the harvest in `data/chr21_chunks/`.
- Produces:
  - `scripts/build_chr21_genome.sh <out_dir>`, which writes `<out_dir>/chr21.bin` and `<out_dir>/chr21.bin.idx`.
  - The env var `CLINGEN_CHR21_GENOME=<path to chr21.bin>`. Tests that need it skip with a printed notice when it is unset.

- [ ] **Step 1: Write `build_chr21_genome.sh`.**
  - It downloads `https://hgdownload.soe.ucsc.edu/goldenPath/hg38/chromosomes/chr21.fa.gz` into its own new directory.
  - It runs a small helper binary, `crates/clingen-allele/examples/fasta_to_bin.rs`, which calls `write_genome_binary` with contig name `chr21` and build `GRCh38`.

  Expected: `ls -l` shows `chr21.bin` at about 46.7 MB.
- [ ] **Step 2: Run the full validation.** `cargo run -p clingen-index --release -- validate --chunks data/chr21_chunks --genome <out>/chr21.bin --mismatches data/chr21_mismatches.tsv`.

  Expected: `substitution` at 100%. Record every class's numbers in `docs/known-differences.md`.
- [ ] **Step 3: Resolve the mismatches.**
  - For each indel mismatch pattern: if it is fixable in the `canonical_hgvs` post-processing (Task 2 step 5), add a failing row to `hgvs_examples.rs`, fix it, and commit.
  - Otherwise, document the pattern with 3 example rows in `docs/known-differences.md`.
  - Repeat until each pattern is fixed or documented.
- [ ] **Step 4: Write `sample_harvest.py`.** It draws a deterministic sample of 10,000 registered rows (seed 21), at least 2,000 of them indels, into `tests/data/chr21_sample.tsv`.
- [ ] **Step 5: Write `conformance.rs` with two tests.**
  - `sample_matches_registry`: every sample row's `canonical_hgvs` equals column 6, except rows listed in `docs/known-differences.md`. The known-differences file has a fenced block of `CHROM POS REF ALT` lines that the test reads.
  - `representation_invariance`: for each indel row, build three variants and assert each gives the same HGVS as the original:
    - left-padded: prepend the reference base before POS and set POS−1;
    - right-padded: append the reference base after the REF end;
    - fully left-shifted, using a local left-align loop over `genome.fetch_base`.
- [ ] **Step 6:** Run `CLINGEN_CHR21_GENOME=<out>/chr21.bin cargo test -p clingen-allele --test conformance --release`. Expected: PASS. Without the env var: both tests print `skipped: CLINGEN_CHR21_GENOME unset` and pass.
- [ ] **Step 7: Build the real chr21 index.**
  - Run `cargo run -p clingen-index --release -- build --chunks data/chr21_chunks --out data/index_chr21 --source gnomad41_genomes_chr21 --genome <full GRCh38.bin path>`.
  - Record `entries` and the size of `caid.fst` in `README.md`.
- [ ] **Step 8: Commit.** `git commit -m "test: chr21 Registry conformance and representation invariance"` (data under `data/` stays gitignored).

### Task 7: vep-rs adapter

**Files** (fork `natera-open-source/vep-rs` → branch `clingen-allele-plugin`):
- Create: `crates/vep-builtins/src/plugins/clingen.rs`, `crates/vep-cli/tests/clingen_plugin.rs`
- Modify: `crates/vep-builtins/Cargo.toml`, `crates/vep-builtins/src/plugins/mod.rs`, `crates/vep-builtins/src/registry.rs:332-347`, `crates/vep-cli/Cargo.toml`, the plugin docs next to `docs/developers/writing-a-plugin.md`

**Interfaces:**
- Consumes: `clingen_allele::{Annotator, Lookup, Status}` (Task 4), as a git dependency on this repo, or a path dependency during development.
- Produces: `pub struct ClinGenPlugin`, implementing `BuiltinPlugin`:
  - `name()` → `"ClinGen"`.
  - `header_info()` → `[("ClinGen_CAID", "ClinGen Allele Registry canonical allele ID"), ("ClinGen_HGVSg", "ClinGen canonical genomic HGVS")]`.
  - `init(params)` parses `index=DIR` and `genome=PATH`. `genome` is optional when the manifest records one. Errors are `PluginError` with a message naming the missing parameter.
  - `set_assembly(Some(a))`: anything other than GRCh38 makes `init` return an error.
  - `prefetch` and `annotate` work like `cadd.rs:218-285`. `annotate` sets `variant.plugin_data["ClinGen_CAID"]` and `["ClinGen_HGVSg"]` only when the value is `Some`.
- Allele reconstruction: VCF-form `(pos1, ref, alt)` comes from `InputVariant.original_start` and `original_allele_string` (`vep-core/src/variant.rs:178-182`), split on `/` into REF and each ALT.
  - When those are `None` (non-VCF input formats), fall back to `start`/`ref_allele`/`alt_allele()`, mapping `-` to an anchored form by reading one base at `start - 1` from the genome.
  - Add `pub fn base_at(&self, chrom: &str, pos1: u64) -> Option<u8>` to `Annotator` for that.

- [ ] **Step 1: Fork and branch.** Before creating the fork, ask the user; it is an outward-facing action. Then `git checkout -b clingen-allele-plugin`.
- [ ] **Step 2: Write the failing unit tests in `clingen.rs` `#[cfg(test)]`.** They use `clingen_allele::test_support` for the tiny genome and an index built as in Task 4.
  - `annotates_dup_any_representation`: two `InputVariant`s for `21:33000166 T/TA` and `21:33000172 A/AA` → both `plugin_data["ClinGen_CAID"] == "CA42"`.
  - `missing_index_param_errors`: `init(&[])` → `Err`, with a message containing `index=`.
  - `rejects_grch37`: `set_assembly(Some(Assembly::GRCh37))`, then `init` → `Err`.
- [ ] **Step 3:** Run `cargo test -p vep-builtins --features clingen clingen`. Expected: FAIL.
- [ ] **Step 4: Implement the plugin, and register it.** Add `#[cfg(feature = "clingen")] "ClinGen" => Some(Box::new(plugins::clingen::ClinGenPlugin::new())),` to `create_builtin`. At the end of the run, print `Annotator::stats()` to stderr from the plugin's `Drop`.
- [ ] **Step 5: Write the integration test `crates/vep-cli/tests/clingen_plugin.rs` (`#![cfg(feature = "clingen")]`).**
  - Run the CLI as `common/mod.rs` does, with `--vcf --plugin ClinGen,index=…,genome=…` on a 4-line VCF made of the Task 4 variants.
  - Assert the header CSQ `Format:` ends with `|ClinGen_CAID|ClinGen_HGVSg`, and the dup line's CSQ contains `CA42|NC_000021.9:g.33000172dup`.
  - Assert a run without `--features clingen` reports `ClinGen` as an unknown plugin. Check this by hand; no test.
- [ ] **Step 6:** Run `cargo test --workspace --features vep-cli/clingen`, `cargo test --workspace` (the default build must be unaffected), `cargo fmt --check` and `cargo clippy --workspace --features vep-cli/clingen -- -D warnings`. Expected: all pass.
- [ ] **Step 7:** Document the plugin, with its parameters and fields, following the existing plugin docs. Then commit: `git commit -m "feat(builtins): ClinGen Allele Registry plugin (feature clingen)"`

### Task 8: fastVEP adapter

**Files** (fork `Huang-lab/fastVEP` → branch `clingen-allele-plugin`):
- Create: `crates/fastvep-annotate/src/clingen.rs`, `crates/fastvep-annotate/tests/clingen_provider.rs`
- Modify: `crates/fastvep-annotate/Cargo.toml`, `crates/fastvep-annotate/src/lib.rs:1627-1650`, `crates/fastvep-io/src/output.rs` (after `DBSNP_FIELDS` at line 603, and in `VCF_PROJECTION_SPECS` at 633+), `crates/fastvep-cli/src/main.rs` (flag near `--sa-dir` at 121-129), `crates/fastvep-cli/Cargo.toml`, `docs/SUPPLEMENTARY_ANNOTATIONS.md:19-41,279-295`

**Interfaces:**
- Consumes: `clingen_allele::{Annotator, Status}`.
- Produces:
  - `pub struct ClinGenProvider`, implementing `AnnotationProvider`:
    - `name()` → `"ClinGen"`, `json_key()` → `"clingen"`.
    - `metadata()`: `match_by_allele: true`, `is_array: false`, `is_positional: false`, `assembly: "GRCh38"`.
    - `annotate_position(chrom, pos, ref, alt)` → `Some(AnnotationValue::Json(r#"{"caid":"CA…","hgvsg":"NC_…"}"#))`, omitting `caid` when it is `None`; `Ok(None)` when there is no HGVS.
  - `pub fn open_clingen_dir(dir: &Path, genome_override: Option<&Path>) -> Result<ClinGenProvider>`.
  - In `fastvep-io/src/output.rs`: `const CLINGEN_FIELDS: &[(&str, &str)] = &[("CAID", "caid"), ("HGVSG", "hgvsg")];` and the spec entry `{ json_key: "clingen", info_id: "FV_CLINGEN", description: "fastVEP ClinGen Allele Registry annotations. Format: ALLELE|CAID|HGVSG", fields: CLINGEN_FIELDS, kind: VcfProjectionKind::AlleleObject }`. This module stays feature-independent, so the projection exists in every build and is simply unused without the provider.
  - CLI: `--clingen-genome <PATH>`. `load_sa_providers` takes an extra `clingen_genome: Option<&Path>` argument behind `#[cfg(feature = "clingen")]`, or reads it from a `OnceLock` set by the CLI. Prefer the argument, and update the callers.

- [ ] **Step 1: Fork and branch.** Ask the user first, as in Task 7.
- [ ] **Step 2: Check the shared-library path first.** Write the test `library_path_passes_vcf_alleles` in `tests/clingen_provider.rs`.
  - It uses a recording provider that stores the `(pos, ref, alt)` it receives, and runs `21:33000166 T TA` through the `fastvep-annotate` library entry point that serves the web path (`lib.rs:790-833`).
  - It asserts the provider received `(33000166, "T", "TA")`.
  - If this fails (the earlier exploration suspected trimmed alleles there), `ClinGenProvider` must rebuild VCF form. Add `Annotator::base_at`, as in Task 7, and handle a `-` REF or ALT by anchoring at `pos - 1`.
- [ ] **Step 3: Write the failing provider tests.**
  - `provider_found`: the tiny genome and index from Task 4; `annotate_position("21", 33000172, "A", "AA")` → JSON containing `"caid":"CA42"`.
  - `provider_hgvs_only`: `("21", 33000166, "TA", "T")` → JSON containing `"hgvsg":"NC_000021.9:g.33000172del"`, without `caid`.
  - `sa_dir_loads_clingen`: a temp `--sa-dir` with a subdirectory holding `manifest.json` and `caid.fst` → `load_sa_providers` returns a provider whose `json_key() == "clingen"`.
  - `vcf_info_projection`: in `fastvep-io` tests, a `clingen` supplementary payload produces the header `##INFO=<ID=FV_CLINGEN,` and the value `TA|CA42|NC_000021.9:g.33000172dup` for allele `TA`. Use the allele formatting rules of the existing dbSNP projection tests.
- [ ] **Step 4:** Run `cargo test --workspace --features fastvep-cli/clingen clingen`. Expected: FAIL.
- [ ] **Step 5: Implement.**
  - Add the provider, the directory detection (a subdirectory of `sa_dir` containing `manifest.json` whose JSON has `"format_version"` and `"assembly"`), the CLI flag, the projection entry, and the docs row and pipe format.
  - Print `Annotator::stats()` to stderr when the provider is dropped.
- [ ] **Step 6:** Run `cargo fmt --check`, `cargo clippy --workspace --all-targets --features fastvep-cli/clingen -- -D warnings`, `cargo test --workspace` and `cargo test --workspace --features fastvep-cli/clingen`. Expected: all pass, including the INFO-field documentation check (`SUPPLEMENTARY_ANNOTATIONS.md`, lines 382-397).
- [ ] **Step 7: Commit.** `git commit -m "feat: ClinGen Allele Registry provider (feature clingen)"`

### Task 9: End-to-end cross-check and README

**Files:**
- Create: `scripts/e2e_crosscheck.py`
- Modify: `README.md`

**Interfaces:**
- Consumes: the chr21 index (Task 6), both adapters (Tasks 7–8) and the harvest TSVs.
- Produces: `scripts/e2e_crosscheck.py --harvest data/chr21_chunks --vcf-out <tool output VCF> --tool vep-rs|fastvep`, which prints `checked=N agree=N disagree=N missing=N` and exits 1 if `disagree > 0` or `missing > 0`.

- [ ] **Step 1: Write the script.**
  - It reads the harvest into a dict `(pos, ref, alt) → CAID`.
  - It parses each tool's VCF: for vep-rs, the CSQ subfield `ClinGen_CAID`, located by the CSQ `Format:` header; for fastVEP, INFO `FV_CLINGEN`, second pipe field, per ALT.
  - It compares only rows where the harvest has a CA ID.
- [ ] **Step 2: Make the input.** Take the first 100,000 harvested sites as a VCF. Run each tool with network blocked: on macOS, `sandbox-exec -p '(version 1)(allow default)(deny network*)' <cmd>`.
- [ ] **Step 3: Run the check.** Run `python3 -I scripts/e2e_crosscheck.py …` for each tool. Expected: `disagree=0 missing=0`, except rows listed in `docs/known-differences.md` (the script reads that file's block, as the conformance test does).
- [ ] **Step 4: Write the README.**
  - Harvesting a chromosome (the existing scripts and their resume behavior).
  - `clingen-index build` and `validate`.
  - The vareffect genome binary, built with `vareffect-cli`, about 3.1 GB, a one-time step.
  - Enabling each adapter: the feature flag and CLI usage.
  - The chr21 index size and entry count, the known differences, and the measured annotation throughput for each tool on the 100k set.
- [ ] **Step 5: Commit.** `git commit -m "docs: README and end-to-end cross-check"`
