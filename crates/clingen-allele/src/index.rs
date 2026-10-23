//! The on-disk CA-ID index: an `fst` map from Registry HGVS to CA ID, plus a
//! `manifest.json` describing how it was built.
//!
//! An index directory holds two files:
//!
//! * `caid.fst` maps each GRCh38 `NC_` HGVS string to its CA ID (the number
//!   after the `CA` prefix) and is memory-mapped on open.
//! * `manifest.json` records provenance (see [`Manifest`]).
//!
//! [`IndexBuilder::finish`] writes each file under a temporary name and
//! renames it into place, `caid.fst` first, so `manifest.json` is only ever
//! present next to a complete `caid.fst`. A rebuild therefore never rewrites a
//! file a running reader has mapped, and one that fails before the first
//! rename leaves the previous index as it was. A failure between the two
//! renames leaves the new `caid.fst` beside the old manifest (see
//! [`IndexBuilder::finish`]).

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, ErrorKind, Write};
use std::path::Path;

use fst::MapBuilder;
use memmap2::Mmap;
use serde::{Deserialize, Serialize};

use crate::Error;

pub(crate) const FST_FILE: &str = "caid.fst";
pub(crate) const MANIFEST_FILE: &str = "manifest.json";

/// Where [`IndexBuilder::finish`] writes each file before renaming it into
/// place. A crashed build can leave these behind; the next build replaces them.
const FST_TMP_FILE: &str = "caid.fst.tmp";
const MANIFEST_TMP_FILE: &str = "manifest.json.tmp";

/// The index layout version [`IndexBuilder::finish`] writes, and the only one
/// [`Annotator::open`](crate::Annotator::open) accepts.
pub(crate) const FORMAT_VERSION: u32 = 1;

/// Build provenance stored next to the index as `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Layout version of the index directory; `1` is the only one so far.
    pub format_version: u32,
    /// Reference assembly the HGVS keys are on (`GRCh38`).
    pub assembly: String,
    /// Where the harvested alleles came from.
    pub sources: Vec<String>,
    /// Date the Registry was queried, as written by the harvester.
    pub harvest_date: String,
    /// Number of distinct HGVS keys in `caid.fst`.
    pub entries: u64,
    /// The vareffect release whose canonical HGVS the keys are meant to
    /// match (the keys themselves are the Registry's strings). Informational:
    /// nothing checks it at open.
    pub vareffect_version: String,
    /// Identifies the reference genome the index was built against, if known.
    #[serde(default)]
    pub genome: Option<String>,
}

/// Collects `(HGVS, CA ID)` pairs and writes them out as an index directory.
///
/// Everything is held in memory until [`finish`](Self::finish); a single
/// chromosome's harvest fits comfortably, and an external sort can wait until
/// a genome-wide build needs one.
#[derive(Debug, Default)]
pub struct IndexBuilder {
    entries: Vec<(String, u64)>,
}

impl IndexBuilder {
    /// An empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `hgvs` is allele `CA{caid}`. Keys may be added in any
    /// order and more than once, provided repeats carry the same ID.
    pub fn add(&mut self, hgvs: &str, caid: u64) {
        self.entries.push((hgvs.to_owned(), caid));
    }

    /// Write `caid.fst` and `manifest.json` into `dir`, creating it if needed,
    /// and return the number of distinct keys indexed.
    ///
    /// `manifest.entries` and `manifest.format_version` are overwritten with
    /// the real count and the version written, so callers may pass any
    /// placeholder for them.
    ///
    /// `dir` may already hold an index; it is replaced safely:
    ///
    /// * Both files are written and synced as `caid.fst.tmp` and
    ///   `manifest.json.tmp` (replacing any a crashed build left), then
    ///   renamed over `caid.fst` and then `manifest.json`, with the directory
    ///   synced after each rename.
    /// * A [`CaIndex`] that is already open keeps reading the old file, which
    ///   a rename does not touch; a fresh [`CaIndex::open`] sees the new one.
    /// * A reader that opens between the two renames finds the new
    ///   `caid.fst` with the old manifest. [`CaIndex::open`] rejects that pair
    ///   when their entry counts differ; when they agree, the reader gets the
    ///   complete new FST with the old provenance.
    /// * If writing fails before the renames, the previous index is left as it
    ///   was and the temporary files are removed (where they can be).
    ///
    /// # Errors
    ///
    /// [`Error::DuplicateConflict`] if a key was added with two different IDs
    /// (the IDs are reported in ascending order); nothing is written in that
    /// case. Otherwise [`Error::Io`], [`Error::Fst`] or [`Error::Manifest`]
    /// if writing fails. An error syncing `dir` after the last rename means
    /// the new index is in place but may not survive a power loss.
    pub fn finish(self, dir: &Path, mut manifest: Manifest) -> Result<u64, Error> {
        let entries = dedup_sorted(self.entries)?;
        let count = entries.len() as u64;
        manifest.entries = count;
        manifest.format_version = FORMAT_VERSION;

        fs::create_dir_all(dir).map_err(io_error(dir))?;
        let written = write_and_swap(dir, &entries, &manifest);
        if written.is_err() {
            // Best effort: whatever was not renamed into place goes. A file
            // already renamed, or a directory squatting on the name, makes
            // this fail harmlessly.
            let _ = fs::remove_file(dir.join(FST_TMP_FILE));
            let _ = fs::remove_file(dir.join(MANIFEST_TMP_FILE));
        }
        written.map(|()| count)
    }
}

/// Write both files under their temporary names, then rename them into place
/// in `dir`: `caid.fst` first, so a manifest is never newer than the FST next
/// to it.
fn write_and_swap(dir: &Path, entries: &[(String, u64)], manifest: &Manifest) -> Result<(), Error> {
    let fst_tmp = dir.join(FST_TMP_FILE);
    let manifest_tmp = dir.join(MANIFEST_TMP_FILE);
    write_fst(&fst_tmp, entries)?;
    write_manifest(&manifest_tmp, manifest)?;
    replace(&fst_tmp, &dir.join(FST_FILE))?;
    sync_dir(dir)?;
    replace(&manifest_tmp, &dir.join(MANIFEST_FILE))?;
    sync_dir(dir)
}

/// Sort by key, drop exact repeats, and fail on a key with two IDs.
fn dedup_sorted(mut entries: Vec<(String, u64)>) -> Result<Vec<(String, u64)>, Error> {
    // Sorting whole tuples (key, then ID) makes the conflict report
    // independent of the order the pairs were added in.
    entries.sort_unstable();
    entries.dedup();
    if let Some(pair) = entries.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        return Err(Error::DuplicateConflict {
            hgvs: pair[0].0.clone(),
            first: pair[0].1,
            second: pair[1].1,
        });
    }
    Ok(entries)
}

/// A new, empty file at the temporary path `path`, replacing whatever an
/// earlier build left there. Unlinking first (rather than truncating) means a
/// stale symlink is replaced, not followed.
fn create_tmp(path: &Path) -> Result<File, Error> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(io_error(path)(e)),
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io_error(path))
}

/// Stream strictly ascending keys into an FST at `path` and sync it to disk.
fn write_fst(path: &Path, entries: &[(String, u64)]) -> Result<(), Error> {
    let file = create_tmp(path)?;
    let mut builder = MapBuilder::new(BufWriter::new(file))?;
    for (hgvs, caid) in entries {
        builder.insert(hgvs, *caid)?;
    }
    let mut writer = builder.into_inner()?;
    writer.flush().map_err(io_error(path))?;
    writer.get_ref().sync_all().map_err(io_error(path))
}

fn write_manifest(path: &Path, manifest: &Manifest) -> Result<(), Error> {
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|e| Error::Manifest(format!("cannot serialise {}: {e}", path.display())))?;
    let mut file = create_tmp(path)?;
    file.write_all(json.as_bytes()).map_err(io_error(path))?;
    file.write_all(b"\n").map_err(io_error(path))?;
    file.sync_all().map_err(io_error(path))
}

/// Atomically rename `tmp` over `dest`; the error names `dest`.
fn replace(tmp: &Path, dest: &Path) -> Result<(), Error> {
    fs::rename(tmp, dest).map_err(io_error(dest))
}

/// Sync `dir` so the renames in it survive a crash.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> Result<(), Error> {
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(io_error(dir))
}

/// Directories cannot be opened as files here; the renames are left to the OS.
#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> Result<(), Error> {
    Ok(())
}

/// `map_err` adapter attaching the offending path to an I/O error.
fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// A memory-mapped HGVS to CA ID lookup table.
#[derive(Debug)]
pub struct CaIndex {
    map: fst::Map<Mmap>,
    manifest: Manifest,
}

impl CaIndex {
    /// Open the index directory written by [`IndexBuilder::finish`].
    ///
    /// The manifest is parsed but not checked against what the caller expects
    /// (assembly, format version); that is up to the caller. The FST's key
    /// count is checked against `manifest.entries`, which catches a truncated
    /// `caid.fst` or one paired with another build's manifest at open time
    /// rather than as a panic on the first lookup.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] naming the file that could not be read or mapped,
    /// [`Error::Manifest`] if `manifest.json` is not valid or lists a
    /// different number of entries than `caid.fst` holds, and [`Error::Fst`]
    /// if `caid.fst` is not a valid FST.
    pub fn open(dir: &Path) -> Result<Self, Error> {
        let manifest_path = dir.join(MANIFEST_FILE);
        let json = fs::read_to_string(&manifest_path).map_err(io_error(&manifest_path))?;
        let manifest: Manifest = serde_json::from_str(&json)
            .map_err(|e| Error::Manifest(format!("{}: {e}", manifest_path.display())))?;

        let fst_path = dir.join(FST_FILE);
        let file = File::open(&fst_path).map_err(io_error(&fst_path))?;
        // SAFETY: `IndexBuilder::finish` never writes to an existing
        // `caid.fst`: it renames a new file over it, which leaves this mapped
        // inode unchanged. Truncating or rewriting the file in place by other
        // means while it is mapped would be undefined behaviour.
        let mmap = unsafe { Mmap::map(&file) }.map_err(io_error(&fst_path))?;
        let map = fst::Map::new(mmap)?;

        // `fst::Map::new` only checks the header, so a truncated file can still
        // open and then panic in `get`. The key count sits in the last bytes of
        // the file, so a cut file reads back garbage here; comparing it with
        // the manifest is an O(1) way to reject it.
        if map.len() as u64 != manifest.entries {
            return Err(Error::Manifest(format!(
                "{}: lists {} entries but {} contains {}",
                manifest_path.display(),
                manifest.entries,
                fst_path.display(),
                map.len()
            )));
        }

        Ok(Self { map, manifest })
    }

    /// The CA ID recorded for exactly this HGVS string, if any.
    pub fn get(&self, hgvs: &str) -> Option<u64> {
        self.map.get(hgvs)
    }

    /// The manifest stored alongside the index.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
}

/// Parse a Registry allele ID such as `CA749862939` into `749862939`.
///
/// Accepts `CA` followed by one or more ASCII digits that fit in a `u64`.
/// Anything else (an empty string, `_:CA` placeholders, `ERROR:...` messages,
/// lowercase `ca1`, signs or whitespace) is `None`.
pub fn parse_caid(s: &str) -> Option<u64> {
    let digits = s.strip_prefix("CA")?;
    // `u64::from_str` would also take a leading `+`.
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Format a CA ID number the way the Registry prints it: `CA{n}`.
pub fn format_caid(n: u64) -> String {
    format!("CA{n}")
}
