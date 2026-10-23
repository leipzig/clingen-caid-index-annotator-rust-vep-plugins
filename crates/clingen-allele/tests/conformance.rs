//! chr21 conformance: `canonical_hgvs` against the ClinGen Allele Registry.
//!
//! `tests/data/chr21_sample.tsv` is a 10,000-row sample of the gnomAD v4.1
//! chr21 harvest, drawn by `scripts/sample_harvest.py` (seed 21), in the
//! harvest's format: no header, columns
//! `CHROM POS REF ALT CAID HGVS_GRCh38 HGVS_GRCh37`. Rows listed in the
//! `known-differences` block of `docs/known-differences.md` are exempt from
//! the Registry comparison.
//!
//! The genome tests need the chr21-only genome built by
//! `scripts/build_chr21_genome.sh`:
//!
//! ```text
//! CLINGEN_CHR21_GENOME=$PWD/data/genome_chr21/chr21.bin \
//!     cargo test -p clingen-allele --test conformance --release
//! ```
//!
//! Without the variable they print a notice and pass.

use std::collections::HashSet;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

use clingen_allele::{HgvsResult, canonical_hgvs, resolve_contig};
use vareffect::FastaReader;

/// Path to the chr21 genome binary (`chr21.bin`).
const GENOME_VAR: &str = "CLINGEN_CHR21_GENOME";

/// hg38 chr21 length, to catch the variable pointing at some other genome.
const CHR21_LEN: u64 = 46_709_983;

const SAMPLE_ROWS: usize = 10_000;
const MIN_SAMPLE_INDELS: usize = 2_000;

/// Failures listed in a panic message before "... and N more".
const SHOWN_FAILURES: usize = 20;

/// A VCF-style allele: 1-based POS, uppercase REF and ALT.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Allele {
    chrom: String,
    pos: u64,
    ref_allele: Vec<u8>,
    alt_allele: Vec<u8>,
}

impl Allele {
    fn new(chrom: &str, pos: u64, ref_allele: &str, alt_allele: &str) -> Self {
        Allele {
            chrom: chrom.to_owned(),
            pos,
            ref_allele: ref_allele.as_bytes().to_vec(),
            alt_allele: alt_allele.as_bytes().to_vec(),
        }
    }

    fn is_indel(&self) -> bool {
        self.ref_allele.len() != self.alt_allele.len()
    }

    /// REF and ALT end in the same base, so a left-aligner would move it.
    fn ends_alike(&self) -> bool {
        self.ref_allele.last() == self.alt_allele.last()
    }
}

impl fmt::Display for Allele {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.chrom,
            self.pos,
            String::from_utf8_lossy(&self.ref_allele),
            String::from_utf8_lossy(&self.alt_allele)
        )
    }
}

struct SampleRow {
    allele: Allele,
    registry_hgvs: String,
}

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// The committed sample.
///
/// # Panics
///
/// On a row that is not seven tab-separated columns with a numeric POS, a
/// `CA<digits>` CAID and a GRCh38 HGVS.
fn sample() -> Vec<SampleRow> {
    let path = repo_path("tests/data/chr21_sample.tsv");
    read(&path)
        .lines()
        .enumerate()
        .map(|(i, line)| {
            let at = || format!("{}:{}", path.display(), i + 1);
            let cols: Vec<&str> = line.split('\t').collect();
            let &[chrom, pos, ref_allele, alt_allele, caid, hgvs38, _hgvs37] = cols.as_slice()
            else {
                panic!("{}: expected 7 tab-separated columns", at());
            };
            let pos = pos
                .parse()
                .unwrap_or_else(|_| panic!("{}: POS {pos:?} is not a number", at()));
            let registered = caid
                .strip_prefix("CA")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
            assert!(registered, "{}: CAID {caid:?} is not CA<digits>", at());
            assert!(!hgvs38.is_empty(), "{}: empty HGVS_GRCh38", at());
            SampleRow {
                allele: Allele::new(chrom, pos, ref_allele, alt_allele),
                registry_hgvs: hgvs38.to_owned(),
            }
        })
        .collect()
}

/// The `CHROM POS REF ALT` lines of the one fenced block with info string
/// `known-differences` in `markdown`; blank lines are ignored.
///
/// # Panics
///
/// If there is not exactly one such block, it is not closed, a line in it is
/// not four whitespace-separated fields with a positive POS, or a line repeats.
fn parse_known_differences(markdown: &str) -> HashSet<Allele> {
    let mut alleles = HashSet::new();
    let mut blocks = 0;
    let mut lines = markdown.lines().enumerate();
    while let Some((_, line)) = lines.next() {
        if line.trim() != "```known-differences" {
            continue;
        }
        blocks += 1;
        let mut closed = false;
        for (i, line) in lines.by_ref() {
            let line = line.trim();
            if line == "```" {
                closed = true;
                break;
            }
            if line.is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            let &[chrom, pos, ref_allele, alt_allele] = fields.as_slice() else {
                panic!(
                    "known-differences line {}: expected CHROM POS REF ALT, got {line:?}",
                    i + 1
                );
            };
            let pos = pos
                .parse()
                .ok()
                .filter(|&p: &u64| p > 0)
                .unwrap_or_else(|| panic!("known-differences line {}: bad POS in {line:?}", i + 1));
            let allele = Allele::new(chrom, pos, ref_allele, alt_allele);
            assert!(
                alleles.insert(allele),
                "known-differences line {}: {line:?} is listed twice",
                i + 1
            );
        }
        assert!(closed, "the known-differences block is not closed");
    }
    assert_eq!(
        blocks, 1,
        "expected exactly one ```known-differences block, found {blocks}"
    );
    alleles
}

fn known_differences() -> HashSet<Allele> {
    parse_known_differences(&read(&repo_path("../../docs/known-differences.md")))
}

/// The chr21 genome named by [`GENOME_VAR`], or `None` (after a notice) when
/// the variable is unset.
///
/// # Panics
///
/// If the variable is set but the genome cannot be opened or lacks hg38 chr21.
fn chr21_genome() -> Option<FastaReader> {
    let Some(path) = std::env::var_os(GENOME_VAR) else {
        // Straight to stderr, so the notice shows even while libtest
        // captures print! output.
        let _ = writeln!(std::io::stderr(), "skipped: {GENOME_VAR} unset");
        return None;
    };
    let path = PathBuf::from(path);
    let genome =
        FastaReader::open(&path).unwrap_or_else(|e| panic!("{GENOME_VAR}={}: {e}", path.display()));
    assert_eq!(
        genome.chrom_length("chr21"),
        Some(CHR21_LEN),
        "{GENOME_VAR}={} has no hg38 chr21",
        path.display()
    );
    Some(genome)
}

fn hgvs(genome: &FastaReader, allele: &Allele) -> HgvsResult {
    canonical_hgvs(
        &allele.chrom,
        allele.pos,
        &allele.ref_allele,
        &allele.alt_allele,
        genome,
    )
    .unwrap_or_else(|e| panic!("{allele}: {e}"))
}

/// The reference base at 1-based `pos1` on `chrom`.
fn base_at(genome: &FastaReader, chrom: &str, pos1: u64) -> u8 {
    let contig = resolve_contig(chrom).unwrap_or_else(|| panic!("unknown contig {chrom:?}"));
    genome
        .fetch_base(contig.vareffect_name, pos1 - 1)
        .unwrap_or_else(|e| panic!("{chrom}:{pos1}: {e}"))
}

/// The allele with the reference base before POS prepended to both sides.
fn left_padded(genome: &FastaReader, allele: &Allele) -> Allele {
    assert!(allele.pos > 1, "{allele}: no reference base before POS");
    let mut padded = allele.clone();
    padded.pos -= 1;
    let base = base_at(genome, &allele.chrom, padded.pos);
    padded.ref_allele.insert(0, base);
    padded.alt_allele.insert(0, base);
    padded
}

/// The allele with the reference base after REF's last base appended to both.
fn right_padded(genome: &FastaReader, allele: &Allele) -> Allele {
    let mut padded = allele.clone();
    let base = base_at(
        genome,
        &allele.chrom,
        allele.pos + allele.ref_allele.len() as u64,
    );
    padded.ref_allele.push(base);
    padded.alt_allele.push(base);
    padded
}

/// The allele shifted fully left, as a VCF left-aligner would: while REF and
/// ALT end in the same base and POS > 1, drop that base from both, prepend
/// the reference base before POS to both, and decrement POS. Each step keeps
/// both alleles' lengths, so neither becomes empty.
fn left_aligned(genome: &FastaReader, allele: &Allele) -> Allele {
    let mut shifted = allele.clone();
    while shifted.pos > 1 && shifted.ends_alike() {
        let base = base_at(genome, &shifted.chrom, shifted.pos - 1);
        shifted.ref_allele.pop();
        shifted.alt_allele.pop();
        shifted.ref_allele.insert(0, base);
        shifted.alt_allele.insert(0, base);
        shifted.pos -= 1;
    }
    shifted
}

/// Panic listing `failures` (the first [`SHOWN_FAILURES`] of them), if any.
fn assert_no_failures(what: &str, failures: &[String]) {
    if failures.is_empty() {
        return;
    }
    let mut message = format!("{} {what}:\n", failures.len());
    for failure in failures.iter().take(SHOWN_FAILURES) {
        message.push_str(&format!("  {failure}\n"));
    }
    if failures.len() > SHOWN_FAILURES {
        message.push_str(&format!(
            "  ... and {} more\n",
            failures.len() - SHOWN_FAILURES
        ));
    }
    panic!("{message}");
}

#[test]
fn fixtures_are_well_formed() {
    let rows = sample();
    assert_eq!(rows.len(), SAMPLE_ROWS);
    let indels = rows.iter().filter(|r| r.allele.is_indel()).count();
    assert!(indels >= MIN_SAMPLE_INDELS, "only {indels} indels");
    // Without these, the left-aligned form in `representation_invariance`
    // would leave every row as it is.
    let unaligned = rows
        .iter()
        .filter(|r| r.allele.is_indel() && r.allele.ends_alike())
        .count();
    assert!(unaligned > 0, "no sample indel needs left-alignment");
    let distinct: HashSet<&Allele> = rows.iter().map(|r| &r.allele).collect();
    assert_eq!(distinct.len(), rows.len(), "the sample repeats a row");
    // Panics if the block is missing or malformed.
    known_differences();
}

#[test]
#[should_panic(expected = "known-differences line 3: expected CHROM POS REF ALT")]
fn malformed_known_difference_fails() {
    parse_known_differences("# Known\n```known-differences\n21 5030578 C\n```\n");
}

#[test]
fn sample_matches_registry() {
    let Some(genome) = chr21_genome() else {
        return;
    };
    let rows = sample();
    let known = known_differences();

    let mut failures = Vec::new();
    let (mut compared, mut excluded) = (0, 0);
    for row in &rows {
        let ours = hgvs(&genome, &row.allele);
        let matches = ours == HgvsResult::Hgvs(row.registry_hgvs.clone());
        if known.contains(&row.allele) {
            excluded += 1;
            if matches {
                failures.push(format!(
                    "{}: listed in known-differences but matches the Registry ({})",
                    row.allele, row.registry_hgvs
                ));
            }
        } else {
            compared += 1;
            if !matches {
                failures.push(format!(
                    "{}: Registry {}, ours {ours:?}",
                    row.allele, row.registry_hgvs
                ));
            }
        }
    }
    let in_sample: HashSet<&Allele> = rows.iter().map(|r| &r.allele).collect();
    for allele in known.iter().filter(|a| !in_sample.contains(a)) {
        failures.push(format!(
            "{allele}: listed in known-differences but not a sample row"
        ));
    }

    println!("compared={compared} excluded={excluded}");
    assert_no_failures("sample rows disagree with the Registry", &failures);
}

#[test]
fn representation_invariance() {
    let Some(genome) = chr21_genome() else {
        return;
    };

    let mut failures = Vec::new();
    let (mut indels, mut realigned) = (0, 0);
    for row in sample().iter().filter(|r| r.allele.is_indel()) {
        indels += 1;
        let original = &row.allele;
        let expected = hgvs(&genome, original);
        if !matches!(expected, HgvsResult::Hgvs(_)) {
            failures.push(format!("{original}: no HGVS ({expected:?})"));
            continue;
        }
        let aligned = left_aligned(&genome, original);
        realigned += usize::from(aligned != *original);
        let forms = [
            ("left-padded", left_padded(&genome, original)),
            ("right-padded", right_padded(&genome, original)),
            ("left-aligned", aligned),
        ];
        for (form, variant) in forms {
            let got = hgvs(&genome, &variant);
            if got != expected {
                failures.push(format!(
                    "{original} {form} as {variant}: {got:?}, original {expected:?}"
                ));
            }
        }
    }

    println!("indels={indels} changed_by_left_alignment={realigned}");
    assert!(indels >= MIN_SAMPLE_INDELS, "only {indels} indels");
    assert_no_failures("indel forms give a different HGVS", &failures);
}
