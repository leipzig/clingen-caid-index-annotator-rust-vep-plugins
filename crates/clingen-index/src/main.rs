//! `clingen-index`: build the CA-ID index from harvested chunks, and check
//! `clingen-allele`'s canonical HGVS against what the Registry returned.
//!
//! Both subcommands read the harvester's chunk files: no header, seven
//! tab-separated columns `CHROM POS REF ALT CAID HGVS_GRCh38 HGVS_GRCh37`.
//! `CAID` is `CA<digits>` when registered, empty when not, or `ERROR:<type>`
//! when the Registry rejected the line.

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, ensure};
use clap::{Args, Parser, Subcommand};
use clingen_allele::{HgvsResult, IndexBuilder, Manifest, canonical_hgvs, parse_caid};
use vareffect::FastaReader;

/// Recorded in the manifest: the exact vareffect release Cargo.toml pins,
/// whose canonical HGVS the index keys are meant to match.
const VAREFFECT_VERSION: &str = "0.6.0";

/// How many malformed rows to show on stderr (all are counted).
const MALFORMED_EXAMPLES: usize = 5;

/// Exit code for a run that could not complete (bad input, I/O, conflicting
/// IDs). Exit 1 is reserved for "a substitution failed validation".
const EXIT_FAILURE: u8 = 2;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the HGVS to CA ID index from harvested chunks.
    ///
    /// Indexes every row whose CAID parses and whose HGVS_GRCh38 is non-empty,
    /// and prints a one-line summary on stderr. Exits 2 if one HGVS was
    /// harvested with two different CA IDs.
    Build(BuildArgs),
    /// Compare canonical HGVS computed from the genome with the Registry's.
    ///
    /// Compares every row that has an HGVS_GRCh38 and no ERROR, registered or
    /// not, and prints per-class match rates on stdout. Exits 1 if any
    /// substitution mismatches, 2 if the run could not complete.
    Validate(ValidateArgs),
}

#[derive(Args)]
struct ChunkArgs {
    /// Directory of `chunk_*.tsv` files (read in filename order); repeatable.
    #[arg(long, required = true, num_args = 1.., value_name = "DIR")]
    chunks: Vec<PathBuf>,
}

#[derive(Args)]
struct BuildArgs {
    #[command(flatten)]
    chunks: ChunkArgs,
    /// Directory to write `caid.fst` and `manifest.json` into. An index
    /// already there is replaced by rename, so running readers keep the old
    /// one and a failed build leaves it intact.
    #[arg(long, value_name = "DIR")]
    out: PathBuf,
    /// Label for the manifest's `sources`; repeatable.
    #[arg(long, value_name = "LABEL")]
    source: Vec<String>,
    /// vareffect genome binary (`genome.bin`) to record in the manifest. It
    /// must exist, and is recorded as an absolute path (directory resolved,
    /// file name kept), so the tools find it whatever directory they run in.
    #[arg(long, value_name = "PATH")]
    genome: Option<PathBuf>,
    /// Date the Registry was queried, recorded in the manifest.
    #[arg(long, value_name = "YYYY-MM-DD", default_value = "unknown")]
    harvest_date: String,
}

#[derive(Args)]
struct ValidateArgs {
    #[command(flatten)]
    chunks: ChunkArgs,
    /// vareffect genome binary (`genome.bin`).
    #[arg(long, value_name = "BIN")]
    genome: PathBuf,
    /// Stop after this many compared rows (the first N in file order).
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..))]
    sample: Option<u64>,
    /// Write each mismatching row here as TSV.
    #[arg(long, value_name = "TSV")]
    mismatches: PathBuf,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Build(args) => build(args),
        Command::Validate(args) => validate(args),
    }
}

// ---------------------------------------------------------------------------
// Reading chunks
// ---------------------------------------------------------------------------

/// The first six columns of a chunk row. `HGVS_GRCh37` is never needed.
struct Row<'a> {
    chrom: &'a str,
    pos: &'a str,
    ref_allele: &'a str,
    alt_allele: &'a str,
    caid: &'a str,
    hgvs38: &'a str,
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
enum Verdict {
    Continue,
    /// Stop reading: the visitor has seen enough rows.
    Stop,
    /// The row is unusable for the visitor; count it and say why.
    Malformed(&'static str),
}

/// Malformed rows seen while reading, with the first few for the report.
#[derive(Default)]
struct Malformed {
    count: u64,
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

    fn report(&self) {
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
fn chunk_files(dirs: &[PathBuf]) -> Result<Vec<PathBuf>> {
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
struct ReadSummary {
    /// Non-blank lines read, malformed ones included.
    rows: u64,
    malformed: Malformed,
}

/// Stream every row of `files` through `visit`, without holding a file in
/// memory. Blank lines are skipped, a trailing `\r` is tolerated, and a line
/// that is not UTF-8 or has fewer than six columns is counted as malformed.
fn read_rows(
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

// ---------------------------------------------------------------------------
// build
// ---------------------------------------------------------------------------

/// `--genome` as the manifest records it: its directory made absolute and
/// canonical, its file name kept as given.
///
/// Consumers resolve a relative manifest path against the index directory,
/// so recording one relative to the working directory would name another
/// file. The file name is not resolved because vareffect finds the genome's
/// index at `<path>.idx`: when `genome.bin` and `genome.bin.idx` are separate
/// symlinks into a content-addressed store, the link's target has no `.idx`
/// next to it.
fn manifest_genome(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .ok_or_else(|| anyhow!("--genome {} is not a file", path.display()))?;
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let recorded = fs::canonicalize(dir)
        .with_context(|| format!("cannot resolve --genome {}", path.display()))?
        .join(name);
    let meta = fs::metadata(&recorded)
        .with_context(|| format!("cannot resolve --genome {}", path.display()))?;
    ensure!(meta.is_file(), "--genome {} is not a file", path.display());
    recorded
        .into_os_string()
        .into_string()
        .map_err(|p| anyhow!("--genome {} is not valid UTF-8", p.display()))
}

fn build(args: BuildArgs) -> Result<ExitCode> {
    // Before any chunk is read, so a mistyped path fails fast.
    let genome = args.genome.as_deref().map(manifest_genome).transpose()?;
    let files = chunk_files(&args.chunks.chunks)?;

    let mut builder = IndexBuilder::new();
    let (mut indexed, mut unregistered, mut errors) = (0u64, 0u64, 0u64);
    let read = read_rows(&files, |row| {
        if row.caid.is_empty() {
            unregistered += 1;
        } else if row.caid.starts_with("ERROR:") {
            errors += 1;
        } else if let Some(caid) = parse_caid(row.caid) {
            if row.hgvs38.is_empty() {
                return Ok(Verdict::Malformed("registered row has no HGVS_GRCh38"));
            }
            builder.add(row.hgvs38, caid);
            indexed += 1;
        } else {
            return Ok(Verdict::Malformed(
                "CAID is not CA<digits>, empty or ERROR:...",
            ));
        }
        Ok(Verdict::Continue)
    })?;
    read.malformed.report();

    let manifest = Manifest {
        // `finish` fills in the format version and the entry count.
        format_version: 0,
        assembly: "GRCh38".to_owned(),
        sources: args.source,
        harvest_date: args.harvest_date,
        entries: 0,
        vareffect_version: VAREFFECT_VERSION.to_owned(),
        genome,
    };
    let entries = builder
        .finish(&args.out, manifest)
        .with_context(|| format!("building index in {}", args.out.display()))?;

    eprintln!(
        "rows={} indexed={indexed} unregistered={unregistered} errors={errors} malformed={} entries={entries}",
        read.rows, read.malformed.count
    );
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

/// Kind of edit named by a Registry HGVS string, for the per-class report.
///
/// Declaration order is the report order; the discriminants index [`Tally`].
#[derive(Clone, Copy)]
enum Class {
    Substitution,
    Del,
    Dup,
    Ins,
    Delins,
    Mito,
    Other,
}

impl Class {
    /// Report order.
    const ALL: [Class; 7] = [
        Class::Substitution,
        Class::Del,
        Class::Dup,
        Class::Ins,
        Class::Delins,
        Class::Mito,
        Class::Other,
    ];

    fn of(registry_hgvs: &str) -> Class {
        if registry_hgvs.contains(":m.") {
            Class::Mito
        } else if registry_hgvs.contains('>') {
            Class::Substitution
        } else if registry_hgvs.contains("delins") {
            Class::Delins
        } else if registry_hgvs.ends_with("dup") {
            Class::Dup
        } else if registry_hgvs.ends_with("del") {
            Class::Del
        } else if registry_hgvs.contains("ins") {
            Class::Ins
        } else {
            Class::Other
        }
    }

    fn name(self) -> &'static str {
        match self {
            Class::Substitution => "substitution",
            Class::Del => "del",
            Class::Dup => "dup",
            Class::Ins => "ins",
            Class::Delins => "delins",
            Class::Mito => "m",
            Class::Other => "other",
        }
    }
}

/// Compared and matching row counts, indexed like [`Class::ALL`].
#[derive(Default)]
struct Tally {
    total: [u64; 7],
    matched: [u64; 7],
}

impl Tally {
    fn record(&mut self, class: Class, matched: bool) {
        self.total[class as usize] += 1;
        self.matched[class as usize] += u64::from(matched);
    }

    fn substitution_mismatches(&self) -> u64 {
        let i = Class::Substitution as usize;
        self.total[i] - self.matched[i]
    }

    fn write_table(&self, out: &mut impl Write) -> io::Result<()> {
        writeln!(out, "class\ttotal\tmatch\tpct")?;
        for class in Class::ALL {
            let (total, matched) = (self.total[class as usize], self.matched[class as usize]);
            let pct = if total == 0 {
                0.0
            } else {
                matched as f64 * 100.0 / total as f64
            };
            writeln!(out, "{}\t{total}\t{matched}\t{pct:.1}", class.name())?;
        }
        Ok(())
    }
}

/// Our canonical HGVS for the row, or a label saying why we have none.
/// The bool is whether it equals the Registry's string.
fn ours(row: &Row, pos1: u64, genome: &FastaReader) -> (String, bool) {
    match canonical_hgvs(
        row.chrom,
        pos1,
        row.ref_allele.as_bytes(),
        row.alt_allele.as_bytes(),
        genome,
    ) {
        Ok(HgvsResult::Hgvs(hgvs)) => {
            let matched = hgvs == row.hgvs38;
            (hgvs, matched)
        }
        Ok(HgvsResult::RefMismatch) => ("RefMismatch".to_owned(), false),
        Ok(HgvsResult::Unsupported) => ("Unsupported".to_owned(), false),
        // Keep the message on one TSV field.
        Err(e) => (format!("Error:{e}").replace(['\t', '\r', '\n'], " "), false),
    }
}

fn validate(args: ValidateArgs) -> Result<ExitCode> {
    let files = chunk_files(&args.chunks.chunks)?;
    let genome = FastaReader::open(&args.genome)
        .with_context(|| format!("opening genome {}", args.genome.display()))?;
    let mismatch_file = File::create(&args.mismatches)
        .with_context(|| format!("creating {}", args.mismatches.display()))?;
    let mut mismatches = BufWriter::new(mismatch_file);
    writeln!(mismatches, "CHROM\tPOS\tREF\tALT\tCLASS\tREGISTRY\tOURS")?;

    let mut tally = Tally::default();
    let (mut compared, mut skipped) = (0u64, 0u64);
    let read = read_rows(&files, |row| {
        // Nothing to compare against on a Registry error or without an HGVS.
        if row.caid.starts_with("ERROR:") || row.hgvs38.is_empty() {
            skipped += 1;
            return Ok(Verdict::Continue);
        }
        let Ok(pos1) = row.pos.parse::<u64>() else {
            return Ok(Verdict::Malformed("POS is not a number"));
        };

        let class = Class::of(row.hgvs38);
        let (ours, matched) = ours(row, pos1, &genome);
        tally.record(class, matched);
        if !matched {
            writeln!(
                mismatches,
                "{}\t{}\t{}\t{}\t{}\t{}\t{ours}",
                row.chrom,
                row.pos,
                row.ref_allele,
                row.alt_allele,
                class.name(),
                row.hgvs38,
            )
            .with_context(|| format!("writing {}", args.mismatches.display()))?;
        }

        compared += 1;
        Ok(if args.sample == Some(compared) {
            Verdict::Stop
        } else {
            Verdict::Continue
        })
    })?;
    mismatches
        .flush()
        .with_context(|| format!("writing {}", args.mismatches.display()))?;
    read.malformed.report();

    eprintln!(
        "rows={} compared={compared} skipped={skipped} malformed={}",
        read.rows, read.malformed.count
    );
    tally
        .write_table(&mut io::stdout().lock())
        .context("writing the class table to stdout")?;

    Ok(if tally.substitution_mismatches() > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The recorded version must be the exact one Cargo.toml pins, since the
    /// index keys only match vareffect's HGVS for that release.
    #[test]
    fn vareffect_version_is_the_cargo_pin() {
        let pin = format!("vareffect = \"={VAREFFECT_VERSION}\"");
        assert!(
            include_str!("../Cargo.toml")
                .lines()
                .any(|l| l.trim() == pin),
            "Cargo.toml must pin `{pin}`"
        );
    }
}
