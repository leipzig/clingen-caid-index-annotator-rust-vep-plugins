//! The on-disk CA-ID index: an `fst` map from Registry HGVS to CA ID, plus a
//! `manifest.json` describing how it was built.
//!
//! An index directory holds two files:
//!
//! * `caid.fst` maps each GRCh38 `NC_` HGVS string to its CA ID (the number
//!   after the `CA` prefix) and is memory-mapped on open.
//! * `manifest.json` records provenance (see [`Manifest`]). It is written
//!   last, so its presence implies a complete `caid.fst`.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use fst::MapBuilder;
use memmap2::Mmap;
use serde::{Deserialize, Serialize};

use crate::Error;

const FST_FILE: &str = "caid.fst";
const MANIFEST_FILE: &str = "manifest.json";

/// The index layout version [`IndexBuilder::finish`] writes.
const FORMAT_VERSION: u32 = 1;

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
    /// Version of vareffect that normalised the harvested variants.
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
    /// # Errors
    ///
    /// [`Error::DuplicateConflict`] if a key was added with two different IDs
    /// (the IDs are reported in ascending order); nothing is written in that
    /// case. Otherwise [`Error::Io`], [`Error::Fst`] or [`Error::Manifest`]
    /// if writing fails.
    pub fn finish(self, dir: &Path, mut manifest: Manifest) -> Result<u64, Error> {
        let entries = dedup_sorted(self.entries)?;
        let count = entries.len() as u64;
        manifest.entries = count;
        manifest.format_version = FORMAT_VERSION;

        fs::create_dir_all(dir).map_err(io_error(dir))?;

        // A manifest left by an earlier build must not outlive a failed rebuild
        // of the FST it describes.
        let manifest_path = dir.join(MANIFEST_FILE);
        match fs::remove_file(&manifest_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(&manifest_path)(e)),
        }

        write_fst(&dir.join(FST_FILE), &entries)?;
        write_manifest(&manifest_path, &manifest)?;
        Ok(count)
    }
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

/// Stream strictly ascending keys into an FST at `path` and sync it to disk.
fn write_fst(path: &Path, entries: &[(String, u64)]) -> Result<(), Error> {
    let file = File::create(path).map_err(io_error(path))?;
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
    let mut file = File::create(path).map_err(io_error(path))?;
    file.write_all(json.as_bytes()).map_err(io_error(path))?;
    file.write_all(b"\n").map_err(io_error(path))?;
    file.sync_all().map_err(io_error(path))
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
    /// (assembly, format version); that is up to the caller.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] naming the file that could not be read or mapped,
    /// [`Error::Manifest`] if `manifest.json` is not valid, and [`Error::Fst`]
    /// if `caid.fst` is not a valid FST.
    pub fn open(dir: &Path) -> Result<Self, Error> {
        let manifest_path = dir.join(MANIFEST_FILE);
        let json = fs::read_to_string(&manifest_path).map_err(io_error(&manifest_path))?;
        let manifest: Manifest = serde_json::from_str(&json)
            .map_err(|e| Error::Manifest(format!("{}: {e}", manifest_path.display())))?;

        let fst_path = dir.join(FST_FILE);
        let file = File::open(&fst_path).map_err(io_error(&fst_path))?;
        // SAFETY: `caid.fst` is only written by `IndexBuilder::finish`, before
        // the manifest. As with any memory map, truncating the file while it is
        // mapped (rebuilding an index in place under a running reader) is
        // undefined behaviour; index directories are meant to be immutable.
        let mmap = unsafe { Mmap::map(&file) }.map_err(io_error(&fst_path))?;
        let map = fst::Map::new(mmap)?;

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
