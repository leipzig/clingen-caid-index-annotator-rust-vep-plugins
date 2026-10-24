//! Opening an input that may be gzipped, or standard input.
//!
//! `genome --fasta` and `build --sorted` both take a path or `-`, plain or
//! gzip/BGZF, and tell which by the data's first bytes rather than its name,
//! so a pipe or a misnamed file reads the same as a `.gz`.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Cursor, Read};
use std::path::Path;

use anyhow::{Context, Result};
use flate2::read::MultiGzDecoder;

/// The two bytes every gzip member, and so every BGZF block, starts with.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// What the input is called in error messages.
pub(crate) fn input_label(path: &Path) -> String {
    if is_stdin(path) {
        "standard input".to_owned()
    } else {
        path.display().to_string()
    }
}

/// Whether `path` is `-`, which names standard input.
pub(crate) fn is_stdin(path: &Path) -> bool {
    path == Path::new("-")
}

/// Open `path` (`-` for standard input) for line reading, undoing gzip when
/// the data starts with its magic.
///
/// A `.gz` may be BGZF, as `bgzip` and ClinVar's files are: many gzip members
/// back to back, which `GzDecoder` would read only the first of, so
/// `MultiGzDecoder` it is. The two magic bytes are read off the front and put
/// back, since standard input cannot be rewound.
pub(crate) fn open_input(path: &Path) -> Result<Box<dyn BufRead>> {
    let mut source: Box<dyn Read> = if is_stdin(path) {
        Box::new(io::stdin().lock())
    } else {
        Box::new(File::open(path).with_context(|| format!("cannot open {}", path.display()))?)
    };

    // `take` + `read_to_end` keeps reading until two bytes or end of input; a
    // pipe may hand over its first byte alone.
    let mut head = Vec::with_capacity(GZIP_MAGIC.len());
    source
        .by_ref()
        .take(GZIP_MAGIC.len() as u64)
        .read_to_end(&mut head)
        .with_context(|| format!("reading {}", input_label(path)))?;
    let gzipped = head == GZIP_MAGIC;
    let whole = Cursor::new(head).chain(source);

    let bytes: Box<dyn Read> = if gzipped {
        Box::new(MultiGzDecoder::new(whole))
    } else {
        Box::new(whole)
    };
    Ok(Box::new(BufReader::with_capacity(1 << 20, bytes)))
}
