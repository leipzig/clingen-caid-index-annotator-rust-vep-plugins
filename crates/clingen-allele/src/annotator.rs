//! The annotation entry point both tool adapters call: a VCF-style variant in,
//! a ClinGen CA ID (when the index knows one) out.
//!
//! An [`Annotator`] pairs a [`CaIndex`] with the GRCh38 reference genome. For
//! each variant it builds the Registry's canonical HGVS (see
//! [`canonical_hgvs`]) and looks that key up, so every spelling of an allele
//! (left- or right-aligned, padded or not) lands on the same CA ID.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use vareffect::FastaReader;

use crate::{CaIndex, Error, HgvsResult, Manifest, canonical_hgvs, format_caid, resolve_contig};

const MANIFEST_FILE: &str = "manifest.json";

/// The index layout version this crate reads.
const SUPPORTED_FORMAT_VERSION: u32 = 1;

/// The only assembly the HGVS keys can be on.
const SUPPORTED_ASSEMBLY: &str = "GRCh38";

/// How one [`Annotator::lookup`] ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The canonical HGVS is in the index; `caid` is set.
    Found,
    /// A valid variant whose canonical HGVS the index does not hold; `hgvs`
    /// is set, `caid` is not.
    NotInIndex,
    /// REF disagrees with the reference genome.
    RefMismatch,
    /// Not a variant that can be keyed: a patch or alt contig, a symbolic or
    /// multi-allelic ALT, or a contig the genome file cannot be read for.
    Unsupported,
}

/// The result of annotating one variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    /// The canonical Registry HGVS, set for [`Status::Found`] and
    /// [`Status::NotInIndex`].
    pub hgvs: Option<String>,
    /// The allele's CA ID printed as `CA{n}`, set for [`Status::Found`].
    pub caid: Option<String>,
    /// What happened.
    pub status: Status,
}

impl Lookup {
    fn without_key(status: Status) -> Self {
        Self {
            hgvs: None,
            caid: None,
            status,
        }
    }
}

/// How many lookups ended in each [`Status`] since the [`Annotator`] opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    /// Lookups that ended [`Status::Found`].
    pub found: u64,
    /// Lookups that ended [`Status::NotInIndex`].
    pub not_in_index: u64,
    /// Lookups that ended [`Status::RefMismatch`].
    pub ref_mismatch: u64,
    /// Lookups that ended [`Status::Unsupported`].
    pub unsupported: u64,
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ClinGen: found={} not_in_index={} ref_mismatch={} unsupported={}",
            self.found, self.not_in_index, self.ref_mismatch, self.unsupported
        )
    }
}

/// Annotates variants with ClinGen CA IDs.
///
/// Shared by reference: [`lookup`](Self::lookup) takes `&self`, so one
/// `Annotator` serves every thread. (`FastaReader::try_clone` exists for
/// per-thread handles, but the reader is already `Sync`.)
pub struct Annotator {
    index: CaIndex,
    genome: FastaReader,
    found: AtomicU64,
    not_in_index: AtomicU64,
    ref_mismatch: AtomicU64,
    unsupported: AtomicU64,
    /// Contigs already reported to stderr. Only the genome-error path touches
    /// this, so the normal lookup path stays lock-free.
    failed_contigs: Mutex<HashSet<String>>,
}

impl Annotator {
    /// Open the index in `index_dir` and the flat binary genome at
    /// `genome_bin` (its `.idx` sidecar must sit next to it).
    ///
    /// # Errors
    ///
    /// [`Error::Manifest`] if the index is not format version 1 or not on
    /// GRCh38 (the message names what was found), the errors of
    /// [`CaIndex::open`] for an unreadable index, and [`Error::Genome`] if the
    /// genome cannot be opened.
    pub fn open(index_dir: &Path, genome_bin: &Path) -> Result<Self, Error> {
        let index = CaIndex::open(index_dir)?;
        check_manifest(index.manifest(), &index_dir.join(MANIFEST_FILE))?;
        let genome = FastaReader::open(genome_bin)?;
        Ok(Self {
            index,
            genome,
            found: AtomicU64::new(0),
            not_in_index: AtomicU64::new(0),
            ref_mismatch: AtomicU64::new(0),
            unsupported: AtomicU64::new(0),
            failed_contigs: Mutex::new(HashSet::new()),
        })
    }

    /// The genome the index's manifest names, if it names one.
    ///
    /// Reads `manifest.json` in `index_dir`. A relative `genome` path is
    /// resolved against `index_dir`. `None` if the manifest is missing or
    /// unparseable, or has no `genome`.
    pub fn genome_path_from_manifest(index_dir: &Path) -> Option<PathBuf> {
        let json = std::fs::read_to_string(index_dir.join(MANIFEST_FILE)).ok()?;
        let manifest: Manifest = serde_json::from_str(&json).ok()?;
        // `join` keeps an absolute path as it is.
        Some(index_dir.join(manifest.genome?))
    }

    /// Look up the CA ID of a VCF-style variant.
    ///
    /// * `chrom` is any name [`resolve_contig`] accepts (`21`, `chr21`, `MT`).
    /// * `pos1` is the 1-based VCF position of the first REF base.
    ///
    /// Never fails and never panics: whatever is wrong with one variant is
    /// reported in its [`Status`] and in [`stats`](Self::stats), so a single
    /// bad record cannot abort a run. If the genome cannot be read for a
    /// contig (typically because the genome file does not carry it), the
    /// variant is [`Status::Unsupported`] and one line per such contig goes
    /// to stderr.
    pub fn lookup(&self, chrom: &str, pos1: u64, ref_allele: &[u8], alt_allele: &[u8]) -> Lookup {
        match canonical_hgvs(chrom, pos1, ref_allele, alt_allele, &self.genome) {
            Ok(HgvsResult::Hgvs(hgvs)) => match self.index.get(&hgvs) {
                Some(caid) => {
                    count(&self.found);
                    Lookup {
                        hgvs: Some(hgvs),
                        caid: Some(format_caid(caid)),
                        status: Status::Found,
                    }
                }
                None => {
                    count(&self.not_in_index);
                    Lookup {
                        hgvs: Some(hgvs),
                        caid: None,
                        status: Status::NotInIndex,
                    }
                }
            },
            Ok(HgvsResult::RefMismatch) => {
                count(&self.ref_mismatch);
                Lookup::without_key(Status::RefMismatch)
            }
            Ok(HgvsResult::Unsupported) => {
                count(&self.unsupported);
                Lookup::without_key(Status::Unsupported)
            }
            Err(error) => {
                self.report_genome_error(chrom, &error);
                count(&self.unsupported);
                Lookup::without_key(Status::Unsupported)
            }
        }
    }

    /// The uppercase reference base at 1-based `pos1` on `chrom`.
    ///
    /// `None` for position 0, a contig name that is not a GRCh38 primary
    /// contig, a contig the genome file does not carry, or a position past the
    /// end of the contig.
    pub fn base_at(&self, chrom: &str, pos1: u64) -> Option<u8> {
        let contig = resolve_contig(chrom)?;
        let pos0 = pos1.checked_sub(1)?;
        self.genome
            .fetch_base(contig.vareffect_name, pos0)
            .ok()
            .map(|base| base.to_ascii_uppercase())
    }

    /// How many lookups have ended in each [`Status`] so far.
    ///
    /// Each counter is read on its own, so a snapshot taken while other
    /// threads are still looking variants up may not add up to a moment that
    /// ever existed.
    pub fn stats(&self) -> Stats {
        Stats {
            found: self.found.load(Ordering::Relaxed),
            not_in_index: self.not_in_index.load(Ordering::Relaxed),
            ref_mismatch: self.ref_mismatch.load(Ordering::Relaxed),
            unsupported: self.unsupported.load(Ordering::Relaxed),
        }
    }

    /// Print a one-line warning the first time `chrom` fails.
    fn report_genome_error(&self, chrom: &str, error: &Error) {
        // Key on the canonical name so `1` and `chr1` share one warning;
        // `canonical_hgvs` only reads the genome for a resolvable contig, but
        // fall back to the caller's spelling rather than assume that.
        let contig = resolve_contig(chrom).map_or(chrom, |c| c.vareffect_name);
        if self.first_failure(contig) {
            eprintln!(
                "clingen-allele: genome lookup failed for contig '{contig}' ({error}); \
                 variants on this contig are reported Unsupported"
            );
        }
    }

    /// True the first time `contig` is passed in.
    fn first_failure(&self, contig: &str) -> bool {
        // A poisoned set is still a usable set; the lookup path must not panic.
        self.failed_contigs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(contig.to_owned())
    }
}

fn count(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}

/// Reject an index this crate cannot interpret.
fn check_manifest(manifest: &Manifest, path: &Path) -> Result<(), Error> {
    if manifest.format_version != SUPPORTED_FORMAT_VERSION {
        return Err(Error::Manifest(format!(
            "{}: format_version is {}, but only {SUPPORTED_FORMAT_VERSION} is supported",
            path.display(),
            manifest.format_version
        )));
    }
    if manifest.assembly != SUPPORTED_ASSEMBLY {
        return Err(Error::Manifest(format!(
            "{}: assembly is {:?}, but only {SUPPORTED_ASSEMBLY} is supported",
            path.display(),
            manifest.assembly
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IndexBuilder;
    use crate::test_support::tiny_genome;

    #[test]
    fn genome_errors_are_reported_once_per_contig() {
        let (genome_dir, _reader) = tiny_genome();
        let index_dir = tempfile::tempdir().unwrap();
        IndexBuilder::new()
            .finish(
                index_dir.path(),
                Manifest {
                    format_version: 1,
                    assembly: "GRCh38".into(),
                    sources: Vec::new(),
                    harvest_date: "2026-10-23".into(),
                    entries: 0,
                    vareffect_version: "0.6.0".into(),
                    genome: None,
                },
            )
            .unwrap();
        let annotator =
            Annotator::open(index_dir.path(), &genome_dir.path().join("genome.bin")).unwrap();

        // `1` and `chr1` are the same contig; neither has been reported yet.
        annotator.lookup("1", 1000, b"A", b"G");
        annotator.lookup("chr1", 1000, b"A", b"G");
        assert!(!annotator.first_failure("chr1"), "chr1 already reported");
        assert!(annotator.first_failure("chr2"), "chr2 is new");
        assert!(!annotator.first_failure("chr2"));
    }
}
