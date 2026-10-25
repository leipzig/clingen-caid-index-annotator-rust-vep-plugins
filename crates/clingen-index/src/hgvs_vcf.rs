//! Turning a Registry HGVS key back into a VCF record.
//!
//! `reach` needs, for every key in an index, a VCF record that names the same
//! allele, so it can check that looking that record up finds the key again.
//! Only the forms `canonical_hgvs` writes are converted: a substitution,
//! `del`, `dup`, `ins` and `delins` on a GRCh38 primary accession, `g.` on
//! the nuclear ones and `m.` on `NC_012920.1`. Indels take their anchor base
//! (and a `dup` its copied bases) from the genome, which is why the
//! conversion is handed a reader.
//!
//! Two kinds of key name nothing a VCF record the adapters key could hold,
//! and are told apart here rather than left to fail the lookup:
//!
//! - a `delins` that puts back the bases it removes, a reference allele the
//!   Registry holds in that form (`NC_012920.1:m.3105_3106delinsAC` where the
//!   reference is `AC`): REF would equal ALT, which names no variant;
//! - a key whose record would hold a genome base other than ACGT, such as the
//!   `N` the rCRS keeps at 3107 (`NC_012920.1:m.3107del`): `canonical_hgvs`
//!   takes only ACGT, so no record holding it is ever looked up.

use clingen_allele::resolve_contig;

/// A VCF-style allele: 1-based position of the first REF base, and both
/// alleles spelled out, anchor base included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VcfAllele {
    /// The contig as vareffect names it (`chr21`, `chrM`), which
    /// `Annotator::lookup` accepts.
    pub(crate) chrom: &'static str,
    /// 1-based position of the first REF base.
    pub(crate) pos1: u64,
    /// REF, uppercase as the genome reader returns it.
    pub(crate) ref_allele: Vec<u8>,
    /// ALT.
    pub(crate) alt_allele: Vec<u8>,
}

/// What an HGVS key converts to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Parsed {
    /// A record that names the key's allele.
    Vcf(VcfAllele),
    /// A reference allele (`…=`, or a `delins` whose bases are the
    /// reference's): no variant, so no record.
    Reference,
    /// Not a form `canonical_hgvs` writes, or one that needs reference bases
    /// the genome does not have (off the contig, or not ACGT); the reason
    /// says which.
    Unparseable(&'static str),
}

/// The longest span, in bases, a key may cover and still be converted.
///
/// Converting reads the whole span from the genome, and the lookup then
/// normalises it, for that one key; the cap keeps a single huge allele from
/// stalling, or exhausting the memory of, a walk over millions of keys.
const MAX_SPAN: u64 = 10_000_000;

/// The reason given when the genome has no bases for a span the record needs.
const NO_REFERENCE: &str = "no reference sequence there";

/// The reason given when the record would hold a genome base other than ACGT,
/// which `canonical_hgvs` refuses.
const NOT_ACGT_REFERENCE: &str = "reference sequence there is not ACGT";

/// Convert `hgvs` to a VCF record, reading reference bases with
/// `ref_seq(chrom, start1, end1)` (1-based, inclusive, uppercase, `None` off
/// the contig), as `Annotator::ref_seq` does.
///
/// The record is the one the HGVS describes, not a normalised one: a `del` is
/// anchored on the base before it (after it, at position 1, where there is
/// none before), a `dup` inserts the copy after its last base, and an `ins`
/// is anchored on the base before the insertion. REF of a substitution is
/// taken from the key, so a key that disagrees with the genome comes back
/// from the lookup as a REF mismatch rather than being quietly corrected.
pub(crate) fn hgvs_to_vcf(
    hgvs: &str,
    ref_seq: impl Fn(&str, u64, u64) -> Option<Vec<u8>>,
) -> Parsed {
    convert(hgvs, &ref_seq).unwrap_or_else(Parsed::Unparseable)
}

/// [`hgvs_to_vcf`], with the reason a key is unparseable as the error so `?`
/// can carry it.
fn convert(
    hgvs: &str,
    ref_seq: &impl Fn(&str, u64, u64) -> Option<Vec<u8>>,
) -> Result<Parsed, &'static str> {
    let (accession, position) = hgvs.split_once(':').ok_or("no accession")?;
    // `resolve_contig` also takes `21` and `chr21`; a key is written with the
    // accession itself.
    let contig = resolve_contig(accession)
        .filter(|c| c.accession == accession)
        .ok_or("not a GRCh38 primary accession")?;
    let edit = match (position.split_once('.'), contig.mito) {
        (Some(("g", edit)), false) | (Some(("m", edit)), true) => edit,
        (Some(("g", _)), true) => return Err("g. on the mitochondrial accession"),
        (Some(("m", _)), false) => return Err("m. on a nuclear accession"),
        _ => return Err("not a g. or m. position"),
    };
    if edit.ends_with('=') {
        return Ok(Parsed::Reference);
    }

    let positions_len = edit
        .find(|c: char| !c.is_ascii_digit() && c != '_')
        .unwrap_or(edit.len());
    let (positions, op) = edit.split_at(positions_len);
    let (start, range_end) = parse_positions(positions)?;
    let end = range_end.unwrap_or(start);
    if end - start + 1 > MAX_SPAN {
        return Err("span too long");
    }

    let chrom = contig.vareffect_name;
    let read = |start1, end1| ref_seq(chrom, start1, end1).ok_or(NO_REFERENCE);
    let record = |pos1, ref_allele: Vec<u8>, alt_allele: Vec<u8>| {
        // Only bases read from the genome can fail this: a substitution's
        // come from the key, checked below.
        if !is_dna(&ref_allele) || !is_dna(&alt_allele) {
            return Err(NOT_ACGT_REFERENCE);
        }
        Ok(Parsed::Vcf(VcfAllele {
            chrom,
            pos1,
            ref_allele,
            alt_allele,
        }))
    };

    if let Some((ref_base, alt_base)) = op.split_once('>') {
        if range_end.is_some() || !is_base(ref_base) || !is_base(alt_base) {
            return Err("unsupported edit");
        }
        return record(start, ref_base.into(), alt_base.into());
    }
    match op {
        "del" if start >= 2 => {
            let reference = read(start - 1, end)?;
            let anchor = *reference.first().ok_or(NO_REFERENCE)?;
            record(start - 1, reference, vec![anchor])
        }
        // Nothing precedes position 1, so the anchor is the base after.
        // `end` is at most `MAX_SPAN` here, so `end + 1` cannot overflow.
        "del" => {
            let reference = read(1, end + 1)?;
            let anchor = *reference.last().ok_or(NO_REFERENCE)?;
            record(1, reference, vec![anchor])
        }
        "dup" => {
            let copied = read(start, end)?;
            let anchor = *copied.last().ok_or(NO_REFERENCE)?;
            record(end, vec![anchor], [&[anchor][..], &copied].concat())
        }
        _ => {
            if let Some(seq) = op.strip_prefix("delins") {
                if !is_sequence(seq) {
                    return Err("sequence is not ACGT");
                }
                let reference = read(start, end)?;
                // The Registry holds some reference alleles in this form.
                if reference == seq.as_bytes() {
                    return Ok(Parsed::Reference);
                }
                record(start, reference, seq.into())
            } else if let Some(seq) = op.strip_prefix("ins") {
                // `e - start`, not `start + 1`, which overflows at the last
                // u64; a range ends after its start, so this cannot underflow.
                if range_end.is_none_or(|e| e - start != 1) {
                    return Err("ins is not between adjacent positions");
                }
                if !is_sequence(seq) {
                    return Err("sequence is not ACGT");
                }
                let anchor = read(start, start)?;
                let alt = [&anchor[..], seq.as_bytes()].concat();
                record(start, anchor, alt)
            } else {
                Err("unsupported edit")
            }
        }
    }
}

/// HGVS positions `s` or `s_e`, both 1-based; the second is `None` for a
/// single position. A range must end after it starts.
///
/// `text` is the run of digits and `_` the edit starts with, so `parse` never
/// sees the leading `+` it would also accept.
fn parse_positions(text: &str) -> Result<(u64, Option<u64>), &'static str> {
    let position = |p: &str| {
        p.parse::<u64>()
            .ok()
            .filter(|&n| n > 0)
            .ok_or("position is not a positive integer")
    };
    match text.split_once('_') {
        None => Ok((position(text)?, None)),
        Some((start, end)) => {
            let (start, end) = (position(start)?, position(end)?);
            if end <= start {
                return Err("range does not end after it starts");
            }
            Ok((start, Some(end)))
        }
    }
}

/// One of `A`, `C`, `G`, `T`.
fn is_base(text: &str) -> bool {
    matches!(text.as_bytes(), [b'A' | b'C' | b'G' | b'T'])
}

/// One or more of `A`, `C`, `G`, `T`.
fn is_sequence(text: &str) -> bool {
    is_dna(text.as_bytes())
}

/// One or more of `A`, `C`, `G`, `T`, the only alleles `canonical_hgvs` takes.
fn is_dna(bases: &[u8]) -> bool {
    !bases.is_empty() && bases.iter().all(|b| matches!(b, b'A' | b'C' | b'G' | b'T'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clingen_allele::test_support::{CHR21_LEN, CHRM_LEN, windows};

    /// The tiny genome as a closure: the committed windows pasted into a
    /// background of `N` on contigs of GRCh38's lengths, read the way
    /// `Annotator::ref_seq` reads (`None` past the end or for another contig).
    fn tiny() -> impl Fn(&str, u64, u64) -> Option<Vec<u8>> {
        let windows = windows();
        move |chrom, start1, end1| {
            let len = match chrom {
                "chr21" => CHR21_LEN,
                "chrM" => CHRM_LEN,
                _ => return None,
            } as u64;
            if start1 == 0 || start1 > end1 || end1 > len {
                return None;
            }
            let base = |pos1| {
                windows
                    .iter()
                    .filter(|w| w.chrom == chrom)
                    .find_map(|w| w.base_at(pos1))
                    .unwrap_or(b'N')
            };
            Some((start1..=end1).map(base).collect())
        }
    }

    fn convert(hgvs: &str) -> Parsed {
        hgvs_to_vcf(hgvs, tiny())
    }

    fn vcf(chrom: &'static str, pos1: u64, ref_allele: &str, alt_allele: &str) -> Parsed {
        Parsed::Vcf(VcfAllele {
            chrom,
            pos1,
            ref_allele: ref_allele.as_bytes().to_vec(),
            alt_allele: alt_allele.as_bytes().to_vec(),
        })
    }

    #[test]
    fn substitution_is_its_own_record() {
        assert_eq!(
            convert("NC_000021.9:g.5030578C>T"),
            vcf("chr21", 5030578, "C", "T")
        );
        assert_eq!(
            convert("NC_012920.1:m.3243A>G"),
            vcf("chrM", 3243, "A", "G")
        );
    }

    #[test]
    fn del_is_anchored_on_the_base_before() {
        // 33000166 is T, then 33000167-33000172 are six As.
        assert_eq!(
            convert("NC_000021.9:g.33000172del"),
            vcf("chr21", 33000171, "AA", "A")
        );
        assert_eq!(
            convert("NC_000021.9:g.33000167_33000172del"),
            vcf("chr21", 33000166, "TAAAAAA", "T")
        );
    }

    #[test]
    fn del_at_position_1_is_anchored_on_the_base_after() {
        // The tiny genome is N at position 1, so give it the rCRS's first
        // bases, GATCACAGG.
        let rcrs = |chrom: &str, start1: u64, end1: u64| {
            assert_eq!(chrom, "chrM");
            let start0 = usize::try_from(start1 - 1).unwrap();
            let end0 = usize::try_from(end1).unwrap();
            b"GATCACAGG".get(start0..end0).map(<[u8]>::to_vec)
        };
        assert_eq!(
            hgvs_to_vcf("NC_012920.1:m.1del", rcrs),
            vcf("chrM", 1, "GA", "A")
        );
        assert_eq!(
            hgvs_to_vcf("NC_012920.1:m.1_2del", rcrs),
            vcf("chrM", 1, "GAT", "T")
        );
    }

    #[test]
    fn dup_inserts_the_copy_after_its_last_base() {
        assert_eq!(
            convert("NC_000021.9:g.33000172dup"),
            vcf("chr21", 33000172, "A", "AA")
        );
        // 33000126-33000127 are TC.
        assert_eq!(
            convert("NC_000021.9:g.33000126_33000127dup"),
            vcf("chr21", 33000127, "C", "CTC")
        );
    }

    #[test]
    fn ins_is_anchored_on_the_base_before() {
        assert_eq!(
            convert("NC_000021.9:g.33000167_33000168insGA"),
            vcf("chr21", 33000167, "A", "AGA")
        );
    }

    #[test]
    fn delins_replaces_the_range() {
        // 33000017-33000018 are CA.
        assert_eq!(
            convert("NC_000021.9:g.33000017_33000018delinsTG"),
            vcf("chr21", 33000017, "CA", "TG")
        );
        assert_eq!(
            convert("NC_000021.9:g.33000017delinsTG"),
            vcf("chr21", 33000017, "C", "TG")
        );
    }

    #[test]
    fn reference_allele_has_no_record() {
        assert_eq!(convert("NC_000021.9:g.5030578="), Parsed::Reference);
    }

    #[test]
    fn delins_that_puts_back_the_reference_is_a_reference_allele() {
        // 33000017-33000018 are CA.
        assert_eq!(
            convert("NC_000021.9:g.33000017_33000018delinsCA"),
            Parsed::Reference
        );
        assert_eq!(convert("NC_000021.9:g.33000017delinsC"), Parsed::Reference);
        // One base different is a change.
        assert_eq!(
            convert("NC_000021.9:g.33000017_33000018delinsCG"),
            vcf("chr21", 33000017, "CA", "CG")
        );
    }

    /// The rCRS from 3101 to 3110, ATCTACNTTC, with the `N` it keeps at 3107
    /// (the bases of tests/data/chrM.fa.gz in the pipeline), and nothing else.
    fn rcrs_3101_3110(chrom: &str, start1: u64, end1: u64) -> Option<Vec<u8>> {
        assert_eq!(chrom, "chrM");
        let start0 = usize::try_from(start1.checked_sub(3101)?).unwrap();
        let end0 = usize::try_from(end1.checked_sub(3100)?).unwrap();
        b"ATCTACNTTC".get(start0..end0).map(<[u8]>::to_vec)
    }

    #[test]
    fn a_record_holding_a_genome_base_other_than_acgt_is_unparseable() {
        for hgvs in [
            "NC_012920.1:m.3107del",
            "NC_012920.1:m.3105_3108del",
            "NC_012920.1:m.3107dup",
            "NC_012920.1:m.3107_3108insA",
            "NC_012920.1:m.3106_3107delinsAC",
        ] {
            assert_eq!(
                hgvs_to_vcf(hgvs, rcrs_3101_3110),
                Parsed::Unparseable("reference sequence there is not ACGT"),
                "{hgvs}"
            );
        }
        // Beside the N, keys convert as ever.
        assert_eq!(
            hgvs_to_vcf("NC_012920.1:m.3106del", rcrs_3101_3110),
            vcf("chrM", 3105, "AC", "A")
        );
        assert_eq!(
            hgvs_to_vcf("NC_012920.1:m.3105_3106delinsAC", rcrs_3101_3110),
            Parsed::Reference
        );
        // A substitution's REF is the key's: an N there is an edit the
        // conversion does not take, as before.
        assert_eq!(
            hgvs_to_vcf("NC_012920.1:m.3107N>A", rcrs_3101_3110),
            Parsed::Unparseable("unsupported edit")
        );
    }

    #[test]
    fn accession_and_coordinate_type_must_agree() {
        for (hgvs, reason) in [
            ("NC_000021.9:m.5030578C>T", "m. on a nuclear accession"),
            ("NC_012920.1:g.3243A>G", "g. on the mitochondrial accession"),
            ("NC_000021.8:g.5030578C>T", "not a GRCh38 primary accession"),
            // A name `resolve_contig` takes is still not the accession.
            ("chr21:g.5030578C>T", "not a GRCh38 primary accession"),
            ("NC_000021.9:c.100A>G", "not a g. or m. position"),
            ("NC_000021.9", "no accession"),
        ] {
            assert_eq!(convert(hgvs), Parsed::Unparseable(reason), "{hgvs}");
        }
    }

    #[test]
    fn forms_canonical_hgvs_never_writes_are_unparseable() {
        for (hgvs, reason) in [
            (
                "NC_000021.9:g.33000167_33000169insGA",
                "ins is not between adjacent positions",
            ),
            (
                "NC_000021.9:g.33000167insGA",
                "ins is not between adjacent positions",
            ),
            (
                "NC_000021.9:g.33000167_33000168ins(10)",
                "sequence is not ACGT",
            ),
            (
                "NC_000021.9:g.33000017_33000018delinsN",
                "sequence is not ACGT",
            ),
            ("NC_000021.9:g.33000167_33000170inv", "unsupported edit"),
            (
                "NC_000021.9:g.33000167_33000170conNC_000021.9:g.100_103",
                "unsupported edit",
            ),
            ("NC_000021.9:g.33000167A[10]", "unsupported edit"),
            ("NC_000021.9:g.33000167_33000168A>T", "unsupported edit"),
            ("NC_000021.9:g.5030578C>TT", "unsupported edit"),
            (
                "NC_000021.9:g.(?_33000167)_(33000170_?)del",
                "position is not a positive integer",
            ),
            ("NC_000021.9:g.?del", "position is not a positive integer"),
            ("NC_000021.9:g.0A>C", "position is not a positive integer"),
            // Past u64.
            (
                "NC_000021.9:g.18446744073709551616del",
                "position is not a positive integer",
            ),
            // The last u64, where `start + 1` would overflow.
            (
                "NC_000021.9:g.18446744073709551615insA",
                "ins is not between adjacent positions",
            ),
            (
                "NC_000021.9:g.33000172_33000172del",
                "range does not end after it starts",
            ),
            (
                "NC_000021.9:g.33000172_33000170del",
                "range does not end after it starts",
            ),
        ] {
            assert_eq!(convert(hgvs), Parsed::Unparseable(reason), "{hgvs}");
        }
    }

    #[test]
    fn bases_the_genome_does_not_have_are_unparseable() {
        // chr21 is 46,709,983 bases long.
        for hgvs in [
            "NC_000021.9:g.46709984del",
            "NC_000021.9:g.46709984dup",
            "NC_000021.9:g.46709984_46709985insA",
            "NC_000021.9:g.46709983_46709984delinsA",
        ] {
            assert_eq!(
                convert(hgvs),
                Parsed::Unparseable("no reference sequence there"),
                "{hgvs}"
            );
        }
    }

    #[test]
    fn a_span_over_ten_million_bases_is_too_long() {
        // A genome of As, so the longest allowed span is quick to read.
        let all_a = |_: &str, start1: u64, end1: u64| {
            Some(vec![b'A'; usize::try_from(end1 - start1 + 1).unwrap()])
        };
        let Parsed::Vcf(allele) = hgvs_to_vcf("NC_000021.9:g.2_10000001del", all_a) else {
            panic!("a 10,000,000 bp span is allowed");
        };
        assert_eq!((allele.pos1, allele.ref_allele.len()), (1, 10_000_001));
        assert_eq!(
            hgvs_to_vcf("NC_000021.9:g.2_10000002del", all_a),
            Parsed::Unparseable("span too long")
        );
        assert_eq!(
            hgvs_to_vcf("NC_000021.9:g.1_10000001dup", all_a),
            Parsed::Unparseable("span too long")
        );
    }
}
