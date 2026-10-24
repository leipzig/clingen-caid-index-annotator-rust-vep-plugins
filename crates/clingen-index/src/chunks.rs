//! Reading the harvester's chunk files.
//!
//! Every subcommand that consumes chunks goes through [`chunk_files`] and
//! [`read_rows`], so they agree on which files count, how rows are split, and
//! what a malformed row is.

use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

/// How many malformed rows to show on stderr (all are counted).
const MALFORMED_EXAMPLES: usize = 5;

/// The first six columns of a chunk row. `HGVS_GRCh37` is never needed.
pub(crate) struct Row<'a> {
    pub(crate) chrom: &'a str,
    pub(crate) pos: &'a str,
    pub(crate) ref_allele: &'a str,
    pub(crate) alt_allele: &'a str,
    pub(crate) caid: &'a str,
    pub(crate) hgvs38: &'a str,
}

impl<'a> Row<'a> {
    /// `None` when the line has fewer than six tab-separated columns.
    fn parse(line: &'a str) -> Option<Self> {
        let mut cols = line.split('\t');
        Some(Row {
            chrom: cols.next()?,
            pos: cols.next()?,
            ref_allele: cols.next()?,
            alt_allele: cols.next()?,
            caid: cols.next()?,
            hgvs38: cols.next()?,
        })
    }
}

/// What a row visitor wants the reader to do next.
pub(crate) enum Verdict {
    /// Go on to the next row.
    Continue,
    /// Stop reading: the visitor has seen enough rows.
    Stop,
    /// The row is unusable for the visitor; count it and say why.
    Malformed(&'static str),
}

/// Malformed rows seen while reading, with the first few for the report.
#[derive(Default)]
pub(crate) struct Malformed {
    pub(crate) count: u64,
    examples: Vec<String>,
}

impl Malformed {
    fn note(&mut self, file: &Path, line_no: u64, reason: &str) {
        self.count += 1;
        if self.examples.len() < MALFORMED_EXAMPLES {
            self.examples
                .push(format!("{}:{line_no}: {reason}", file.display()));
        }
    }

    /// Print the first few malformed rows, and how many more there were, as
    /// warnings on stderr.
    pub(crate) fn report(&self) {
        for example in &self.examples {
            eprintln!("warning: malformed row at {example}");
        }
        let hidden = self.count - self.examples.len() as u64;
        if hidden > 0 {
            eprintln!("warning: ... and {hidden} more malformed rows");
        }
    }
}

/// Every `chunk_*.tsv` in each of `dirs`: directories in the order given,
/// files within a directory in filename order. Other files (`*.partial`, ...)
/// are ignored.
pub(crate) fn chunk_files(dirs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for dir in dirs {
        let entries = fs::read_dir(dir)
            .with_context(|| format!("reading chunk directory {}", dir.display()))?;
        let mut found = Vec::new();
        for entry in entries {
            let entry =
                entry.with_context(|| format!("reading chunk directory {}", dir.display()))?;
            let name = entry.file_name();
            let is_chunk = name
                .to_str()
                .is_some_and(|n| n.starts_with("chunk_") && n.ends_with(".tsv"));
            if is_chunk && entry.path().is_file() {
                found.push(entry.path());
            }
        }
        // A mistyped directory would otherwise build an empty index.
        ensure!(
            !found.is_empty(),
            "no chunk_*.tsv files in {}",
            dir.display()
        );
        found.sort();
        files.extend(found);
    }
    Ok(files)
}

/// What [`read_rows`] saw.
pub(crate) struct ReadSummary {
    /// Non-blank lines read, malformed ones included.
    pub(crate) rows: u64,
    pub(crate) malformed: Malformed,
}

/// Stream every row of `files` through `visit`, without holding a file in
/// memory. Blank lines are skipped, a trailing `\r` is tolerated, and a line
/// that is not UTF-8 or has fewer than six columns is counted as malformed.
pub(crate) fn read_rows(
    files: &[PathBuf],
    mut visit: impl FnMut(&Row) -> Result<Verdict>,
) -> Result<ReadSummary> {
    let mut summary = ReadSummary {
        rows: 0,
        malformed: Malformed::default(),
    };
    let mut buf = Vec::new();
    for path in files {
        let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut reader = BufReader::with_capacity(1 << 20, file);
        let mut line_no = 0u64;
        loop {
            buf.clear();
            let n = reader
                .read_until(b'\n', &mut buf)
                .with_context(|| format!("reading {}", path.display()))?;
            if n == 0 {
                break;
            }
            line_no += 1;
            let line = buf.strip_suffix(b"\n").unwrap_or(&buf);
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            summary.rows += 1;
            let Ok(line) = std::str::from_utf8(line) else {
                summary.malformed.note(path, line_no, "not valid UTF-8");
                continue;
            };
            let Some(row) = Row::parse(line) else {
                summary
                    .malformed
                    .note(path, line_no, "fewer than 6 tab-separated columns");
                continue;
            };
            match visit(&row)? {
                Verdict::Continue => {}
                Verdict::Stop => return Ok(summary),
                Verdict::Malformed(reason) => summary.malformed.note(path, line_no, reason),
            }
        }
    }
    Ok(summary)
}
