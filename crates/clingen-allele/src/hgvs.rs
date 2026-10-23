//! Canonical GRCh38 HGVS keys, byte-identical to the ClinGen Allele Registry.
//!
//! The Registry names an allele by its normalised genomic HGVS on the
//! `NC_` accession (`NC_000021.9:g.33000172dup`, `NC_012920.1:m.3243A>G`).
//! vareffect's `format_hgvs_g` already applies the HGVS 3' rule and the
//! insertion-to-duplication rewrite; this module vets the input around it and
//! adapts the contig prefix.

use vareffect::FastaReader;
use vareffect::hgvs_g::format_hgvs_g;

use crate::{Error, resolve_contig};

/// Outcome of building the canonical HGVS key for one variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HgvsResult {
    /// The Registry-style HGVS string, e.g. `NC_000021.9:g.33000172dup`.
    Hgvs(String),
    /// REF disagrees with the reference genome (or runs past the contig end).
    RefMismatch,
    /// Not a variant we can key: patch/alt contig, symbolic or multi-allelic
    /// ALT, ambiguity codes, an empty allele, or REF equal to ALT.
    Unsupported,
}

/// Bytes that mark an ALT as symbolic, a breakend, a missing-allele
/// placeholder, or one of several comma-separated alleles.
const NON_SEQUENCE_ALT_BYTES: &[u8] = b",<>*[].";

fn is_plain_dna(allele: &[u8]) -> bool {
    !allele.is_empty()
        && allele
            .iter()
            .all(|b| matches!(b, b'A' | b'C' | b'G' | b'T'))
}

/// Build the Registry's canonical GRCh38 HGVS for a VCF-style variant.
///
/// * `chrom` — any form [`resolve_contig`] accepts (`21`, `chr21`, `MT`,
///   `NC_000021.9`).
/// * `pos1` — 1-based VCF position of the first REF base.
/// * `ref_allele`, `alt_allele` — VCF alleles, case-insensitive. Only `ACGT`
///   is accepted, which is stricter than vareffect (it also lets `N` through).
///
/// REF is checked against `genome`, so a variant on the wrong build comes back
/// as [`HgvsResult::RefMismatch`] rather than as a key for a different allele.
///
/// # Errors
///
/// [`Error::Genome`] when vareffect cannot read the reference, for example
/// because `genome` has no sequence for a supported contig.
pub fn canonical_hgvs(
    chrom: &str,
    pos1: u64,
    ref_allele: &[u8],
    alt_allele: &[u8],
    genome: &FastaReader,
) -> Result<HgvsResult, Error> {
    if alt_allele
        .iter()
        .any(|b| NON_SEQUENCE_ALT_BYTES.contains(b))
    {
        return Ok(HgvsResult::Unsupported);
    }
    let Some(contig) = resolve_contig(chrom) else {
        return Ok(HgvsResult::Unsupported);
    };

    let ref_allele = ref_allele.to_ascii_uppercase();
    let alt_allele = alt_allele.to_ascii_uppercase();
    // VCF positions are 1-based, so 0 names no base.
    if !is_plain_dna(&ref_allele) || !is_plain_dna(&alt_allele) || pos1 == 0 {
        return Ok(HgvsResult::Unsupported);
    }

    // vareffect positions are 0-based. `verify_ref` raises an error for a REF
    // that overruns the contig, but that can only mean REF does not match.
    let pos0 = pos1 - 1;
    let ref_end = pos0.saturating_add(ref_allele.len() as u64);
    if genome
        .chrom_length(contig.vareffect_name)
        .is_some_and(|len| ref_end > len)
        || !genome.verify_ref(contig.vareffect_name, pos0, &ref_allele)?
    {
        return Ok(HgvsResult::RefMismatch);
    }

    let Some(formatted) = format_hgvs_g(
        contig.vareffect_name,
        pos0,
        &ref_allele,
        &alt_allele,
        genome,
    )?
    else {
        return Ok(HgvsResult::Unsupported);
    };

    // Whatever contig name vareffect put in front, the key starts with the
    // accession; mitochondrial coordinates use `m.` instead of `g.`.
    let Some((_, edit)) = formatted.split_once(":g.") else {
        return Ok(HgvsResult::Unsupported);
    };
    let coordinate_type = if contig.mito { "m" } else { "g" };
    Ok(HgvsResult::Hgvs(format!(
        "{}:{coordinate_type}.{edit}",
        contig.accession
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{CHR21_LEN, tiny_genome};

    fn check(chrom: &str, pos1: u64, ref_allele: &str, alt_allele: &str) -> HgvsResult {
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

    #[test]
    fn unsupported_alts() {
        for alt in [
            "A,G",
            "<DEL>",
            "*",
            ".",
            "T]chr1:100]",
            "[chr1:100[T",
            "A>",
            "",
            "N",
            "TN",
            "R",
        ] {
            assert_eq!(
                check("21", 33000166, "T", alt),
                HgvsResult::Unsupported,
                "{alt:?}"
            );
        }
    }

    #[test]
    fn bad_ref_or_position_is_unsupported() {
        // Empty and ambiguous REF, REF equal to ALT, and position 0 (VCF is
        // 1-based; a naive `pos1 - 1` would underflow).
        assert_eq!(check("21", 33000166, "", "T"), HgvsResult::Unsupported);
        assert_eq!(check("21", 33000166, "N", "T"), HgvsResult::Unsupported);
        assert_eq!(check("21", 33000166, "T", "T"), HgvsResult::Unsupported);
        assert_eq!(check("21", 0, "T", "A"), HgvsResult::Unsupported);
    }

    #[test]
    fn ref_overrunning_the_contig_is_a_mismatch() {
        // A two-base REF at chr21's last base runs one base off the end.
        let last = CHR21_LEN as u64;
        assert_eq!(check("21", last, "AC", "A"), HgvsResult::RefMismatch);
        assert_eq!(check("21", last + 1, "A", "C"), HgvsResult::RefMismatch);
        assert_eq!(check("21", 50_000_000, "A", "C"), HgvsResult::RefMismatch);
        assert_eq!(check("21", u64::MAX, "A", "C"), HgvsResult::RefMismatch);
    }

    #[test]
    fn contig_missing_from_genome_is_an_error() {
        let (_dir, genome) = tiny_genome();
        let result = canonical_hgvs("1", 100, b"A", b"C", &genome);
        assert!(
            matches!(
                result,
                Err(Error::Genome(
                    vareffect::VarEffectError::ChromNotFound { .. }
                ))
            ),
            "{result:?}"
        );
    }

    #[test]
    fn mito_uses_m_coordinates_for_indels_too() {
        // rCRS 3243 is A and 3244 is G, so inserting T after 3243 neither
        // shifts nor duplicates.
        assert_eq!(
            check("chrM", 3243, "A", "AT"),
            HgvsResult::Hgvs("NC_012920.1:m.3243_3244insT".into())
        );
    }
}
