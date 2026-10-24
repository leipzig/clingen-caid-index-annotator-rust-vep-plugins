# Index Regeneration Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Nextflow pipeline (local / SLURM / AWS Batch) that regenerates the ClinGen CA-ID index from Registry region enumeration over gene/exome windows plus ClinVar (and optionally gnomAD), and proves every key reachable before publishing.

**Architecture:** All logic that needs testing lives in new `clingen-index` subcommands (`genome`, `build --sorted`, `reach`, `regions`, `registry enumerate|lookup`, `validate --strict`) on top of small `clingen-allele` API additions. The pipeline is plumbing: each process runs one subcommand or a short bash/coreutils pipeline, inside one Docker image.

**Tech Stack:** Rust 1.94.1 (edition 2024; vareffect =0.6.0, fst, ureq 3, flate2), Nextflow 26.04.7 (strict syntax), nf-schema 3.0.0, nf-test 0.9.5, Docker / Apptainer / AWS Batch, CloudFormation.

**Spec:** `docs/superpowers/specs/2026-10-24-index-regeneration-pipeline-design.md`

**Reference (verified facts and snippets, not yet executed):** `docs/superpowers/plans/2026-10-24-index-regeneration-pipeline-reference.md`. Sections: *Pins and versions*, *Rust HTTP client*, *nf-schema and nf-test*, *SLURM executor protocol and shims*, *AWS Batch*. Tasks cite snippets by their `####` title.

## Global Constraints

- Rust: `rust-version = "1.94.1"`, edition 2024, `vareffect = "=0.6.0"` (unchanged). After every Rust task: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` all clean.
- `clingen-allele` gains no network or compression dependency. New dependencies go into `clingen-index` only: `ureq = "=3.4.3"` (default features: rustls + ring + webpki-roots + gzip), `flate2 = "1.1.10"`, `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"`.
- Version pins are releases at least two weeks old (on or before 2026-10-10), **except security fixes**: `ureq 3.4.3`, `rustls 0.23.46`, `rustls-webpki 0.103.16`, `webpki-roots 1.0.11` (they close RUSTSEC-2026-0346…0349, 0381, 0382; see Reference › Rust HTTP › *Security conflict*). Any other crate the new dependencies pull in that is newer than 2026-10-10 is downgraded with `cargo update -p <crate> --precise <ver>` where compatible; record the final versions in the task report. Commit `Cargo.lock`.
- Other pins: Nextflow `26.04.7` (`manifest.nextflowVersion = '!>=26.04.7'`), nf-schema `3.0.0` (exact), nf-test `0.9.5`, openjdk `21.0.10` (conda-forge), cfn-lint `1.57.2`, AWS CLI v2 `2.37.12` (pinned installer URL + SHA-256 from Reference), base images `rust:1.94.1-bookworm` and `debian:bookworm-20261005-slim`, each by digest (Reference › Pins and versions).
- Registry etiquette: read-only `GET /alleles?refseq=…` and `POST /alleles?file=vcf` only, never `PUT`; `User-Agent: clingen-index/<crate version>`; one stream per Registry process (`maxForks 1`); client retries network errors, 5xx and 429 with exponential backoff, 6 attempts, 30 s doubling; any other 4xx fails at once.
- `clingen-index` exit codes: 0 success; 1 a check failed (`validate --strict` mismatch, `reach` unreachable key); 2 the run could not complete.
- Default test runs are offline: `cargo test` and `nf-test test` (without `--tag network`) never touch the network. Live-Registry tests are `#[ignore]` Rust tests run with `CLINGEN_LIVE_REGISTRY=1 cargo test -- --ignored`, and nf-test tests tagged `network`.
- Pipeline: Nextflow DSL2 in strict syntax (the 26.04 default parser). No Python anywhere in the pipeline. Processes run `clingen-index` or bash with coreutils, gawk, gzip, curl, openssl, split, join, sort. Do not use the deprecated `splitText`/`splitCsv`: batching is done by `clingen-index regions --batch` and coreutils `split`. Every process has a `stub:` block.
- Never commit `data/`. Add `.nf-test/`, `pipeline/work/`, `.nextflow*`, `results/` to `.gitignore`. Commit test fixtures (small) under `pipeline/tests/data/` and `crates/*/tests/`.
- Commit messages: `type(scope): subject`, ending with the committing agent's own `Co-Authored-By` trailer.
- Match the repo's style: module `//!` docs, `///` on every public item, doc comments explaining *why*, error messages naming the file/line/value at fault.
- Docs claims are measured; command output pasted into docs is pasted verbatim from a run.

## Review Focus

1. **A full or broken Registry page.** A window whose page holds exactly `--page-size` alleles must be followed by the next page; a truncated, non-JSON or HTML body must be retried and then fail the task, and in no case may `registry enumerate`/`lookup` leave a partial output file. → Task 7 tests `enumerate_pages_until_a_short_page`, `enumerate_fails_on_truncated_json_after_six_attempts`, `enumerate_fails_on_400_and_writes_nothing`.
2. **Byte order across shards and prefix keys.** Keys reach `build --sorted` in FST byte order only if every shard is sorted with `LC_ALL=C` and shards are concatenated in accession order (`NC_000009.12` < `NC_000010.11` < … < `NC_000023.11` (X) < `NC_000024.10` (Y) < `NC_012920.1` (MT)); `…g.12del` must precede `…g.12delinsA`. A locale-sorted shard must be refused with file and line. → Task 4 tests `sorted_build_rejects_locale_order`, `sorted_build_across_files_in_accession_order`; Task 2 `sorted_writer_accepts_prefix_keys`.
3. **Registry HGVS that must not enter the index.** Reference alleles (`…=`), other accessions for the same allele (`CM000683.2:…`, GRCh37 `NC_000021.8:…`), and `g.` on chrM / `m.` elsewhere. Enumeration keeps only the window's GRCh38 accession and drops `=`; `reach` reports forms our tools never write. → Task 7 `enumerate_keeps_only_the_windows_grch38_hgvs`; Task 5 `hgvs_vcf` unit tests.
4. **`--chrom` forms.** `21`, `chr21`, `X`, `chrX`, `MT`, `chrM`, `all` accepted (nf-schema casts a bare `21` to Integer, so the schema type is `["string","integer"]`); `23`, `chrUn`, `M` refused; an `MT` run with gnomAD uses v3.1's chrM file and no exomes. → Task 9 `main.nf.test` cases; Task 10 `RESOLVE_GNOMAD` test.
5. **Retry and resume idempotence.** A retried task must produce complete outputs or none (write `*.partial`, rename at the end); a gnomAD stream with a wrong md5 fails its task; `-resume` re-runs only unfinished batches. → Task 7 atomic-output tests; Task 10 `GNOMAD_SITES md5 mismatch fails`; Task 13 resume check.

---

### Task 1: Split `clingen-index` into modules; `validate --strict`

`crates/clingen-index/src/main.rs` (546 lines) will gain six subcommands, so first split it with no behaviour change, then add `--strict`.

**Files:**
- Modify: `crates/clingen-index/src/main.rs` (CLI and dispatch only)
- Create: `crates/clingen-index/src/chunks.rs` (`Row`, `Verdict`, `Malformed`, `chunk_files`, `ReadSummary`, `read_rows`, moved verbatim)
- Create: `crates/clingen-index/src/build.rs` (`BuildArgs` handling, `manifest_genome`, `build`)
- Create: `crates/clingen-index/src/validate.rs` (`Class`, `Tally`, `ours`, `validate`)
- Test: `crates/clingen-index/tests/cli.rs`

**Interfaces:**
- Produces: `pub(crate) fn read_rows(files: &[PathBuf], visit: impl FnMut(&Row) -> Result<Verdict>) -> Result<ReadSummary>`, `pub(crate) fn chunk_files(dirs: &[PathBuf]) -> Result<Vec<PathBuf>>`, `pub(crate) enum Class` with `Class::of(hgvs: &str) -> Class` and `name()`; `const EXIT_FAILURE: u8 = 2` stays in `main.rs` and is `pub(crate)`.
- `ValidateArgs` gains `#[arg(long)] strict: bool`: with it, exit 1 when any class has a mismatch; without it, unchanged (exit 1 only for substitution mismatches).

- [ ] **Step 1: Move code into the modules** without editing bodies; `cargo test -p clingen-index` passes with all 18 existing tests unchanged.
- [ ] **Step 2: Write failing test** `validate_strict_exits_1_on_an_indel_mismatch` in `tests/cli.rs`: chunk row `21\t33000166\tT\tTA\tCA100\tNC_000021.9:g.33000171dup\t` on `tiny_genome()`; `validate` without `--strict` exits 0 and stdout's `dup` row reads `dup\t1\t0\t0.0`; with `--strict` exits 1; the mismatches TSV holds the row with `OURS` = `NC_000021.9:g.33000172dup`.
- [ ] **Step 3:** Run `cargo test -p clingen-index --test cli validate_strict` → FAIL (`--strict` unknown, exit 2).
- [ ] **Step 4: Implement `--strict`** (`Tally::any_mismatches()`).
- [ ] **Step 5:** Full Rust gate (Global Constraints) passes.
- [ ] **Step 6: Commit** `refactor(clingen-index): split into modules; feat: validate --strict`.

### Task 2: `clingen-allele` API for streaming builds, key walks and reference spans

**Files:**
- Modify: `crates/clingen-allele/src/index.rs`, `src/lib.rs`, `src/contig.rs`, `src/annotator.rs`
- Test: `crates/clingen-allele/tests/index_roundtrip.rs`, `tests/annotator.rs`

**Interfaces (Produces):**
- `pub fn primary_contigs() -> &'static [Contig]` (was `pub(crate)`), re-exported from `lib.rs`.
- `pub struct SortedIndexWriter` in `index.rs`, re-exported:
  - `pub fn create(dir: &Path) -> Result<Self, Error>` — creates `dir` if needed and opens `caid.fst.tmp` (via `create_tmp`).
  - `pub fn push(&mut self, hgvs: &str, caid: u64) -> Result<bool, Error>` — `Ok(true)` added; `Ok(false)` exact repeat of the previous pair, skipped; `Err(Error::DuplicateConflict { hgvs, first: <previous id>, second: <this id> })` same key, other id; `Err(Error::Unsorted { previous, key })` key byte-less than the previous one.
  - `pub fn finish(self, manifest: Manifest) -> Result<u64, Error>` — sets `entries`/`format_version`, writes `manifest.json.tmp`, renames `caid.fst` then `manifest.json` with directory syncs exactly as `IndexBuilder::finish` does today.
  - Dropping a writer that was not finished, or any error, removes both `.tmp` files and leaves an existing index untouched.
- New `Error::Unsorted { previous: String, key: String }`, message `keys out of order: {key} after {previous}`.
- `IndexBuilder::finish` keeps its signature and behaviour and is reimplemented as `dedup_sorted` + `SortedIndexWriter` (one write path).
- `pub fn CaIndex::visit_prefix(&self, prefix: &[u8], visit: impl FnMut(&[u8], u64) -> ControlFlow<()>)` — every key starting with `prefix`, in byte order (`&[]` = all keys); `Break` stops.
- `pub fn Annotator::ref_seq(&self, chrom: &str, start1: u64, end1: u64) -> Option<Vec<u8>>` — uppercase bases `start1..=end1` (1-based, inclusive) via `FastaReader::fetch_sequence(contig.vareffect_name, start1 - 1, end1)`; `None` for `start1 == 0`, `start1 > end1`, an unresolvable or absent contig, or `end1` past the contig end.

- [ ] **Step 1: Write failing tests**
  - `index_roundtrip.rs`: `sorted_writer_matches_index_builder_bytes` (the 1000-key fixture the file already uses, pushed in order, gives a byte-identical `caid.fst` and equal manifest to `IndexBuilder`); `sorted_writer_skips_exact_repeats` (push `(k,1),(k,1)` → second returns `Ok(false)`, entries 1); `sorted_writer_rejects_conflict` (`(k,1),(k,2)` → `DuplicateConflict{first:1, second:2}`; an index already in `dir` still opens with its old entries; no `.tmp` left); `sorted_writer_rejects_out_of_order` (`"b"` then `"a"` → `Unsorted{previous:"b", key:"a"}`); `sorted_writer_accepts_prefix_keys` (`NC_000021.9:g.12del` then `NC_000021.9:g.12delinsA` both added); `sorted_writer_dropped_without_finish_leaves_old_index`; `visit_prefix_walks_in_byte_order` (keys on NC_000001.11, NC_000021.9, NC_012920.1: prefix `NC_000021.9:` yields only those, in order; `&[]` yields all; `Break` after the first stops at one).
  - `annotator.rs`: `ref_seq_returns_inclusive_span` (`ref_seq("21", 33000166, 33000172) == Some(b"TAAAAAA".to_vec())`, `ref_seq("MT", 3243, 3243) == Some(b"A".to_vec())`); `ref_seq_none_cases` (`start1` 0; `start1 > end1`; contig `foo`; `end1` = `CHR21_LEN + 1`).
- [ ] **Step 2:** `cargo test -p clingen-allele` → FAIL to compile.
- [ ] **Step 3: Implement** the items above; existing tests stay green unchanged.
- [ ] **Step 4:** Full Rust gate passes; `vep-rs` and `fastVEP` still build with their `clingen` features (`cargo build -p vep-builtins --features clingen` in `../vep-rs`, `cargo build -p fastvep-annotate --features clingen` in `../fastVEP`).
- [ ] **Step 5: Commit** `feat(clingen-allele): SortedIndexWriter, CaIndex::visit_prefix, Annotator::ref_seq`.

### Task 3: `clingen-index genome`

**Files:**
- Create: `crates/clingen-index/src/genome.rs`; modify `src/main.rs`, `crates/clingen-index/Cargo.toml` (add `flate2`)
- Delete: `crates/clingen-allele/examples/fasta_to_bin.rs`
- Modify: `scripts/build_chr21_genome.sh` (call `clingen-index genome`)
- Test: `crates/clingen-index/tests/genome.rs`

**Interfaces:**
- CLI: `genome --fasta <PATH|-> [--contigs <LIST>] --out <BIN>`. `<LIST>` is comma-separated names in any form `resolve_contig` accepts (`21,chrX,MT,NC_000022.11`); default all 25.
- Input plain or gzip/BGZF (detect magic `1f 8b`; read with `flate2::read::MultiGzDecoder` — `GzDecoder` stops after the first BGZF member).
- A record is kept when the first word of its header resolves to a requested primary contig (UCSC `chr21`, Ensembl `21`, `MT`, an `NC_` accession). It is written under the contig's `vareffect_name` (`chr21`, `chrM`), in FASTA order, with build `GRCh38`.
- Each written contig's length must equal `Contig::length`, else exit 2: `contig chr21 is 1000 bases long; GRCh38 chr21 is 46709983`. A requested contig missing, or seen twice, is exit 2. Output goes to `<out>.tmp` and `<out>.idx.tmp`, renamed to `<out>` then `<out>.idx` only on success.
- stderr summary: `contigs=<n> bases=<total>`.

- [ ] **Step 1: Write failing tests** in `tests/genome.rs` (fixture FASTA built in the test: `N` × GRCh38 length with the `test_support::windows()` pasted in, like `tiny_genome`):
  - `genome_writes_requested_contigs`: `--contigs 21,MT` → `FastaReader` has `chrom_length("chr21") == 46709983`, `chrom_length("chrM") == 16569`, `fetch_base("chr21", 5030577) == b'C'`; stderr `contigs=2 bases=46726552`.
  - `genome_reads_gzip_from_stdin` (same FASTA gzipped, `--fasta -`), identical `.bin` bytes to the plain run.
  - `genome_refuses_a_wrong_length` (chr21 of 1000 bases) → exit 2, message above, no `<out>` or `.tmp` files.
  - `genome_missing_or_duplicate_contig_exits_2`.
  - `genome_skips_unrequested_and_non_primary_records` (`chr21_KI270872v1_alt`, `chrUn_…`, unrequested `chr22`).
  - `genome_accepts_ensembl_headers` (`>21 dna:chromosome chromosome:GRCh38:21:1:46709983:1 REF`).
- [ ] **Step 2:** `cargo test -p clingen-index --test genome` → FAIL.
- [ ] **Step 3: Implement**; delete the example; switch `scripts/build_chr21_genome.sh` to `gunzip -c … | cargo run --release -q -p clingen-index -- genome --fasta - --contigs chr21 --out "$out_dir/chr21.bin"`.
- [ ] **Step 4: Real-data check** (record in the report): `scripts/build_chr21_genome.sh <tmp>` gives `chr21.bin` and `chr21.bin.idx` byte-identical (`cmp`) to `data/genome_chr21/`; `gunzip -c data/genome_grch38/download/hg38.fa.gz | target/release/clingen-index genome --fasta - --out <tmp>/GRCh38.bin` gives files byte-identical to `data/genome_grch38/GRCh38.bin{,.idx}` (built earlier by a throwaway converter writing the same 25 contigs in FASTA order) and prints `contigs=25 bases=3088286401`.
- [ ] **Step 5:** Full Rust gate passes.
- [ ] **Step 6: Commit** `feat(clingen-index): genome subcommand builds a GRCh38 vareffect genome`.

### Task 4: `build --sorted` and `--genome-relative`

**Files:**
- Modify: `crates/clingen-index/src/build.rs`, `src/main.rs`
- Test: `crates/clingen-index/tests/build_sorted.rs`

**Interfaces:**
- `build` takes exactly one of `--chunks <DIR>...` (unchanged) and `--sorted <FILE>...` (clap `ArgGroup`, required, exclusive).
- `--sorted` files: one `HGVS\tCAID` per line (`CAID` = `CA<digits>`), plain or gzip; blank lines skipped. Any other line → exit 2 `malformed pair at FILE:LINE`. Pairs stream through `SortedIndexWriter` in file order; `Error::Unsorted` → exit 2 `keys out of order at FILE:LINE: KEY after PREVIOUS`; conflicts → exit 2 as today. Memory is constant.
- stderr summary for `--sorted`: `pairs=<n> repeats=<n> entries=<n>`.
- `--genome-relative` (requires `--genome`): the manifest records the genome relative to `--out` (both made canonical first; e.g. `out=/r/index`, genome `/r/genome/GRCh38.bin` → `../genome/GRCh38.bin`). `Annotator::genome_path_from_manifest` already resolves relative paths against the index directory.

- [ ] **Step 1: Write failing tests**
  - `sorted_build_equals_chunk_build`: the rows of `build_from_chunks` (SNV, DUP, MITO) as sorted pairs give a byte-identical `caid.fst` and the same `entries`.
  - `sorted_build_across_files_in_accession_order`: files `NC_000009.12.tsv`, `NC_000010.11.tsv`, `NC_012920.1.tsv` in that order succeed; passing them reversed exits 2 naming the second file and line 1.
  - `sorted_build_skips_exact_repeats_and_rejects_conflicts` (`pairs=3 repeats=1 entries=2`; a conflicting id exits 2, old index intact).
  - `sorted_build_rejects_locale_order`: lines `NC_000021.9:g.10A>C\tCA1` then `NC_000021.9:g.100A>C\tCA2` (natural order; in byte order `100A` < `10A`) → exit 2 `keys out of order at …:2`.
  - `sorted_build_malformed_pair_exits_2` (no tab; `CA` without digits; empty HGVS).
  - `genome_relative_is_recorded_relative_to_out` → manifest `genome` = `../genome/GRCh38.bin` and `genome_path_from_manifest` returns an existing file; `--genome-relative` without `--genome` exits 2.
  - `chunks_and_sorted_are_exclusive` → exit 2.
- [ ] **Step 2:** `cargo test -p clingen-index --test build_sorted` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Real-data check**: `cat data/chr21_chunks/chunk_*.tsv data/clinvar/20261019/chunks/chunk_*.tsv | awk -F'\t' '$5 ~ /^CA/ {print $6 "\t" $5}' | LC_ALL=C sort -u > <tmp>/pairs.tsv`; `/usr/bin/time -l target/release/clingen-index build --sorted <tmp>/pairs.tsv --out <tmp>/idx` → `entries=15528839` and `caid.fst` byte-identical to `data/index_corpus/caid.fst` (182,447,000 bytes); report peak RSS (expected far below the 1,045,839,872 B of the in-memory build).
- [ ] **Step 5:** Full Rust gate passes.
- [ ] **Step 6: Commit** `feat(clingen-index): build --sorted streams pre-sorted pairs; --genome-relative`.

### Task 5: HGVS → VCF and `reach`

**Files:**
- Create: `crates/clingen-index/src/hgvs_vcf.rs` (unit tests inside), `src/reach.rs`; move `Class` to `src/hgvs_class.rs` (shared by `validate` and `reach`)
- Test: `crates/clingen-index/tests/reach.rs`

**Interfaces:**
- `pub(crate) struct VcfAllele { chrom: &'static str /* Contig::vareffect_name, e.g. "chr21", "chrM"; Annotator::lookup accepts it */, pos1: u64, ref_allele: Vec<u8>, alt_allele: Vec<u8> }`
- `pub(crate) enum Parsed { Vcf(VcfAllele), Reference, Unparseable(&'static str) }`
- `pub(crate) fn hgvs_to_vcf(hgvs: &str, ref_seq: impl Fn(&str, u64, u64) -> Option<Vec<u8>>) -> Parsed` (`ref_seq(chrom, start1, end1)`, inclusive, as `Annotator::ref_seq`).
- CLI: `reach --index <DIR> [--genome <BIN>] [--prefix <P>] [--examples <N=5>]`. The genome defaults to the manifest's (`Annotator::genome_path_from_manifest`); neither → exit 2.

**Conversion rules** (accession resolved with `resolve_contig`; `g.` required for nuclear accessions, `m.` for `NC_012920.1`; positions 1-based; `S(a,b)` = `ref_seq(chrom,a,b)`; a span longer than 10,000,000 bp is `Unparseable("span too long")`):

```
<p><R>>(<A>)        R, A single ACGT        → pos p, REF R, ALT A
<s>[_<e>]del        s ≥ 2                   → pos s-1, REF S(s-1,e), ALT S(s-1,s-1)
<s>[_<e>]del        s = 1                   → pos 1,   REF S(1,e+1), ALT S(e+1,e+1)
<s>[_<e>]dup                                → pos e,   REF S(e,e),   ALT S(e,e) ++ S(s,e)
<s>_<e>ins<SEQ>     e = s+1, SEQ ACGT+      → pos s,   REF S(s,s),   ALT S(s,s) ++ SEQ
<s>[_<e>]delins<SEQ> SEQ ACGT+              → pos s,   REF S(s,e),   ALT SEQ
…=                                          → Reference
anything else (inv, con, ins(10), [n], ?, unknown accession, g./m. mismatch, S(...) None) → Unparseable(<reason>)
```

**`reach` outcome per key**, after `Annotator::lookup(chrom, pos1, REF, ALT)`: `reachable` (Found, same id), `wrong_ca` (Found, other id), `not_found` (NotInIndex: our HGVS differs; stderr example shows it), `ref_mismatch`, `unsupported`, `reference`, `unparseable`. stdout: one `outcome\tcount` line each, after `keys\t<n>`, then a per-class table (`Class`: substitution, del, dup, ins, delins, m, other) of `class\tkeys\treachable`. Exit 1 if `wrong_ca + not_found + ref_mismatch + unsupported > 0`; `reference`/`unparseable` are reported, not failed.

- [ ] **Step 1: Write failing unit tests** in `hgvs_vcf.rs` with a closure genome over the tiny-genome windows: one per rule row, including `del` at position 1 (right anchor), `m.` on `NC_000021.9` → Unparseable, `g.` on `NC_012920.1` → Unparseable, `NC_000021.8:g.…` → Unparseable, `ins` with `e ≠ s+1` → Unparseable, `33000167_33000168ins(10)` → Unparseable, `…5030578=` → Reference.
- [ ] **Step 2: Write failing CLI tests** in `tests/reach.rs` (tiny genome; index built with `build --sorted`):
  - `every_form_on_the_tiny_genome_is_reachable`: keys `NC_000021.9:g.5030578C>T`→749862939, `g.33000172del`→1, `g.33000172dup`→42, `g.33000126_33000127dup`→2, `g.33000167_33000168insGA`→3, `g.33000017_33000018delinsTG`→4, `NC_012920.1:m.3243A>G`→5 → `reachable\t7`, exit 0.
  - `a_key_written_differently_is_not_found`: key `NC_000021.9:g.33000171dup`→43 alone → `not_found\t1`, exit 1, stderr shows `ours=NC_000021.9:g.33000172dup`.
  - `wrong_ca_is_reported`: keys `…g.33000171dup`→43 and `…g.33000172dup`→42 → `wrong_ca\t1`, `reachable\t1`, exit 1.
  - `reference_and_unparseable_are_counted_not_failed`: `…g.5030578=` and `…g.33000167_33000168ins(10)` → exit 0.
  - `prefix_limits_the_walk`: `--prefix NC_012920.1:` → `keys\t1`.
  - `missing_genome_exits_2`.
- [ ] **Step 3:** `cargo test -p clingen-index` → FAIL.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Real-data check**: `target/release/clingen-index reach --index data/index_corpus --genome data/genome_grch38/GRCh38.bin` → `keys\t15528839`, every key `reachable`, exit 0; report wall time. Any non-reachable key is a finding to triage before continuing (it means real Registry keys our tools cannot produce).
- [ ] **Step 6:** Full Rust gate passes.
- [ ] **Step 7: Commit** `feat(clingen-index): reach proves every index key is found from a VCF record`.

### Task 6: `clingen-index regions`

**Files:**
- Create: `crates/clingen-index/src/regions.rs`; modify `src/main.rs`
- Test: `crates/clingen-index/tests/regions.rs`

**Interfaces:**
- CLI: `regions (--refseq <ncbiRefSeq.txt[.gz]> --gencode <knownGene.txt[.gz]> --set <cds|pc_exons|exons|genes> | --bed <BED>) [--pad 20] [--merge-gap 5000] [--max-window 50000] [--flank 200] [--chrom <C|all>] [--batch 500] --out-dir <DIR>`
- UCSC columns: `ncbiRefSeq` has a leading `bin` column, `knownGene` does not; both then read `name chrom strand txStart txEnd cdsStart cdsEnd exonCount exonStarts exonEnds` (0-based, half-open; comma lists). A transcript is coding when `cdsEnd > cdsStart`.
- Sets: `cds` = coding transcripts' exon ∩ [cdsStart, cdsEnd); `pc_exons` = coding transcripts' exons; `exons` = all exons; `genes` = [txStart, txEnd) of all transcripts. `--bed`: BED3+, any chrom form `resolve_contig` accepts.
- Algorithm per contig (primary contigs only; others skipped and counted): pad ±`pad` and clamp to `[0, length)`; sort; merge when `next.start − cur.end ≤ merge_gap`; split each merged interval of length `L` into `k = ceil(L / max_window)` near-equal windows (the core); query span = core ± `flank`, clamped. Registry query `begin` = query start (0-based) and `end` = query end (exclusive), because the Registry returns alleles with start > `begin` and end ≤ `end`.
- Output in `--out-dir`: `windows_<chrom>_<NNNN>.tsv` (`chrom` bare, e.g. `21`, `X`, `MT`; ≤ `--batch` windows per file; one chromosome per file), columns `chrom accession begin end core_begin core_end`; and `summary.tsv` (`chrom intervals merged windows core_bp query_bp`). stderr: `contigs=<n> windows=<n> core_bp=<n> query_bp=<n> skipped_records=<n>`.

- [ ] **Step 1: Write failing tests** with a small UCSC fixture written in the test (a 3-exon coding transcript and a non-coding one on `chr21`, a coding one on `chr21_KI270872v1_alt`, one on `chrM`, a 120,000 bp gene for splitting):
  - `sets_select_the_right_intervals` (exact core intervals for each set with `--pad 0 --merge-gap 0`).
  - `padding_merging_and_splitting` (two exons 30 bp apart become one interval with `--pad 20 --merge-gap 0`, since padding makes them overlap, and stay two with `--pad 0 --merge-gap 0`; exons 4,000 bp apart merge at `--merge-gap 5000` and not at `--merge-gap 3000`; the 120,000 bp gene at `--set genes --max-window 50000` gives 3 windows of 40,000).
  - `flank_is_clamped_at_contig_ends` (a window at position 0 has `begin` 0; chrM end clamped to 16,569).
  - `chrom_filter_and_batches` (`--chrom 21 --batch 2` → only `windows_21_*` files, each ≤ 2 rows; `MT` rows carry `NC_012920.1`).
  - `bed_input_with_ensembl_names`.
  - `non_primary_records_are_skipped_and_counted`.
- [ ] **Step 2:** `cargo test -p clingen-index --test regions` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Real-data check** against `data/regions/download/{ncbiRefSeq,knownGene}.txt.gz` (sizes measured on 2026-10-24 with an equivalent script): `--set cds --pad 20 --merge-gap 0` core ≈ 49.5 Mb; `--set pc_exons --pad 20 --merge-gap 0` ≈ 148.2 Mb with 254,888 merged intervals; `--set exons --pad 20 --merge-gap 0` ≈ 271.7 Mb; `--set pc_exons --pad 20 --merge-gap 5000 --max-window 50000` → `windows=60328`, core ≈ 406.7 Mb. Record exact values; a difference above 0.1% is a finding.
- [ ] **Step 5:** Full Rust gate passes.
- [ ] **Step 6: Commit** `feat(clingen-index): regions builds gene/exome query windows`.

### Task 7: Registry client: `registry enumerate` and `registry lookup`

**Files:**
- Create: `crates/clingen-index/src/registry.rs`; modify `src/main.rs`, `Cargo.toml` (ureq, serde, serde_json), `Cargo.lock`
- Create: `crates/clingen-index/tests/common/fake_registry.rs` (std `TcpListener` server; start from Reference › Rust HTTP › *Fake HTTP/1.1 server for unit tests*)
- Test: `crates/clingen-index/tests/registry.rs`

**Interfaces:**
- Shared flags: `--base-url <URL>` (default `https://reg.genome.network`), hidden `--retry-base-ms <N>` (default 30000; tests use 1).
- Client (start from Reference › Rust HTTP › *Registry client*): one `ureq::Agent` with `http_status_as_error(false)`, a 600 s global timeout, the `User-Agent`; bodies read with `.body_mut().with_config().limit(1 << 30)`. Retryable: timeouts, I/O errors, 429, 5xx, a body that is not the expected JSON. Others fail at once with the status and the first 200 bytes of the body.
- `registry enumerate --windows <TSV>... --out <TSV> [--page-size 50000]`: per window `GET /alleles?refseq=<accession>&begin=<b>&end=<e>&fields=none+@id+genomicAlleles.hgvs+genomicAlleles.referenceGenome&limit=<page>&skip=<s>` (keep `+` and `@` literal with `query_raw`), next page while a page is full. Per allele: CA id from `@id` (after `/allele/`), and the HGVS in `genomicAlleles[*].hgvs` that starts with `<accession>:`; skip it if it ends with `=`. Writes `HGVS\tCAID` (`CAID` exactly as the Registry writes it, e.g. `CA068622`) to `<out>.partial`, renamed to `<out>` at the end. stderr: `windows=<n> requests=<n> alleles=<n> reference=<n> no_grch38_hgvs=<n> written=<n>`.
- `registry lookup --sites <TSV> --out <TSV> [--batch 250000]`: the Rust port of `harvest/query_registry.py` (`POST /alleles?file=vcf&fields=none+@id+genomicAlleles.referenceGenome+genomicAlleles.hgvs`, VCF body with `##contig=<ID=<c>,length=<GRCh38 length>,assembly=GRCh38>` for each contig present, results count must equal rows); output the same seven columns; atomic like `enumerate`.

- [ ] **Step 1: Write failing tests** in `tests/registry.rs` (fake server records each request):
  - `enumerate_pages_until_a_short_page` (`--page-size 2`; pages of 2, 2, 1 → 5 rows; requests carry `skip=0`, `2`, `4`).
  - `enumerate_keeps_only_the_windows_grch38_hgvs` (alleles whose hgvs lists hold `NC_000021.9:…`, `CM000683.2:…`, `NC_000021.8:…`, and one with only `…=` → only the `NC_000021.9` non-reference rows written; counts in stderr).
  - `enumerate_keeps_m_hgvs_on_chrM`.
  - `enumerate_retries_429_and_503_then_succeeds`.
  - `enumerate_fails_on_400_and_writes_nothing` (exit 2; neither `<out>` nor `<out>.partial` exists).
  - `enumerate_fails_on_truncated_json_after_six_attempts` (exactly 6 requests seen; exit 2; no output).
  - `lookup_writes_the_harvester_columns` (registered, `_:CA`, `errorType` results → `CA…`, empty, `ERROR:IncorrectReferenceAllele`; the POST body has the `##contig` line for `21` with length 46709983).
  - `lookup_result_count_mismatch_fails`.
  - `#[ignore]` `live_enumerate_finds_a_known_allele` (only with `CLINGEN_LIVE_REGISTRY=1`): window `NC_000021.9` 5030570–5030590 includes `NC_000021.9:g.5030578C>T\tCA749862939`.
- [ ] **Step 2:** `cargo test -p clingen-index --test registry` → FAIL.
- [ ] **Step 3: Implement**; add the dependencies and pin the lock per Global Constraints.
- [ ] **Step 4: Live checks** (record in the report): the ignored test passes; `registry lookup` on the first 1,000 rows of `data/clinvar/20261019/to_query.tsv` reproduces the first 1,000 rows of `chunks/chunk_00000.tsv` exactly, except alleles registered since 2026-10-24 (list any); `registry enumerate` on 10 windows from the probe set returns at least the non-reference `NC_000021.9` keys recorded then.
- [ ] **Step 5:** Full Rust gate passes.
- [ ] **Step 6: Commit** `feat(clingen-index): registry enumerate and lookup clients`.

### Task 8: Requirements, Dockerfile, ignore files

**Files:**
- Create: `Dockerfile`, `.dockerignore`, `pipeline/environment.yml`; modify `.gitignore`

**Interfaces:**
- `pipeline/environment.yml` (channels conda-forge, bioconda): `openjdk=21.0.10`, `nextflow=26.04.7`, `nf-test=0.9.5`, `cfn-lint=1.57.2` (start from Reference › Pins › *pipeline/environment.yml*).
- `Dockerfile` (start from Reference › Pins › *Dockerfile base images and pinned AWS CLI v2*): stage `build` `rust:1.94.1-bookworm@sha256:…` runs `cargo build --release --locked -p clingen-index`; stage `awscli` downloads the 2.37.12 installer for `TARGETARCH`, checks its SHA-256, installs; runtime `debian:bookworm-20261005-slim@sha256:…` with `bash coreutils gawk gzip curl ca-certificates procps openssl`, the AWS CLI and `/usr/local/bin/clingen-index`; no `ENTRYPOINT`; `LABEL org.opencontainers.image.version=0.1.0`.
- `.dockerignore`: `data/`, `target/`, `.git/`, `pipeline/work/`, `.nf-test/`, `results/`.

- [ ] **Step 1:** Write the files.
- [ ] **Step 2: Verify** (needs a container engine; the controller has asked the user before installing one): `docker build -t clingen-index:0.1.0 .`; then in the image `clingen-index --version` (`clingen-index 0.1.0`), `aws --version` (starts `aws-cli/2.37.12`), `ps --version`, `gawk --version`, `openssl version`, `curl --version`; `docker inspect --format '{{json .Config.Entrypoint}}'` → `null`; and `clingen-index reach` over a tiny fixture index mounted in succeeds. With no engine available, report the task BLOCKED on that step; do not mark it done.
- [ ] **Step 3: Commit** `build: Dockerfile and pinned pipeline requirements`.

### Task 9: Pipeline skeleton: config, schema, profiles, entry workflow

**Files:**
- Create: `pipeline/main.nf`, `pipeline/nextflow.config`, `pipeline/conf/{local,slurm,awsbatch,host}.config`, `pipeline/nextflow_schema.json`, `pipeline/nf-test.config`, `pipeline/tests/nextflow.config`, `pipeline/tests/main.nf.test`, `pipeline/tests/check_profiles.sh`, `pipeline/modules/{sources,registry,index}.nf` (process declarations with `stub:` blocks only; real scripts come in Tasks 10–11)

**Interfaces:**
- Params and defaults exactly as the spec's parameter table, plus hidden `registry_url = 'https://reg.genome.network'`, `gnomad_listing_url = 'https://storage.googleapis.com/storage/v1/b/gcp-public-data--gnomad/o'`, `ucsc_base = 'https://hgdownload.soe.ucsc.edu/goldenPath/hg38'`, `help`, `helpFull`, `showHidden`. All in the schema (`$schema` exactly `https://json-schema.org/draft/2020-12/schema`, `$defs` groups, `allOf`); `validation.logging.unrecognisedParams = 'error'`. `chrom`: type `["string","integer"]`, pattern `^((chr)?([1-9]|1[0-9]|2[0-2]|X|Y|MT)|chrM|all)$`. `outdir`: plain string (no `directory-path` format, so an `s3://` outdir is not probed at validation). Start from Reference › nf-schema snippets.
- `main.nf`: `--help`/`--helpFull` via `paramsHelp` then stop; `validateParameters()`; `paramsSummaryLog()`; normalise CLI strings (`chrom` without `chr`, `chrM`→`MT`; integers; booleans via `.toString().toBoolean()`); build the chromosome list (`all` = 1–22, X, Y, MT); call the modules.
- `nextflow.config`: manifest (`name 'clingen-index-regeneration'`, `nextflowVersion '!>=26.04.7'`, `version '0.1.0'`), `plugins { id 'nf-schema@3.0.0' }`, process defaults (`container = params.container`, `errorStrategy { sleep(Math.pow(2, task.attempt) * 1000 as long); 'retry' }`, `maxRetries = 2`), labels `network`, `sort` (4 CPUs, 16 GB), `genome` (8 GB), `build` (2 GB), `small` (1 CPU, 2 GB); `withName: 'ENUMERATE|LOOKUP' { maxForks = 1 }`; `profiles { local { includeConfig 'conf/local.config' } … }`; `timeline`, `report`, `trace` enabled into `${params.outdir}/pipeline_info`.
- Profiles: `local` = local executor + `docker.enabled = true`; `slurm` = slurm executor, `queue = params.slurm_queue`, `withLabel: network { queue = params.network_queue ?: params.slurm_queue }`, `apptainer.enabled = true`, `apptainer.autoMounts = true`, container `docker://${params.container}` when it has no scheme; `awsbatch` = Reference › AWS › *pipeline/conf/awsbatch.config* (no `aws.batch.cliPath`); `host` = all container engines off, `process.container = null`.
- `pipeline/tests/check_profiles.sh`: `nextflow config pipeline -profile <p> [params]` for each profile; asserts with grep: `local` → `executor = 'local'` and `docker { enabled = true }`; `slurm` → `executor = 'slurm'` and `apptainer`; `awsbatch` → `executor = 'awsbatch'` and `workDir`/`aws.region`; `local,host` → no container.

- [ ] **Step 1: Install the launcher stack** from `pipeline/environment.yml` (the controller has the user's approval; e.g. micromamba into a prefix the task report names). `nextflow -version` shows 26.04.7; `nf-test version` shows 0.9.5.
- [ ] **Step 2: Write failing nf-test** `pipeline/tests/main.nf.test` (`nextflow_pipeline`, `-stub`, profile `host`): `help prints the parameters` (`help = true` → success; stdout contains `--chrom` and `--regions`); `missing chrom fails` (failed; output mentions `chrom`); `chrom 23 fails`; `chrom chrM is accepted` (stub run succeeds and the summary shows `MT`); `unknown param fails` (`foo = "x"` → failed).
- [ ] **Step 3:** `cd pipeline && nf-test test tests/main.nf.test` → FAIL.
- [ ] **Step 4: Implement** the files above; every process in the modules exists with inputs/outputs and a `stub:` that writes its declared outputs.
- [ ] **Step 5:** `nf-test test tests/main.nf.test` passes; `bash pipeline/tests/check_profiles.sh` passes; `nextflow lint pipeline` (if available in 26.04.7) reports no errors.
- [ ] **Step 6: Commit** `feat(pipeline): skeleton with schema, profiles and stubbed processes`.

### Task 10: Source processes

**Files:**
- Modify: `pipeline/modules/sources.nf`
- Create: `pipeline/tests/modules/sources.nf.test`, fixtures `pipeline/tests/data/{clinvar_mini.vcf.gz,clinvar_mini.vcf.gz.md5,gnomad_mini.vcf.bgz,gnomad_listing.json,ucsc_mini_ncbiRefSeq.txt.gz,ucsc_mini_knownGene.txt.gz,chrM.fa.gz,chrM_md5sum.txt}`

**Interfaces (each process: `script:` real, `stub:` from Task 9):**
- `RESOLVE_GNOMAD(version, chroms, datasets)` [network, small]: lists `params.gnomad_listing_url` (`?prefix=release/&delimiter=/`, then `?prefix=release/<v>/vcf/<dataset>/`), picks the highest version that has `vcf/genomes/` objects when `version == 'latest'` (today 4.1.1; `4.1.2` has only `constraint/`), and emits `gnomad_files.tsv` (`dataset chrom url md5_base64 size`). `MT` → `release/3.1/vcf/genomes/gnomad.genomes.v3.1.sites.chrM.vcf.bgz` for genomes, nothing for exomes. URLs use the mirror (`gcs`: `https://storage.googleapis.com/gcp-public-data--gnomad/…`; `aws`: `https://gnomad-public-us-east-1.s3.amazonaws.com/…`); md5 always from the GCS listing.
- `GNOMAD_SITES(dataset, chrom, url, md5)` [network, small]: `curl -sS --fail --retry 5 "$url" | tee >(openssl md5 -binary | base64 > got.md5) | gzip -dc | awk …`; strips `chr`, splits multi-ALT, writes `sites.tsv` (`CHROM POS REF ALT`) and `skipped.tsv` (`* . non-ACGT`); fails when `got.md5` ≠ `md5`.
- `CLINVAR_SITES(vcf, md5file, chroms)` [network for the URL form, small]: verifies md5 (`openssl dgst -md5 -r`), reads `##fileDate`, and applies the rules of the README's ClinVar run (`non_primary_contig`, `no_alt`, `non_acgt`) → `sites_<chrom>.tsv` per requested chromosome, `skipped.tsv`, `clinvar_date.txt`.
- `FETCH_UCSC()` [network, small]: `ncbiRefSeq.txt.gz`, `knownGene.txt.gz` and their `Last-Modified`/size (`curl -sI`) into `ucsc_sources.tsv`.
- `FETCH_FASTA()` [network, small]: `hg38.fa.gz` checked against `md5sum.txt`.

- [ ] **Step 1: Make fixtures**: `clinvar_mini.vcf.gz` (BGZF via `bgzip` if present, else gzip; 20 records from the real ClinVar covering chr21, MT, an `NT_` contig, an `ALT=.` and an `N`-containing allele) and its `.md5`; `gnomad_mini.vcf.bgz` (12 gnomAD-style records incl. a multi-ALT and a `*`); `gnomad_listing.json` (GCS JSON API shape with `release/4.1/`, `4.1.1/` (with `vcf/genomes/` objects), `4.1.2/` (only `constraint/`)); `chrM.fa.gz` (the chrM record of `data/genome_grch38/download/hg38.fa.gz`) with a matching md5 list.
- [ ] **Step 2: Write failing nf-test** `tests/modules/sources.nf.test` (offline; `file://` URLs for curl): `RESOLVE_GNOMAD picks 4.1.1 and the v3.1 chrM file`; `GNOMAD_SITES splits multi-ALT and skips star` (exact `sites.tsv` snapshot); `GNOMAD_SITES md5 mismatch fails` (`process.failed`); `CLINVAR_SITES applies the skip rules` (exact counts per reason; MT sites present for `chroms = ['MT']`); stub tests for every process.
- [ ] **Step 3:** `nf-test test tests/modules/sources.nf.test` → FAIL.
- [ ] **Step 4: Implement** the scripts.
- [ ] **Step 5:** Tests pass. Live spot check (record): `RESOLVE_GNOMAD` with the real listing returns `4.1.1` and 48 genomes/exomes files for `all`.
- [ ] **Step 6: Commit** `feat(pipeline): source processes for gnomAD, ClinVar, UCSC and hg38`.

### Task 11: Registry and index processes, workflow wiring, publish gate, report

**Files:**
- Modify: `pipeline/modules/registry.nf`, `pipeline/modules/index.nf`, `pipeline/main.nf`
- Create: `pipeline/tests/modules/index.nf.test`, `pipeline/tests/workflow_mt.nf.test` (tag `network`), fixtures `pipeline/tests/data/{harvest_MT.tsv,pairs_MT.tsv,regions_MT.bed}`

**Interfaces:**
- `REGIONS(tables or bed, chrom)` [small] → `clingen-index regions … --batch params.windows_per_task --out-dir windows` → `windows/windows_*.tsv` (flattened into one task each) and `summary.tsv`.
- `ENUMERATE(batch)` [network, maxForks 1] → `clingen-index registry enumerate --windows batch --out enum_<chrom>_<n>.tsv --base-url params.registry_url`.
- `SPLIT_CACHE(cache files)` [small] → registered rows split per chromosome (`cache_<chrom>.tsv`).
- `MERGE_SITES(chrom, sites files, cache_<chrom>.tsv)` [sort]: `LC_ALL=C sort -u` the sites; composite key `CHROM:POS:REF:ALT` as field 1 on both sides; `join -t $'\t'` (and `join -v 1`) on sorted inputs splits cached answers from sites to query, so memory stays bounded; `split -l params.batch_size -d` → `lookup_<chrom>_<n>.tsv`.
- `LOOKUP(batch)` [network, maxForks 1] → `clingen-index registry lookup`.
- `COLLECT(chrom, cached, answers…)` [sort] → `harvest/<chrom>.tsv.gz` (sorted by POS) [publishDir copy].
- `BUILD_GENOME(fasta, chroms)` [genome] → `genome/GRCh38.bin{,.idx}` (`--contigs` = the run's chromosomes); skipped when `--genome` is given.
- `KEYS(chrom, enum files, harvest)` [sort]: `{ cat enum_*.tsv; gzip -dc harvest.tsv.gz | awk -F'\t' '$5 ~ /^CA/ {print $6 "\t" $5}'; } | LC_ALL=C sort -u -S 2G -T . > <accession>.tsv`.
- `BUILD_INDEX(shards, genome?)` [build]: `clingen-index build --sorted $(ls shards/*.tsv | LC_ALL=C sort) --out index --source …` with `--genome genome/GRCh38.bin --genome-relative` for a built genome or `--genome <abs path>` for a supplied one; sources `registry_<set>_<run date>`, `clinvar_<fileDate>`, `gnomad_<dataset>_v<version>`.
- `VALIDATE(chrom, harvest, genome)` [small] → `clingen-index validate --strict` (chunk dir staged as `chunks/chunk_00000.tsv`) → `validate_<chrom>.txt` and a status.
- `REACH(chrom, index, genome)` [small] → `clingen-index reach --prefix <accession>:` → `reach_<chrom>.txt` and a status.
- `FINALIZE(index, genome?, statuses)` [small]: fails unless every status is 0 or `params.allow_mismatches`; publishes `index/` (and a built `genome/`) with `mode: 'copy'`.
- `REPORT(...)` [small]: `coverage.tsv`, `summary.md`, `sources.tsv`, `skipped.tsv.gz` (awk/bash only).

- [ ] **Step 1: Make fixtures** from real data: `harvest_MT.tsv` (60 ClinVar MT harvest rows from `data/clinvar/20261019/chunks`, incl. 2 unregistered), `pairs_MT.tsv` (their registered `HGVS\tCAID`, byte-sorted), `regions_MT.bed` (two chrM windows).
- [ ] **Step 2: Write failing offline nf-test** `tests/modules/index.nf.test` (genome built in `setup` by `BUILD_GENOME` from `chrM.fa.gz`): `KEYS sorts and dedups` (exact output); `BUILD_INDEX refuses a locale-sorted shard` (`process.failed`); `BUILD_INDEX then REACH reaches every key` (reach file shows every key reachable); `VALIDATE strict passes on real MT rows`; `FINALIZE stops on a failing status`; `FINALIZE publishes with allow_mismatches`; `MERGE_SITES splits cached from new and batches` (exact counts); stub test of the whole workflow for `chrom = "21"` and `chrom = "all"` (all processes appear in the trace; `index/caid.fst` and `reports/summary.md` published).
- [ ] **Step 3: Write the network test** `tests/workflow_mt.nf.test` (tag `network`, real, profile `host`): `--chrom MT --regions genes` with the UCSC mini tables, `--clinvar_vcf tests/data/clinvar_mini.vcf.gz`, genome from `chrM.fa.gz` → success; every reach key reachable; `coverage.tsv` lists `registry` and `clinvar` rows for MT.
- [ ] **Step 4:** `nf-test test tests/modules/index.nf.test` → FAIL.
- [ ] **Step 5: Implement** the processes and wiring.
- [ ] **Step 6:** `nf-test test` (all offline tests) passes; then `nf-test test --tag network` passes (live, about a minute; the Registry is hit by one enumerate and one lookup request).
- [ ] **Step 7: Commit** `feat(pipeline): registry, index, gate and report processes`.

### Task 12: SLURM shim test, AWS Batch profile, CloudFormation and guide

**Files:**
- Create: `pipeline/tests/slurm-shim/{sbatch,squeue,scancel}` (from Reference › SLURM › shim snippets), `pipeline/tests/slurm.config`, `pipeline/tests/run_slurm_shim.sh`
- Create: `pipeline/aws/clingen-batch.yaml` (from Reference › AWS › *pipeline/aws/clingen-batch.yaml*), `docs/aws-batch.md`
- Modify: `pipeline/conf/awsbatch.config` (if Task 9's version needs changes)

**Interfaces:**
- `run_slurm_shim.sh`: puts the shims first on `PATH`, runs `nextflow run pipeline -profile slurm,host -c pipeline/tests/slurm.config -stub --chrom MT --slurm_queue normal --network_queue transfer --outdir <tmp>` and asserts: success; every trace row has a numeric `native_id`; a `network`-labelled task's `.command.run` contains `#SBATCH -p transfer` and a `sort`-labelled one `#SBATCH -p normal`. `slurm.config`: `executor { queueStatInterval = '2s'; pollInterval = '1s'; exitReadTimeout = '30s' }`, `apptainer.enabled = false`.
- CloudFormation template: bucket (lifecycle `work/` expiry, parameter default 30 days), ECR repository, instance role (S3 on the bucket, `service-role/AmazonEC2ContainerServiceforEC2Role`, `AmazonEC2ContainerRegistryReadOnly`) and profile, launch template (gp3 root volume size parameter, IMDSv2 required, hop limit 2), compute environment (`Type` parameter SPOT|EC2; `SPOT_PRICE_CAPACITY_OPTIMIZED` for Spot, `BEST_FIT_PROGRESSIVE` for EC2; `optimal`; `MaxvCpus`, subnets, security groups parameters), job queue; outputs bucket, queue, repository URI.
- `docs/aws-batch.md`: prerequisites; `aws cloudformation deploy --capabilities CAPABILITY_IAM …`; build for `linux/amd64` or `linux/arm64` and push to ECR; the launcher IAM policy (Reference › AWS › *Launcher IAM policy*); running the launcher for ~31 h (small EC2 instance in `tmux`, or a Batch head job); `--gnomad_mirror aws` in us-east-1; the `work/` expiry vs `-resume`; Fusion as the alternative to the in-image CLI; costs; teardown (`aws cloudformation delete-stack`, emptying the bucket and repository first).

- [ ] **Step 1: Write** the shims, configs and script; run `bash pipeline/tests/run_slurm_shim.sh` → passes (if it fails, fix the shims against the real Nextflow behaviour and note it in the report).
- [ ] **Step 2: Write** the template and guide; `cfn-lint pipeline/aws/clingen-batch.yaml` (cfn-lint 1.57.2) → no errors; `nextflow config pipeline -profile awsbatch --aws_queue q --container x -work-dir s3://b/work` shows `executor = 'awsbatch'`, `queue = 'q'`, `aws.region`, no `cliPath`.
- [ ] **Step 3: Commit** `feat(pipeline): slurm shim test, AWS Batch template and guide`.

### Task 13: First real run, end-to-end check, README

**Files:**
- Modify: `README.md` (new section "Regenerating the index (Nextflow)"; ClinVar section points to `clingen-index genome`; layout section), `docs/known-differences.md` if the run adds evidence

- [ ] **Step 1: Real local run** (Docker if available, else `-profile local,host` with `target/release` on `PATH` for `clingen-index`): `nextflow run pipeline -profile local --chrom 21 --regions pc_exons --cache data/chr21_chunks,data/clinvar/20261019/chunks --genome data/genome_grch38/GRCh38.bin --outdir results/chr21` → success; record wall time, windows, requests, enumerated keys, index entries, `validate`/`reach` summaries from `reports/`.
- [ ] **Step 2: Resume check**: rerun the same command with `-resume` → every task `CACHED` (count from the trace); then delete one `ENUMERATE` work dir's output and rerun → exactly that task re-runs.
- [ ] **Step 3: End-to-end**: both tools offline (`sandbox-exec` deny-network, as in the README) on the ClinVar chr21 records with `index=results/chr21/index` → `e2e_crosscheck.py` against `results/chr21/harvest/21.tsv.gz` (gunzipped into a chunk dir) gives `disagree=0 missing=0`; record the lines.
- [ ] **Step 4: README**: requirements (`pipeline/environment.yml`, container engines), the three run examples (spec), outputs, the measured numbers from Steps 1–3 pasted verbatim, timings, limits, links to `docs/aws-batch.md`; the full exome run (~31 h) is described but not run.
- [ ] **Step 5: Commit** `docs: regenerate the index with the Nextflow pipeline`.
