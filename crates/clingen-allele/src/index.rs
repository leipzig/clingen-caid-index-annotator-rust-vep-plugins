//! The on-disk CA-ID index: an `fst` map from Registry HGVS to CA ID, plus a
//! `manifest.json` describing how it was built.
//!
//! An index directory holds two files:
//!
//! * `caid.fst` maps each GRCh38 `NC_` HGVS string to its CA ID (the number
//!   after the `CA` prefix) and is memory-mapped on open.
//! * `manifest.json` records provenance (see [`Manifest`]).
//!
//! [`SortedIndexWriter`] streams pairs that already arrive in byte order into
//! an index, in memory that does not grow with their number. [`IndexBuilder`]
//! takes pairs in any order, sorts them in memory and writes through a
//! `SortedIndexWriter`, so there is one write path.
//! [`SortedIndexWriter::finish`] writes each file under a temporary name and
//! renames it into place, `caid.fst` first, so `manifest.json` is only ever
//! present next to a complete `caid.fst`. A rebuild therefore never rewrites a
//! file a running reader has mapped, and one that fails before the first
//! rename leaves the previous index as it was. A failure between the two
//! renames leaves the new `caid.fst` beside the old manifest (see
//! [`SortedIndexWriter::finish`]).

use std::cmp::Ordering;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, ErrorKind, Write};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use fst::{IntoStreamer, MapBuilder, Streamer};
use memmap2::Mmap;
use serde::{Deserialize, Serialize};

use crate::Error;

pub(crate) const FST_FILE: &str = "caid.fst";
pub(crate) const MANIFEST_FILE: &str = "manifest.json";

/// Where a [`SortedIndexWriter`] writes each file before renaming it into
/// place. A crashed build can leave these behind; the next build replaces them.
const FST_TMP_FILE: &str = "caid.fst.tmp";
const MANIFEST_TMP_FILE: &str = "manifest.json.tmp";

/// The index layout version [`SortedIndexWriter::finish`] writes, and the
/// only one [`Annotator::open`](crate::Annotator::open) accepts.
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
/// Everything is held in memory until [`finish`](Self::finish), which suits a
/// single chromosome's harvest. Pairs too many to hold can be sorted on disk
/// instead and streamed through a [`SortedIndexWriter`].
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
    /// The pairs are sorted, exact repeats dropped, and the rest streamed
    /// through a [`SortedIndexWriter`], so `manifest.entries` and
    /// `manifest.format_version` are overwritten with the real count and the
    /// version written (callers may pass any placeholder for them), and an
    /// index already in `dir` is replaced safely as
    /// [`SortedIndexWriter::finish`] describes. If writing fails before the
    /// renames, the previous index is left as it was and the temporary files
    /// are removed (where they can be).
    ///
    /// # Errors
    ///
    /// [`Error::DuplicateConflict`] if a key was added with two different IDs
    /// (the IDs are reported in ascending order); nothing is written in that
    /// case. Otherwise [`Error::Io`], [`Error::Fst`] or [`Error::Manifest`]
    /// if writing fails. An error syncing `dir` after the last rename means
    /// the new index is in place but may not survive a power loss.
    pub fn finish(self, dir: &Path, manifest: Manifest) -> Result<u64, Error> {
        // Checking for conflicts before the writer exists keeps a conflicting
        // build from creating `dir` or any file in it.
        let entries = dedup_sorted(self.entries)?;
        let mut writer = SortedIndexWriter::create(dir)?;
        for (hgvs, caid) in &entries {
            writer.push(hgvs, *caid)?;
        }
        writer.finish(manifest)
    }
}

/// Streams `(HGVS, CA ID)` pairs that arrive in byte order into a new index
/// directory, in memory that does not grow with the number of pairs.
///
/// This is how a harvest too big for [`IndexBuilder`] is indexed: sort the
/// pairs on disk first (`LC_ALL=C sort` gives byte order) and push them
/// here. `IndexBuilder` writes through one of these too, so both produce the
/// same files.
///
/// Until [`finish`](Self::finish) succeeds, the pairs only reach
/// `caid.fst.tmp`, and an index already in the directory keeps serving. A
/// writer dropped before then, or one that returns any error, removes both
/// temporary files. After an error every later [`push`](Self::push) and
/// `finish` fails too, so a build missing a pair can never replace the index.
pub struct SortedIndexWriter {
    dir: PathBuf,
    /// The FST under construction in `caid.fst.tmp`; `None` once `finish` has
    /// taken it or an error has discarded the build.
    builder: Option<MapBuilder<BufWriter<File>>>,
    /// The last pair added, which the next key must not sort below.
    previous: Option<(String, u64)>,
    /// Distinct keys added so far.
    entries: u64,
}

impl SortedIndexWriter {
    /// Start a build in `dir`, creating the directory if needed.
    ///
    /// Opens `caid.fst.tmp` there, replacing any a crashed build left. An
    /// index already in `dir` is not touched until [`finish`](Self::finish).
    ///
    /// # Errors
    ///
    /// [`Error::Io`] naming `dir` or `caid.fst.tmp` if either cannot be
    /// created, and [`Error::Fst`] if the FST header cannot be written.
    pub fn create(dir: &Path) -> Result<Self, Error> {
        fs::create_dir_all(dir).map_err(io_error(dir))?;
        let builder = open_fst(&dir.join(FST_TMP_FILE)).inspect_err(|_| remove_tmp_files(dir))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            builder: Some(builder),
            previous: None,
            entries: 0,
        })
    }

    /// Add `hgvs` as allele `CA{caid}`.
    ///
    /// Keys must come in byte order, the order the FST is laid out in and the
    /// one `LC_ALL=C sort` produces; a key sorts before every longer key it is
    /// a prefix of (`…g.12del` before `…g.12delinsA`). Returns `Ok(true)` when
    /// the pair is added, and `Ok(false)` when it repeats the previous pair
    /// exactly and is skipped, so sorted input need not be deduplicated.
    ///
    /// # Errors
    ///
    /// [`Error::Unsorted`] if `hgvs` sorts below the previous key, and
    /// [`Error::DuplicateConflict`] if it is the previous key with another ID
    /// (`first` is the previous ID, `second` this one). [`Error::Fst`] if
    /// writing the FST fails, and [`Error::Io`] naming `caid.fst.tmp` if an
    /// earlier error already discarded the build. Any error discards the
    /// build: both temporary files are removed, and an index already in the
    /// directory stays as it was.
    pub fn push(&mut self, hgvs: &str, caid: u64) -> Result<bool, Error> {
        let pushed = self.try_push(hgvs, caid);
        if pushed.is_err() {
            self.discard();
        }
        pushed
    }

    /// [`push`](Self::push), short of discarding the build on an error.
    fn try_push(&mut self, hgvs: &str, caid: u64) -> Result<bool, Error> {
        let builder = self.builder.as_mut().ok_or_else(|| discarded(&self.dir))?;
        if let Some((previous, previous_caid)) = &self.previous {
            match hgvs.as_bytes().cmp(previous.as_bytes()) {
                Ordering::Less => {
                    return Err(Error::Unsorted {
                        previous: previous.clone(),
                        key: hgvs.to_owned(),
                    });
                }
                Ordering::Equal if caid == *previous_caid => return Ok(false),
                Ordering::Equal => {
                    return Err(Error::DuplicateConflict {
                        hgvs: hgvs.to_owned(),
                        first: *previous_caid,
                        second: caid,
                    });
                }
                Ordering::Greater => {}
            }
        }
        builder.insert(hgvs, caid)?;
        // Reuse the previous key's buffer: one allocation per build, not per key.
        let (previous, previous_caid) = self.previous.get_or_insert_with(Default::default);
        previous.clear();
        previous.push_str(hgvs);
        *previous_caid = caid;
        self.entries += 1;
        Ok(true)
    }

    /// Complete `caid.fst`, write `manifest.json`, and swap both into place
    /// in the directory; return the number of distinct keys indexed.
    ///
    /// `manifest.entries` and `manifest.format_version` are overwritten with
    /// the real count and the version written, so callers may pass any
    /// placeholder for them.
    ///
    /// The directory may already hold an index; it is replaced safely:
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
    /// [`Error::Io`], [`Error::Fst`] or [`Error::Manifest`] if writing fails,
    /// and [`Error::Io`] naming `caid.fst.tmp` if an earlier error discarded
    /// the build. An error syncing the directory after the last rename means
    /// the new index is in place but may not survive a power loss.
    pub fn finish(mut self, mut manifest: Manifest) -> Result<u64, Error> {
        let builder = self.builder.take().ok_or_else(|| discarded(&self.dir))?;
        manifest.entries = self.entries;
        manifest.format_version = FORMAT_VERSION;
        let written = write_and_swap(&self.dir, builder, &manifest);
        if written.is_err() {
            // Best effort: whatever was not renamed into place goes. A file
            // already renamed, or a directory squatting on the name, makes
            // this fail harmlessly.
            remove_tmp_files(&self.dir);
        }
        written.map(|()| self.entries)
    }

    /// Abandon the build, if it is still live: close `caid.fst.tmp` (some
    /// platforms will not remove an open file) and remove both temporary
    /// files.
    fn discard(&mut self) {
        if let Some(builder) = self.builder.take() {
            drop(builder);
            remove_tmp_files(&self.dir);
        }
    }
}

impl Drop for SortedIndexWriter {
    fn drop(&mut self) {
        // A writer dropped before `finish` (on an early return, say) leaves
        // nothing behind and the existing index in place.
        self.discard();
    }
}

impl fmt::Debug for SortedIndexWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `fst::MapBuilder` is not `Debug`; show where the build goes and how
        // far it has got.
        f.debug_struct("SortedIndexWriter")
            .field("dir", &self.dir)
            .field("entries", &self.entries)
            .field("previous", &self.previous)
            .finish_non_exhaustive()
    }
}

/// The error for using a [`SortedIndexWriter`] after an error discarded its
/// build: `caid.fst.tmp` is gone, so nothing more can be written.
fn discarded(dir: &Path) -> Error {
    Error::Io {
        path: dir.join(FST_TMP_FILE),
        source: std::io::Error::other("an earlier error discarded this index build"),
    }
}

/// Finish the FST, write the manifest, then rename both into place in `dir`:
/// `caid.fst` first, so a manifest is never newer than the FST next to it.
fn write_and_swap(
    dir: &Path,
    builder: MapBuilder<BufWriter<File>>,
    manifest: &Manifest,
) -> Result<(), Error> {
    let fst_tmp = dir.join(FST_TMP_FILE);
    let manifest_tmp = dir.join(MANIFEST_TMP_FILE);
    finish_fst(&fst_tmp, builder)?;
    write_manifest(&manifest_tmp, manifest)?;
    replace(&fst_tmp, &dir.join(FST_FILE))?;
    sync_dir(dir)?;
    replace(&manifest_tmp, &dir.join(MANIFEST_FILE))?;
    sync_dir(dir)
}

/// Best effort: remove both temporary files from `dir`. One that is missing,
/// or a directory squatting on the name, is left as it is.
fn remove_tmp_files(dir: &Path) {
    let _ = fs::remove_file(dir.join(FST_TMP_FILE));
    let _ = fs::remove_file(dir.join(MANIFEST_TMP_FILE));
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

/// A new FST builder writing to the temporary file at `path`.
fn open_fst(path: &Path) -> Result<MapBuilder<BufWriter<File>>, Error> {
    let file = create_tmp(path)?;
    Ok(MapBuilder::new(BufWriter::new(file))?)
}

/// Write out the rest of the FST `builder` is writing to `path`, and sync the
/// file to disk.
fn finish_fst(path: &Path, builder: MapBuilder<BufWriter<File>>) -> Result<(), Error> {
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
    /// Open an index directory written by a [`SortedIndexWriter`] (or an
    /// [`IndexBuilder`], which writes through one).
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
        // SAFETY: `SortedIndexWriter::finish` never writes to an existing
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

    /// Call `visit` with each key that starts with `prefix`, and its CA ID,
    /// in byte order, until `visit` returns [`ControlFlow::Break`].
    ///
    /// An empty `prefix` visits every key. Keys sharing a prefix sit together
    /// in the FST, so the walk starts at the first of them and stops after the
    /// last: a prefix such as `NC_000021.9:` costs only that contig's keys.
    /// A visitor rather than an iterator because the FST lends each key from
    /// a buffer it reuses for the next.
    pub fn visit_prefix(
        &self,
        prefix: &[u8],
        mut visit: impl FnMut(&[u8], u64) -> ControlFlow<()>,
    ) {
        let mut stream = self.map.range().ge(prefix).into_stream();
        while let Some((key, caid)) = stream.next() {
            if !key.starts_with(prefix) || visit(key, caid).is_break() {
                break;
            }
        }
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

/// Format a CA ID number the way the Registry prints it: `CA` and the number
/// zero-padded to at least six digits (`CA068622`, `CA1019413128`).
///
/// Of the 15,528,855 IDs harvested from gnomAD chr21 and ClinVar, every one
/// has at least six digits, and only six-digit ones start with `0`, so this
/// reproduces each of them from its number.
pub fn format_caid(n: u64) -> String {
    format!("CA{n:06}")
}
