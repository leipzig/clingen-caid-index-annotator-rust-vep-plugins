//! `clingen-index regions`: turn gene annotations, or a BED file, into the
//! windows `registry enumerate` asks the Registry about.
//!
//! The Registry cannot list every allele it holds, but it can list the alleles
//! inside a span of one reference sequence. So the index of alleles beyond
//! gnomAD and ClinVar is gathered span by span, and this module decides which
//! spans: the exons or genes wanted, padded, merged where they sit close
//! together, and cut into windows small enough to be one request each.
//!
//! The intervals come from UCSC tables (`ncbiRefSeq` with its leading `bin`
//! column, `knownGene` without), both read and pooled, or from a BED file.
//! Everything is 0-based half-open, as UCSC and BED both are. Per primary
//! contig the steps are:
//!
//! 1. each interval is padded by `--pad` on both sides and clamped to the
//!    contig, so a variant in an intron next to an exon (a splice site) falls
//!    inside the interval;
//! 2. the intervals are sorted and merged when the next starts no more than
//!    `--merge-gap` bases past the end of the current one, since one request
//!    for a stretch is cheaper than several for its pieces;
//! 3. each merged interval of length `L` is cut into `ceil(L / --max-window)`
//!    windows of near-equal length, its *cores*, so no request grows beyond
//!    what the Registry answers in one page or two;
//! 4. the query span of a window is its core widened by `--flank` on both
//!    sides, clamped to the contig. The Registry returns only alleles that lie
//!    wholly inside the span, so the flank is what keeps an allele that starts
//!    in one core and runs a little past its end from being lost; an allele
//!    longer than the flank that crosses a window edge is still not returned.
//!
//! The Registry takes `begin` and `end` and returns the alleles whose start is
//! greater than `begin` and whose end is at most `end`. With 1-based allele
//! coordinates, that is exactly the 1-based bases `begin + 1 ..= end`, i.e. the
//! 0-based half-open span `[begin, end)`, so the query span is written as it
//! is, with no adjustment.
//!
//! Records on contigs that are not one of the 25 GRCh38 primary sequences
//! (alt, random, unplaced, patch) are skipped and counted. An interval that
//! lies past the end of its primary contig is an error, not a clamp: it means
//! the input is from another assembly, and a window set quietly built from it
//! would enumerate the wrong places.
//!
//! The output directory gets `windows_<chrom>_<NNNN>.tsv` files with no
//! header, at most `--batch` windows each and one chromosome each, so a
//! workflow can make one task of every file, and a `summary.tsv` with a
//! header. An output directory that already holds either is refused: a file
//! left by another run would otherwise be mistaken for one of this run's.

use std::fs;
use std::io::{BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail, ensure};
use clap::{Args, ValueEnum};
use clingen_allele::{Contig, primary_contigs, resolve_contig};

use crate::input::{input_label, is_stdin, open_input};

/// Columns of a UCSC gene table after its optional `bin`: `name chrom strand
/// txStart txEnd cdsStart cdsEnd exonCount exonStarts exonEnds`. Both tables
/// go on with more, which are not read.
const UCSC_COLUMNS: usize = 10;

/// The part of the UCSC annotation to take intervals from.
#[derive(Clone, Copy, ValueEnum)]
enum RegionSet {
    /// The coding sequence: each coding transcript's exons cut to
    /// `[cdsStart, cdsEnd)`.
    #[value(name = "cds")]
    Cds,
    /// The whole exons of the coding transcripts, UTRs included.
    #[value(name = "pc_exons")]
    PcExons,
    /// The exons of every transcript, non-coding ones too.
    #[value(name = "exons")]
    Exons,
    /// `[txStart, txEnd)` of every transcript: introns and all.
    #[value(name = "genes")]
    Genes,
}

/// Arguments of `regions`.
#[derive(Args)]
pub(crate) struct RegionsArgs {
    /// UCSC `ncbiRefSeq` table, with its leading `bin` column; plain or gzip,
    /// or `-` for standard input.
    #[arg(
        long,
        value_name = "PATH|-",
        requires = "gencode",
        required_unless_present = "bed",
        conflicts_with = "bed"
    )]
    refseq: Option<PathBuf>,
    /// UCSC `knownGene` table (GENCODE), without a `bin` column; plain or
    /// gzip, or `-` for standard input.
    #[arg(
        long,
        value_name = "PATH|-",
        requires = "refseq",
        required_unless_present = "bed",
        conflicts_with = "bed"
    )]
    gencode: Option<PathBuf>,
    /// Which part of the two tables to take intervals from.
    #[arg(
        long,
        value_enum,
        required_unless_present = "bed",
        conflicts_with = "bed"
    )]
    set: Option<RegionSet>,
    /// BED file (three or more columns; `track`, `browser` and `#` lines are
    /// ignored) to take the intervals from instead of the tables. Its
    /// chromosomes may be named `21`, `chr21`, `MT`, `NC_000021.9` or the
    /// like.
    #[arg(long, value_name = "BED", conflicts_with_all = ["refseq", "gencode", "set"])]
    bed: Option<PathBuf>,
    /// Bases added to both ends of every interval, so a variant just outside
    /// an exon (a splice site) is inside.
    #[arg(long, value_name = "BP", default_value_t = 20)]
    pad: u64,
    /// Merge intervals no further apart than this many bases.
    #[arg(long, value_name = "BP", default_value_t = 5000)]
    merge_gap: u64,
    /// Longest core of a window; a longer merged interval is cut into equal
    /// parts.
    #[arg(
        long,
        value_name = "BP",
        default_value_t = 50_000,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    max_window: u64,
    /// Bases added to both ends of a window's core to make its query span, so
    /// an allele that runs a little past the core is still inside.
    #[arg(long, value_name = "BP", default_value_t = 200)]
    flank: u64,
    /// One chromosome (`21`, `chrX`, `MT`, ...) to make windows for, or `all`.
    #[arg(long, value_name = "CHROM|all", default_value = "all")]
    chrom: String,
    /// Windows per output file: the unit a workflow runs as one task.
    #[arg(
        long,
        value_name = "N",
        default_value_t = 500,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    batch: u64,
    /// Directory for `windows_<chrom>_<NNNN>.tsv` and `summary.tsv`; created
    /// if absent, and refused if it already holds either.
    #[arg(long, value_name = "DIR")]
    out_dir: PathBuf,
}

/// A half-open interval `[start, end)` on one contig.
type Span = (u64, u64);

/// A query window: the span asked of the Registry and the core it was cut for.
struct Window {
    begin: u64,
    end: u64,
    core_begin: u64,
    core_end: u64,
}

/// What became of one contig's intervals.
struct ContigWindows {
    contig: Contig,
    /// Padded intervals before merging.
    intervals: u64,
    /// Intervals after merging.
    merged: u64,
    windows: Vec<Window>,
    /// Bases in the cores.
    core_bp: u64,
    /// Bases in the query spans, a base counted once per span holding it.
    query_bp: u64,
}

/// The padded intervals read so far, per primary contig.
struct Collector {
    /// Indexed like [`primary_contigs`].
    intervals: Vec<Vec<Span>>,
    pad: u64,
    /// The one contig wanted, or `None` for all.
    only: Option<Contig>,
    /// Records on contigs that are not primary.
    skipped: u64,
}

impl Collector {
    fn new(pad: u64, only: Option<Contig>) -> Self {
        Collector {
            intervals: vec![Vec::new(); primary_contigs().len()],
            pad,
            only,
            skipped: 0,
        }
    }

    /// The slot of the contig named `name`, if its intervals are wanted.
    ///
    /// A name that is not a primary contig counts as a skipped record. One
    /// that is, but is not the `--chrom` asked for, is just not wanted.
    fn slot(&mut self, name: &str) -> Option<usize> {
        let Some(contig) = resolve_contig(name) else {
            self.skipped += 1;
            return None;
        };
        if self.only.is_some_and(|only| only != contig) {
            return None;
        }
        primary_contigs().iter().position(|c| *c == contig)
    }

    /// Add `[start, end)` on the contig at `slot`, padded and clamped.
    fn add(&mut self, slot: usize, start: u64, end: u64) -> Result<()> {
        let contig = primary_contigs()[slot];
        ensure!(
            start < end,
            "interval {start}-{end} on {} is empty or backwards",
            contig.vareffect_name
        );
        ensure!(
            end <= contig.length,
            "interval {start}-{end} runs past the end of {} ({} bases); is the input GRCh38?",
            contig.vareffect_name,
            contig.length
        );
        self.intervals[slot].push((
            start.saturating_sub(self.pad),
            end.saturating_add(self.pad).min(contig.length),
        ));
        Ok(())
    }

    /// Read a UCSC gene table, taking `set` of each transcript.
    ///
    /// `has_bin` is true for `ncbiRefSeq`. The first columns of each row are
    /// checked as they are read: a table given as the wrong one has its
    /// columns shifted by one, which would otherwise show only as every
    /// record being on an unknown contig.
    fn read_ucsc(&mut self, path: &Path, has_bin: bool, set: RegionSet) -> Result<()> {
        let lead = usize::from(has_bin);
        let table = if has_bin { "ncbiRefSeq" } else { "knownGene" };
        for_each_line(path, |line| {
            let cols: Vec<&str> = line.split('\t').collect();
            ensure!(
                cols.len() >= lead + UCSC_COLUMNS,
                "{} tab-separated columns, but a {table} row has at least {} \
                 ({}name chrom strand txStart txEnd cdsStart cdsEnd exonCount exonStarts exonEnds)",
                cols.len(),
                lead + UCSC_COLUMNS,
                if has_bin { "bin " } else { "" }
            );
            if has_bin {
                ensure!(
                    cols[0].parse::<u64>().is_ok(),
                    "the first column is {:?}, not a number; an ncbiRefSeq row starts with \
                     its bin (is this the knownGene table?)",
                    cols[0]
                );
            }
            let row = &cols[lead..];
            ensure!(
                matches!(row[2], "+" | "-"),
                "the strand is {:?}, not + or -; the columns are not those of a {table} table",
                row[2]
            );
            let Some(slot) = self.slot(row[1]) else {
                return Ok(());
            };
            let transcript = Transcript::parse(row)?;
            self.add_transcript(slot, &transcript, set)
        })
    }

    /// Add the intervals of `set` that `transcript` has.
    fn add_transcript(&mut self, slot: usize, t: &Transcript, set: RegionSet) -> Result<()> {
        // UCSC gives a non-coding transcript an empty coding range.
        let coding = t.cds_end > t.cds_start;
        match set {
            RegionSet::Genes => self.add(slot, t.tx_start, t.tx_end)?,
            RegionSet::Exons => {
                for &(start, end) in &t.exons {
                    self.add(slot, start, end)?;
                }
            }
            RegionSet::PcExons if coding => {
                for &(start, end) in &t.exons {
                    self.add(slot, start, end)?;
                }
            }
            RegionSet::Cds if coding => {
                for &(start, end) in &t.exons {
                    // An exon wholly in the UTR has no coding part.
                    let (start, end) = (start.max(t.cds_start), end.min(t.cds_end));
                    if start < end {
                        self.add(slot, start, end)?;
                    }
                }
            }
            RegionSet::PcExons | RegionSet::Cds => {}
        }
        Ok(())
    }

    /// Read a BED file: the first three columns of each line.
    fn read_bed(&mut self, path: &Path) -> Result<()> {
        for_each_line(path, |line| {
            let mut cols = line.split_ascii_whitespace();
            let first = cols.next().unwrap_or_default();
            if first == "track" || first == "browser" {
                return Ok(());
            }
            let (Some(start), Some(end)) = (cols.next(), cols.next()) else {
                bail!("expected at least 3 columns (chrom start end)");
            };
            let Some(slot) = self.slot(first) else {
                return Ok(());
            };
            self.add(slot, number(start, "start")?, number(end, "end")?)
        })
    }
}

/// The columns of a UCSC row that `regions` uses.
struct Transcript {
    tx_start: u64,
    tx_end: u64,
    cds_start: u64,
    cds_end: u64,
    exons: Vec<Span>,
}

impl Transcript {
    /// Parse a row from `name` on (the `bin` already dropped).
    fn parse(row: &[&str]) -> Result<Self> {
        let count = number(row[7], "exonCount")?;
        let starts = coordinate_list(row[8], "exonStarts")?;
        let ends = coordinate_list(row[9], "exonEnds")?;
        for (what, list) in [("exonStarts", &starts), ("exonEnds", &ends)] {
            ensure!(
                list.len() as u64 == count,
                "exonCount is {count} but {what} lists {}",
                list.len()
            );
        }
        let exons: Vec<Span> = starts.into_iter().zip(ends).collect();
        for &(start, end) in &exons {
            ensure!(start < end, "exon {start}-{end} is empty or backwards");
        }
        Ok(Transcript {
            tx_start: number(row[3], "txStart")?,
            tx_end: number(row[4], "txEnd")?,
            cds_start: number(row[5], "cdsStart")?,
            cds_end: number(row[6], "cdsEnd")?,
            exons,
        })
    }
}

/// `field` as a non-negative integer, naming the column `what` on failure.
fn number(field: &str, what: &str) -> Result<u64> {
    field
        .parse()
        .map_err(|_| anyhow!("{what} {field:?} is not a non-negative integer"))
}

/// A UCSC comma list (`1000,2000,`, with its trailing comma, or without).
fn coordinate_list(field: &str, what: &str) -> Result<Vec<u64>> {
    let body = field.strip_suffix(',').unwrap_or(field);
    if body.is_empty() {
        return Ok(Vec::new());
    }
    body.split(',').map(|item| number(item, what)).collect()
}

/// Call `each` on every line of `path` that is not blank or a `#` comment.
///
/// Both tables and BED files are read through this, so a bad line is always
/// reported as `<file>:<line>: <what is wrong>`.
fn for_each_line(path: &Path, mut each: impl FnMut(&str) -> Result<()>) -> Result<()> {
    let label = input_label(path);
    let mut input = open_input(path)?;
    let mut buf = String::new();
    let mut line_no = 0u64;
    loop {
        buf.clear();
        line_no += 1;
        let n = input
            .read_line(&mut buf)
            .with_context(|| format!("{label}:{line_no}"))?;
        if n == 0 {
            return Ok(());
        }
        let line = buf.trim_end_matches(['\n', '\r']);
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        each(line).with_context(|| format!("{label}:{line_no}"))?;
    }
}

/// `intervals` merged where the next starts no more than `gap` past the end of
/// the current one (an overlap always merges), in order.
fn merge(mut intervals: Vec<Span>, gap: u64) -> Vec<Span> {
    intervals.sort_unstable();
    let mut merged: Vec<Span> = Vec::new();
    for (start, end) in intervals {
        match merged.last_mut() {
            Some(last) if start <= last.1.saturating_add(gap) => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// `[start, end)` cut into `ceil(len / max_window)` pieces whose lengths differ
/// by at most one base: 120,000 bases at 50,000 are three of 40,000, not two of
/// 50,000 and one of 20,000.
fn split(start: u64, end: u64, max_window: u64) -> impl Iterator<Item = Span> {
    let len = end - start;
    let pieces = len.div_ceil(max_window);
    // `i * len` stays far below u64::MAX: a contig is under 2^28 bases, so
    // `pieces <= len` makes the product under 2^56.
    (0..pieces).map(move |i| (start + i * len / pieces, start + (i + 1) * len / pieces))
}

/// The windows of one contig's padded intervals.
fn plan_contig(contig: Contig, intervals: Vec<Span>, args: &RegionsArgs) -> ContigWindows {
    let padded = intervals.len() as u64;
    let merged = merge(intervals, args.merge_gap);
    let mut windows = Vec::new();
    for &(start, end) in &merged {
        for (core_begin, core_end) in split(start, end, args.max_window) {
            windows.push(Window {
                begin: core_begin.saturating_sub(args.flank),
                end: core_end.saturating_add(args.flank).min(contig.length),
                core_begin,
                core_end,
            });
        }
    }
    ContigWindows {
        contig,
        intervals: padded,
        merged: merged.len() as u64,
        core_bp: windows.iter().map(|w| w.core_end - w.core_begin).sum(),
        query_bp: windows.iter().map(|w| w.end - w.begin).sum(),
        windows,
    }
}

/// The name windows files and the `chrom` column use: `21`, `X`, `MT`. It is
/// also the only form `registry lookup` takes, as the harvester did.
pub(crate) fn bare_name(contig: &Contig) -> &'static str {
    if contig.mito {
        "MT"
    } else {
        // Every `vareffect_name` starts with "chr".
        &contig.vareffect_name[3..]
    }
}

/// `--chrom` as the one contig it names, or `None` for `all`.
fn chrom_filter(text: &str) -> Result<Option<Contig>> {
    if text.eq_ignore_ascii_case("all") {
        return Ok(None);
    }
    resolve_contig(text).map(Some).ok_or_else(|| {
        anyhow!(
            "--chrom: {text:?} is not a GRCh38 primary contig \
             (1-22, X, Y, MT, their chr forms, an NC_ accession, or all)"
        )
    })
}

/// Fail if `dir` holds a windows file or a summary: they would be taken for
/// this run's.
fn refuse_leftovers(dir: &Path) -> Result<()> {
    let Ok(entries) = fs::read_dir(dir) else {
        // Absent (made later) or unreadable (the write will say so).
        return Ok(());
    };
    for entry in entries {
        let name = entry
            .with_context(|| format!("reading {}", dir.display()))?
            .file_name();
        let name = name.to_string_lossy();
        ensure!(
            name != "summary.tsv" && !(name.starts_with("windows_") && name.ends_with(".tsv")),
            "{} already holds {name}; use an empty directory, so files left by another \
             run are not taken for this run's windows",
            dir.display()
        );
    }
    Ok(())
}

/// Create `path` and let `write` fill it through a buffer, so that a failure
/// to create, write or flush names the file.
fn write_file(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<fs::File>) -> std::io::Result<()>,
) -> Result<()> {
    let file =
        fs::File::create(path).with_context(|| format!("cannot create {}", path.display()))?;
    let mut out = BufWriter::new(file);
    write(&mut out)
        .and_then(|()| out.flush())
        .with_context(|| format!("writing {}", path.display()))
}

/// Write the windows files and the summary into `dir`.
fn write_output(dir: &Path, plans: &[ContigWindows], batch: usize) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    for plan in plans {
        let chrom = bare_name(&plan.contig);
        for (n, batch_windows) in plan.windows.chunks(batch).enumerate() {
            let path = dir.join(format!("windows_{chrom}_{n:04}.tsv"));
            write_file(&path, |out| {
                for w in batch_windows {
                    writeln!(
                        out,
                        "{chrom}\t{}\t{}\t{}\t{}\t{}",
                        plan.contig.accession, w.begin, w.end, w.core_begin, w.core_end
                    )?;
                }
                Ok(())
            })?;
        }
    }
    write_file(&dir.join("summary.tsv"), |out| {
        writeln!(out, "chrom\tintervals\tmerged\twindows\tcore_bp\tquery_bp")?;
        for plan in plans {
            writeln!(
                out,
                "{}\t{}\t{}\t{}\t{}\t{}",
                bare_name(&plan.contig),
                plan.intervals,
                plan.merged,
                plan.windows.len(),
                plan.core_bp,
                plan.query_bp
            )?;
        }
        Ok(())
    })
}

/// Run `regions`: read the intervals, plan the windows of every contig that has
/// any, write them, and print
/// `contigs=<n> windows=<n> core_bp=<n> query_bp=<n> skipped_records=<n>` on
/// stderr.
pub(crate) fn regions(args: RegionsArgs) -> Result<ExitCode> {
    let only = chrom_filter(&args.chrom)?;
    refuse_leftovers(&args.out_dir)?;

    let mut collector = Collector::new(args.pad, only);
    match (&args.bed, &args.refseq, &args.gencode, args.set) {
        (Some(bed), ..) => collector.read_bed(bed)?,
        (None, Some(refseq), Some(gencode), Some(set)) => {
            // Standard input can be read once; the second table would be empty.
            ensure!(
                !(is_stdin(refseq) && is_stdin(gencode)),
                "--refseq and --gencode cannot both be standard input (-)"
            );
            collector.read_ucsc(refseq, true, set)?;
            collector.read_ucsc(gencode, false, set)?;
        }
        // clap refuses every other combination before this runs.
        _ => bail!("give --bed, or all of --refseq, --gencode and --set"),
    }

    let skipped = collector.skipped;
    let plans: Vec<ContigWindows> = primary_contigs()
        .iter()
        .zip(std::mem::take(&mut collector.intervals))
        .filter(|(_, intervals)| !intervals.is_empty())
        .map(|(contig, intervals)| plan_contig(*contig, intervals, &args))
        .collect();
    ensure!(
        !plans.is_empty(),
        "no intervals on {} ({skipped} records on other contigs were skipped); \
         check --set, --chrom and that the input is GRCh38",
        if only.is_some() {
            args.chrom.as_str()
        } else {
            "any primary contig"
        }
    );

    let batch = usize::try_from(args.batch).unwrap_or(usize::MAX);
    write_output(&args.out_dir, &plans, batch)?;

    eprintln!(
        "contigs={} windows={} core_bp={} query_bp={} skipped_records={skipped}",
        plans.len(),
        plans.iter().map(|p| p.windows.len()).sum::<usize>(),
        plans.iter().map(|p| p.core_bp).sum::<u64>(),
        plans.iter().map(|p| p.query_bp).sum::<u64>(),
    );
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_closes_gaps_up_to_and_including_the_limit() {
        // Touching intervals merge even at gap 0; one base apart do not.
        assert_eq!(merge(vec![(10, 20), (20, 30)], 0), [(10, 30)]);
        assert_eq!(merge(vec![(10, 20), (21, 30)], 0), [(10, 20), (21, 30)]);
        assert_eq!(merge(vec![(10, 20), (21, 30)], 1), [(10, 30)]);
        // Unsorted input, overlaps, and an interval inside another.
        assert_eq!(
            merge(vec![(50, 60), (0, 100), (10, 20), (200, 210)], 0),
            [(0, 100), (200, 210)]
        );
        // A huge gap cannot overflow.
        assert_eq!(merge(vec![(0, 1), (5, 6)], u64::MAX), [(0, 6)]);
    }

    #[test]
    fn split_makes_near_equal_pieces_that_tile_the_interval() {
        assert_eq!(split(0, 100, 100).collect::<Vec<_>>(), [(0, 100)]);
        assert_eq!(
            split(10, 120_010, 50_000).collect::<Vec<_>>(),
            [(10, 40_010), (40_010, 80_010), (80_010, 120_010)]
        );
        // 100,001 bases in three pieces: lengths 33,333 / 33,334 / 33,334.
        let pieces: Vec<Span> = split(0, 100_001, 50_000).collect();
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0].0, 0);
        assert_eq!(pieces[2].1, 100_001);
        assert!(pieces.windows(2).all(|p| p[0].1 == p[1].0));
        let lengths: Vec<u64> = pieces.iter().map(|p| p.1 - p.0).collect();
        assert!(lengths.iter().all(|&l| (33_333..=33_334).contains(&l)));
        // A window limit above the length is one piece.
        assert_eq!(split(5, 10, u64::MAX).count(), 1);
    }

    #[test]
    fn coordinate_lists_take_the_ucsc_trailing_comma_or_not() {
        assert_eq!(coordinate_list("1,22,333,", "x").unwrap(), [1, 22, 333]);
        assert_eq!(coordinate_list("1,22,333", "x").unwrap(), [1, 22, 333]);
        assert!(coordinate_list("", "x").unwrap().is_empty());
        let err = coordinate_list("1,two,", "exonStarts").unwrap_err();
        assert!(err.to_string().contains("exonStarts"), "{err}");
        assert!(coordinate_list("1,,2", "x").is_err());
    }
}
