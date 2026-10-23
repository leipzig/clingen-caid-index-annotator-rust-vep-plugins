//! ClinGen Allele Registry CA-ID annotation for GRCh38 variants.

mod contig;
mod hgvs;
#[cfg(feature = "test-support")]
pub mod test_support;

use std::path::PathBuf;

pub use contig::{Contig, resolve_contig};
pub use hgvs::{HgvsResult, canonical_hgvs};

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
    /// One HGVS key was harvested with two different CA IDs.
    #[error("duplicate HGVS key {hgvs} maps to both CA{first} and CA{second}")]
    DuplicateConflict {
        hgvs: String,
        first: u64,
        second: u64,
    },
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
            "duplicate HGVS key NC_000021.9:g.33000172dup maps to both CA1 and CA2"
        );

        let e = Error::Io {
            path: PathBuf::from("index.fst"),
            source: std::io::Error::other("boom"),
        };
        assert_eq!(e.to_string(), "I/O error on index.fst: boom");
    }
}
