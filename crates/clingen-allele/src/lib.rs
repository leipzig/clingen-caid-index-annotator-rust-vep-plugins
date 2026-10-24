//! ClinGen Allele Registry CA-ID annotation for GRCh38 variants.

mod annotator;
mod contig;
mod hgvs;
mod index;
#[cfg(feature = "test-support")]
pub mod test_support;

use std::path::PathBuf;

pub use annotator::{Annotator, Lookup, Stats, Status};
pub use contig::{Contig, primary_contigs, resolve_contig};
pub use hgvs::{HgvsResult, canonical_hgvs};
pub use index::{CaIndex, IndexBuilder, Manifest, SortedIndexWriter, format_caid, parse_caid};

/// Errors produced by the `clingen-allele` crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Reference genome access or variant normalisation failed in vareffect.
    #[error("reference genome error: {0}")]
    Genome(#[from] vareffect::VarEffectError),
    /// Reading or writing a file failed.
    #[error("I/O error on {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The index manifest is missing, malformed, or inconsistent with the index.
    #[error("invalid manifest: {0}")]
    Manifest(String),
    /// The FST index could not be built or read.
    #[error("FST index error: {0}")]
    Fst(#[from] fst::Error),
    /// The reference genome is not GRCh38: a GRCh38 primary contig it carries
    /// has another length, or it carries none of them.
    #[error("genome {} is not GRCh38: {reason}", path.display())]
    GenomeNotGrch38 { path: PathBuf, reason: String },
    /// One HGVS key was harvested with two different CA IDs.
    #[error("duplicate HGVS key {hgvs} maps to both CA{first:06} and CA{second:06}")]
    DuplicateConflict {
        hgvs: String,
        first: u64,
        second: u64,
    },
    /// A key reached a [`SortedIndexWriter`] after one that sorts above it in
    /// byte order, the order the FST must be written in.
    #[error("keys out of order: {key} after {previous}")]
    Unsorted { previous: String, key: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_messages() {
        let e = Error::DuplicateConflict {
            hgvs: "NC_000021.9:g.33000172dup".into(),
            first: 1,
            second: 2,
        };
        assert_eq!(
            e.to_string(),
            "duplicate HGVS key NC_000021.9:g.33000172dup maps to both CA000001 and CA000002"
        );

        let e = Error::Io {
            path: PathBuf::from("index.fst"),
            source: std::io::Error::other("boom"),
        };
        assert_eq!(e.to_string(), "I/O error on index.fst: boom");

        let e = Error::GenomeNotGrch38 {
            path: PathBuf::from("hg19.bin"),
            reason: "contig chr21 is 48129895 bases long; GRCh38 chr21 is 46709983".into(),
        };
        assert_eq!(
            e.to_string(),
            "genome hg19.bin is not GRCh38: contig chr21 is 48129895 bases long; \
             GRCh38 chr21 is 46709983"
        );
    }
}
