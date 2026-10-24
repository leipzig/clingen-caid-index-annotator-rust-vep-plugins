//! GRCh38 primary-contig name resolution.
//!
//! The Registry's GRCh38 HGVS strings are keyed by `NC_` accession, while
//! callers hand us VCF-style names (`21`, `chr21`, `MT`). Only the 25 primary
//! sequences (1-22, X, Y, MT) are supported; patch, alt and unplaced contigs
//! resolve to `None`. The table also carries each sequence's GRCh38 length,
//! which is how [`Annotator::open`](crate::Annotator::open) tells a GRCh38
//! genome from another build.

/// A supported GRCh38 primary contig.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contig {
    /// UCSC-style name vareffect expects, e.g. `chr21` or `chrM`.
    pub vareffect_name: &'static str,
    /// GRCh38 RefSeq accession used in the Registry's HGVS, e.g. `NC_000021.9`.
    pub accession: &'static str,
    /// True for the mitochondrial genome (`m.` HGVS rather than `g.`).
    pub mito: bool,
    /// Length of the GRCh38 sequence in bases, e.g. 46,709,983 for chr21.
    pub length: u64,
}

const fn nuclear(vareffect_name: &'static str, accession: &'static str, length: u64) -> Contig {
    Contig {
        vareffect_name,
        accession,
        mito: false,
        length,
    }
}

/// The 25 GRCh38 primary sequences. Accessions are copied from vareffect's
/// `chrom` table (its lookup function is not usable here because it passes
/// unknown input through unchanged); `accessions_match_vareffect` keeps the
/// two in step. Lengths are the GRCh38 ones the harvester declares to the
/// Registry (`harvest/query_registry.py`).
static CONTIGS: [Contig; 25] = [
    nuclear("chr1", "NC_000001.11", 248_956_422),
    nuclear("chr2", "NC_000002.12", 242_193_529),
    nuclear("chr3", "NC_000003.12", 198_295_559),
    nuclear("chr4", "NC_000004.12", 190_214_555),
    nuclear("chr5", "NC_000005.10", 181_538_259),
    nuclear("chr6", "NC_000006.12", 170_805_979),
    nuclear("chr7", "NC_000007.14", 159_345_973),
    nuclear("chr8", "NC_000008.11", 145_138_636),
    nuclear("chr9", "NC_000009.12", 138_394_717),
    nuclear("chr10", "NC_000010.11", 133_797_422),
    nuclear("chr11", "NC_000011.10", 135_086_622),
    nuclear("chr12", "NC_000012.12", 133_275_309),
    nuclear("chr13", "NC_000013.11", 114_364_328),
    nuclear("chr14", "NC_000014.9", 107_043_718),
    nuclear("chr15", "NC_000015.10", 101_991_189),
    nuclear("chr16", "NC_000016.10", 90_338_345),
    nuclear("chr17", "NC_000017.11", 83_257_441),
    nuclear("chr18", "NC_000018.10", 80_373_285),
    nuclear("chr19", "NC_000019.10", 58_617_616),
    nuclear("chr20", "NC_000020.11", 64_444_167),
    nuclear("chr21", "NC_000021.9", 46_709_983),
    nuclear("chr22", "NC_000022.11", 50_818_468),
    nuclear("chrX", "NC_000023.11", 156_040_895),
    nuclear("chrY", "NC_000024.10", 57_227_415),
    Contig {
        vareffect_name: "chrM",
        accession: "NC_012920.1",
        mito: true,
        length: 16_569,
    },
];

/// The 25 GRCh38 primary contigs, chr1 to chr22, chrX, chrY, chrM.
///
/// Public so that tools which act on every supported contig (cutting a genome
/// down to them, say) take the list from here rather than restating it.
pub fn primary_contigs() -> &'static [Contig] {
    &CONTIGS
}

/// Resolve a contig name to a GRCh38 primary contig.
///
/// Accepts the bare name (`21`, `X`, `MT`), the `chr`-prefixed form in any
/// case (`chr21`, `CHRX`, `chrMT`), or the exact `NC_` accession from the
/// table. `M` and `MT` both mean the mitochondrial genome. Anything else,
/// including patch/alt/random contigs and `NW_`/`NT_` accessions, is `None`.
pub fn resolve_contig(name: &str) -> Option<Contig> {
    if let Some(c) = CONTIGS.iter().find(|c| c.accession == name) {
        return Some(*c);
    }
    let bare = match name.get(..3) {
        Some(prefix) if prefix.eq_ignore_ascii_case("chr") => &name[3..],
        _ => name,
    };
    let bare = if bare.eq_ignore_ascii_case("MT") {
        "M"
    } else {
        bare
    };
    CONTIGS
        .iter()
        // Every `vareffect_name` starts with "chr".
        .find(|c| c.vareffect_name[3..].eq_ignore_ascii_case(bare))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contig(name: &str) -> Contig {
        resolve_contig(name).unwrap_or_else(|| panic!("{name} should resolve"))
    }

    #[test]
    fn resolve_contig_forms() {
        let chr21 = Contig {
            vareffect_name: "chr21",
            accession: "NC_000021.9",
            mito: false,
            length: 46_709_983,
        };
        for name in ["21", "chr21", "NC_000021.9"] {
            assert_eq!(contig(name), chr21, "{name}");
        }

        for name in ["MT", "chrM", "chrMT", "M", "NC_012920.1"] {
            let c = contig(name);
            assert_eq!(c.vareffect_name, "chrM", "{name}");
            assert_eq!(c.accession, "NC_012920.1", "{name}");
            assert!(c.mito, "{name}");
        }

        assert_eq!(contig("X").accession, "NC_000023.11");
        assert_eq!(contig("Y").accession, "NC_000024.10");

        for name in ["chr1_KI270706v1_random", "NW_009646194.1", "foo"] {
            assert_eq!(resolve_contig(name), None, "{name}");
        }
    }

    #[test]
    fn resolve_contig_is_case_insensitive_and_strict() {
        assert_eq!(contig("CHRX").accession, "NC_000023.11");
        assert_eq!(contig("chry").accession, "NC_000024.10");
        assert_eq!(contig("Chr22").accession, "NC_000022.11");
        assert!(contig("mt").mito);
        assert!(contig("chrm").mito);

        for name in [
            "",
            "chr",
            "0",
            "01",
            "23",
            "chrchr1",
            "chr1 ",
            " 1",
            "NC_000021",
            "nc_000021.9",
            "NC_000021.10",
            "NT_187361.1",
            "MTX",
            "ch",
            "é",
            "chrM_extra",
        ] {
            assert_eq!(resolve_contig(name), None, "{name:?}");
        }
    }

    #[test]
    fn accessions_match_vareffect() {
        assert_eq!(CONTIGS.len(), 25);
        for c in &CONTIGS {
            assert_eq!(
                vareffect::chrom::ucsc_to_refseq(c.vareffect_name),
                c.accession,
                "{}",
                c.vareffect_name
            );
            assert_eq!(c.mito, c.vareffect_name == "chrM", "{}", c.vareffect_name);
        }
    }

    #[test]
    fn lengths_are_grch38() {
        let length = |name: &str| contig(name).length;
        assert_eq!(length("1"), 248_956_422);
        assert_eq!(length("21"), 46_709_983);
        assert_eq!(length("X"), 156_040_895);
        assert_eq!(length("Y"), 57_227_415);
        assert_eq!(length("MT"), 16_569);
        // The GRCh38 primary assembly: 3,088,269,832 nuclear bases plus chrM.
        let total: u64 = primary_contigs().iter().map(|c| c.length).sum();
        assert_eq!(total, 3_088_286_401);
    }
}
