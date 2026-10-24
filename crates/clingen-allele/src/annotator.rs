//! The annotation entry point both tool adapters call: a VCF-style variant in,
//! a ClinGen CA ID (when the index knows one) out.
//!
//! An [`Annotator`] pairs a [`CaIndex`] with the GRCh38 reference genome. For
//! each variant it builds the Registry's canonical HGVS (see
//! [`canonical_hgvs`]) and looks that key up, so every spelling of an allele
//! (left- or right-aligned, padded or not) lands on the same CA ID.

use std::collections::HashSet;
use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use vareffect::FastaReader;

use crate::contig::primary_contigs;
use crate::index::{FORMAT_VERSION, MANIFEST_FILE};
use crate::{CaIndex, Error, HgvsResult, Manifest, canonical_hgvs, format_caid, resolve_contig};

/// The only assembly the HGVS keys can be on.
const SUPPORTED_ASSEMBLY: &str = "GRCh38";

/// Contigs already reported to stderr, for the life of the process (so a
/// second `Annotator`, or one per thread, does not repeat the line). Only the
/// genome-error path touches this, so the normal lookup path stays lock-free.
static FAILED_CONTIGS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

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
    /// Lookups that ended [`Status::Unsupported`], plus alleles counted by
    /// [`Annotator::record_unsupported`].
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
}

impl Annotator {
    /// Open the index in `index_dir` and the flat binary genome at
    /// `genome_bin` (its `.idx` sidecar must sit next to it).
    ///
    /// # Errors
    ///
    /// [`Error::Manifest`] if the index is not format version 1 or not on
    /// GRCh38 (the message names what was found), the errors of
    /// [`CaIndex::open`] for an unreadable index, [`Error::Genome`] if the
    /// genome cannot be opened, and [`Error::GenomeNotGrch38`] if it is not
    /// GRCh38: a primary contig it carries (any of chr1-chr22, chrX, chrY,
    /// chrM, under whatever naming the file uses) has a length other than
    /// GRCh38's, or it carries none of them. A genome may lack some primary
    /// contigs (a chr21-only build, say); variants on those are
    /// [`Status::Unsupported`].
    pub fn open(index_dir: &Path, genome_bin: &Path) -> Result<Self, Error> {
        let index = CaIndex::open(index_dir)?;
        check_manifest(index.manifest(), &index_dir.join(MANIFEST_FILE))?;
        let genome = FastaReader::open(genome_bin)?;
        check_genome(&genome, genome_bin)?;
        Ok(Self {
            index,
            genome,
            found: AtomicU64::new(0),
            not_in_index: AtomicU64::new(0),
            ref_mismatch: AtomicU64::new(0),
            unsupported: AtomicU64::new(0),
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
    /// to stderr, once for the whole process however many `Annotator`s there
    /// are. The warning is best effort: a stderr that cannot be written to
    /// (a closed pipe, a full disk) is ignored.
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
                // Count first, so the tally is right whatever the reporter does.
                count(&self.unsupported);
                report_genome_error(chrom, &error);
                Lookup::without_key(Status::Unsupported)
            }
        }
    }

    /// Look up the CA ID of a VEP-style minimal allele.
    ///
    /// Ensembl/VEP (and the tools built on its conventions) trim the anchor
    /// base off an indel instead of keeping the VCF form. This takes those
    /// alleles as given:
    ///
    /// * `start1` is 1-based. For an insertion it is the position of the base
    ///   *after* the insertion point, so VCF `21:33000166 T>TA` arrives as
    ///   `start1 = 33000167`, REF `-`, ALT `A`. For a deletion it is the first
    ///   deleted base, so `21:33000166 TA>T` arrives as `start1 = 33000167`,
    ///   REF `A`, ALT `-`.
    /// * An allele is empty when it is `-` or zero-length.
    ///
    /// With one side empty, the allele is re-anchored on the reference base at
    /// `start1 - 1` and handed to [`lookup`](Self::lookup) in VCF form, which
    /// yields the same canonical HGVS as the original VCF record. With neither
    /// side empty (an SNV, MNV or replacement) the call is exactly
    /// [`lookup`](Self::lookup).
    ///
    /// The result is [`Status::Unsupported`] when both sides are empty, or
    /// when there is no base to anchor on (`start1` is 0 or 1, the anchor is
    /// past the end of the contig, or the contig is not one the genome file
    /// carries). Like [`lookup`](Self::lookup), each call ends in exactly one
    /// [`Stats`] bucket and never panics.
    pub fn lookup_trimmed(
        &self,
        chrom: &str,
        start1: u64,
        ref_allele: &[u8],
        alt_allele: &[u8],
    ) -> Lookup {
        let ref_empty = is_empty_allele(ref_allele);
        let alt_empty = is_empty_allele(alt_allele);
        if !ref_empty && !alt_empty {
            return self.lookup(chrom, start1, ref_allele, alt_allele);
        }
        if ref_empty && alt_empty {
            return self.unanchored();
        }
        // The anchor is the base before `start1`. Position 0 is not a base, so
        // `start1` of 0 or 1 leaves nothing to anchor on.
        let Some(anchor_pos1) = start1.checked_sub(1).filter(|&pos1| pos1 > 0) else {
            return self.unanchored();
        };
        let Some(anchor) = self.base_at(chrom, anchor_pos1) else {
            return self.unanchored();
        };
        if ref_empty {
            self.lookup(chrom, anchor_pos1, &[anchor], &anchored(anchor, alt_allele))
        } else {
            self.lookup(chrom, anchor_pos1, &anchored(anchor, ref_allele), &[anchor])
        }
    }

    /// A key-less [`Status::Unsupported`] outcome, counted as one.
    fn unanchored(&self) -> Lookup {
        count(&self.unsupported);
        Lookup::without_key(Status::Unsupported)
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

    /// The uppercase reference bases from `start1` to `end1` on `chrom`, both
    /// 1-based and inclusive, as in an HGVS range like `g.33000166_33000172`.
    ///
    /// `None` for `start1` of 0, `start1` after `end1`, a contig name that is
    /// not a GRCh38 primary contig, a contig the genome file does not carry,
    /// or an `end1` past the end of the contig. Like
    /// [`base_at`](Self::base_at) this is a plain genome read: nothing is
    /// counted in [`stats`](Self::stats).
    pub fn ref_seq(&self, chrom: &str, start1: u64, end1: u64) -> Option<Vec<u8>> {
        if start1 == 0 || start1 > end1 {
            return None;
        }
        let contig = resolve_contig(chrom)?;
        // vareffect takes a 0-based, end-exclusive range.
        let mut seq = self
            .genome
            .fetch_sequence(contig.vareffect_name, start1 - 1, end1)
            .ok()?;
        seq.make_ascii_uppercase();
        Some(seq)
    }

    /// Count one allele as [`Status::Unsupported`] without looking it up.
    ///
    /// For adapters that skip an allele before calling
    /// [`lookup`](Self::lookup) or [`lookup_trimmed`](Self::lookup_trimmed),
    /// for example a symbolic or structural ALT, or an input line that does
    /// not pin down one plus-strand allele. Calling this for every such skip
    /// keeps [`stats`](Self::stats) a tally of every allele the run saw, the
    /// same as an adapter that hands those alleles to `lookup` and gets
    /// `Unsupported` back. Like `lookup`, it takes `&self` and is safe to call
    /// from any thread.
    pub fn record_unsupported(&self) {
        count(&self.unsupported);
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
}

fn count(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}

/// True for the VEP spelling of "no bases" (`-`) and for a zero-length slice.
fn is_empty_allele(allele: &[u8]) -> bool {
    allele.is_empty() || allele == b"-"
}

/// `anchor` followed by `allele`: the VCF spelling of a trimmed allele.
fn anchored(anchor: u8, allele: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(allele.len() + 1);
    out.push(anchor);
    out.extend_from_slice(allele);
    out
}

/// Warn on stderr the first time `chrom` fails, ignoring a failed write.
///
/// `eprintln!` would panic on a stderr that errors (EPIPE under `| head`,
/// ENOSPC), and a warning must never take a lookup down.
fn report_genome_error(chrom: &str, error: &Error) {
    let _ = warn_once(&mut io::stderr().lock(), chrom, error);
}

/// Write the one-line warning for `chrom` to `out` if no `Annotator` in this
/// process has reported that contig yet.
fn warn_once(out: &mut impl Write, chrom: &str, error: &Error) -> io::Result<()> {
    // Key on the canonical name so `1` and `chr1` share one warning;
    // `canonical_hgvs` only reads the genome for a resolvable contig, but
    // fall back to the caller's spelling rather than assume that.
    let contig = resolve_contig(chrom).map_or(chrom, |c| c.vareffect_name);
    if !first_failure(contig) {
        return Ok(());
    }
    writeln!(
        out,
        "clingen-allele: genome lookup failed for contig '{contig}' ({error}); \
         variants on this contig are reported Unsupported"
    )
}

/// True the first time `contig` is passed in, in this process.
fn first_failure(contig: &str) -> bool {
    // A poisoned set is still a usable set; the lookup path must not panic.
    let mut seen = FAILED_CONTIGS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    !seen.contains(contig) && seen.insert(contig.to_owned())
}

/// Reject an index this crate cannot interpret.
fn check_manifest(manifest: &Manifest, path: &Path) -> Result<(), Error> {
    if manifest.format_version != FORMAT_VERSION {
        return Err(Error::Manifest(format!(
            "{}: format_version is {}, but only {FORMAT_VERSION} is supported",
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

/// Reject a genome that is not GRCh38, judged by its primary contigs' lengths.
///
/// A wrong-build genome would otherwise pass the REF check of variants on
/// the same build and key them at the wrong GRCh38 position.
fn check_genome(genome: &FastaReader, path: &Path) -> Result<(), Error> {
    let not_grch38 = |reason: String| Error::GenomeNotGrch38 {
        path: path.to_path_buf(),
        reason,
    };
    let mut present = 0;
    for contig in primary_contigs() {
        let Some(found) = genome.chrom_length(contig.vareffect_name) else {
            continue;
        };
        if found != contig.length {
            return Err(not_grch38(format!(
                "contig {name} is {found} bases long; GRCh38 {name} is {expected}",
                name = contig.vareffect_name,
                expected = contig.length,
            )));
        }
        present += 1;
    }
    if present == 0 {
        return Err(not_grch38(
            "it has none of the 25 GRCh38 primary contigs (chr1-chr22, chrX, chrY, chrM)"
                .to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn genome_error() -> Error {
        Error::Manifest("stand-in for a genome read fault".into())
    }

    /// Every unit test here uses contig names no other test touches, because
    /// the dedupe set is shared by the whole test binary.
    #[test]
    fn warning_is_written_once_per_contig_per_process() {
        let mut out = Vec::new();
        // `Y` and `chrY` are the same contig: one line between them, however
        // many callers (or Annotators) ask.
        warn_once(&mut out, "Y", &genome_error()).unwrap();
        warn_once(&mut out, "chrY", &genome_error()).unwrap();
        warn_once(&mut out, "Y", &genome_error()).unwrap();
        // A contig that is not a primary one is keyed by the caller's spelling.
        warn_once(&mut out, "unit-test-contig", &genome_error()).unwrap();
        warn_once(&mut out, "unit-test-contig", &genome_error()).unwrap();

        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[0].contains("contig 'chrY'"), "{text}");
        assert!(lines[1].contains("contig 'unit-test-contig'"), "{text}");
        assert!(lines[0].ends_with("variants on this contig are reported Unsupported"));
    }

    struct BrokenPipe;

    impl Write for BrokenPipe {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }

    #[test]
    fn unwritable_stderr_is_an_error_not_a_panic() {
        let result = warn_once(&mut BrokenPipe, "unit-test-broken-pipe", &genome_error());
        assert!(result.is_err());
        // The contig counts as reported even though the write failed, so a
        // dead stderr is not retried for every variant.
        assert!(!first_failure("unit-test-broken-pipe"));
    }
}
