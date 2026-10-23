//! `canonical_hgvs` against the strings the live ClinGen Allele Registry
//! returns (confirmed 2026-10-23), over a tiny genome of real hg38 windows.

use clingen_allele::test_support::tiny_genome;
use clingen_allele::{HgvsResult, canonical_hgvs};

fn hgvs(chrom: &str, pos1: u64, ref_allele: &str, alt_allele: &str) -> HgvsResult {
    let (_dir, genome) = tiny_genome();
    canonical_hgvs(
        chrom,
        pos1,
        ref_allele.as_bytes(),
        alt_allele.as_bytes(),
        &genome,
    )
    .expect("no genome error")
}

fn expect(chrom: &str, pos1: u64, ref_allele: &str, alt_allele: &str, expected: &str) {
    assert_eq!(
        hgvs(chrom, pos1, ref_allele, alt_allele),
        HgvsResult::Hgvs(expected.to_string()),
        "{chrom} {pos1} {ref_allele} {alt_allele}"
    );
}

#[test]
fn snv() {
    expect("21", 5030578, "C", "T", "NC_000021.9:g.5030578C>T");
}

#[test]
fn del_left_aligned() {
    expect("21", 33000166, "TA", "T", "NC_000021.9:g.33000172del");
}

#[test]
fn del_right_aligned() {
    expect("21", 33000171, "AA", "A", "NC_000021.9:g.33000172del");
}

#[test]
fn del_padded() {
    expect(
        "21",
        33000166,
        "TAAAAAA",
        "TAAAAA",
        "NC_000021.9:g.33000172del",
    );
}

#[test]
fn dup_left() {
    expect("21", 33000166, "T", "TA", "NC_000021.9:g.33000172dup");
}

#[test]
fn dup_right() {
    expect("21", 33000172, "A", "AA", "NC_000021.9:g.33000172dup");
}

#[test]
fn dinuc_dup() {
    expect(
        "21",
        33000127,
        "C",
        "CTC",
        "NC_000021.9:g.33000126_33000127dup",
    );
}

#[test]
fn ins_rotated() {
    expect(
        "21",
        33000166,
        "T",
        "TAG",
        "NC_000021.9:g.33000167_33000168insGA",
    );
}

#[test]
fn delins_revcomp() {
    expect(
        "21",
        33000017,
        "CA",
        "TG",
        "NC_000021.9:g.33000017_33000018delinsTG",
    );
}

#[test]
fn mito_m_prefix() {
    expect("MT", 3243, "A", "G", "NC_012920.1:m.3243A>G");
}

#[test]
fn ref_mismatch() {
    assert_eq!(hgvs("21", 33000166, "G", "T"), HgvsResult::RefMismatch);
}

#[test]
fn symbolic_alt() {
    assert_eq!(hgvs("21", 33000166, "T", "<DEL>"), HgvsResult::Unsupported);
    assert_eq!(hgvs("21", 33000166, "T", "*"), HgvsResult::Unsupported);
}

#[test]
fn iupac_alt() {
    assert_eq!(hgvs("21", 33000166, "T", "R"), HgvsResult::Unsupported);
}

#[test]
fn lowercase_alleles_match_uppercase() {
    let expected = hgvs("21", 33000166, "TA", "T");
    assert_eq!(
        expected,
        HgvsResult::Hgvs("NC_000021.9:g.33000172del".into())
    );
    assert_eq!(hgvs("21", 33000166, "ta", "t"), expected);
}

#[test]
fn contig_aliases() {
    for chrom in ["chr21", "NC_000021.9"] {
        expect(chrom, 5030578, "C", "T", "NC_000021.9:g.5030578C>T");
    }
    expect("chrM", 3243, "A", "G", "NC_012920.1:m.3243A>G");
    assert_eq!(
        hgvs("chr1_KI270706v1_random", 5030578, "C", "T"),
        HgvsResult::Unsupported
    );
}

#[test]
fn comma_alt_is_unsupported() {
    assert_eq!(hgvs("21", 33000166, "T", "A,G"), HgvsResult::Unsupported);
}
