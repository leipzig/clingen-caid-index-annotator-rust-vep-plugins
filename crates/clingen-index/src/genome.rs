//! `clingen-index genome`: cut a GRCh38 FASTA down to its primary contigs and
//! write them as a vareffect genome binary, the file `build --genome`,
//! `validate --genome` and the annotator read.
//!
//! The FASTA may be plain or gzip/BGZF and may name its contigs the UCSC way
//! (`chr21`), the Ensembl way (`21`, `MT`) or by accession (`NC_000021.9`).
//! Whatever it calls them, the output holds them under vareffect's names
//! (`chr21`, `chrM`) in the order the FASTA has them, and only after each one
//! is the length GRCh38 gives it: a genome of another build, or a truncated
//! download, must stop here rather than yield HGVS that is quietly wrong.
//!
//! vareffect's writer takes every contig's sequence at once, so the run holds
//! the kept contigs in memory: about 3.1 GB for all 25.

use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, ErrorKind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail, ensure};
use clap::Args;
use clingen_allele::{Contig, primary_contigs, resolve_contig};
use vareffect::fasta::{is_iupac_nucleotide, write_genome_binary};

use crate::input::{input_label, open_input};

/// The build recorded in the genome's index. The contig lengths checked below
/// are GRCh38's, so no other value would be true.
const BUILD: &str = "GRCh38";

#[derive(Args)]
pub(crate) struct GenomeArgs {
    /// FASTA to read, plain or gzip/BGZF (told by its first bytes, not its
    /// name); `-` reads standard input.
    #[arg(long, value_name = "PATH|-")]
    fasta: PathBuf,
    /// Comma-separated contigs to keep, each as `21`, `chr21`, `MT`, `chrM`,
    /// `NC_000021.9` or the like. Default: all 25 primary contigs. Each one
    /// must be in the FASTA exactly once.
    #[arg(long, value_name = "LIST", value_delimiter = ',')]
    contigs: Vec<String>,
    /// Genome binary to write; its index goes to `<BIN>.idx`. Both are
    /// written beside it under temporary names and renamed into place only
    /// once complete, so a run that fails before then leaves any genome
    /// already there intact. Replacing one removes its old index first.
    #[arg(long, value_name = "BIN")]
    out: PathBuf,
}

/// A requested contig and the sequence the FASTA gave it.
struct Found {
    contig: Contig,
    seq: Vec<u8>,
}

/// `--contigs` as contigs, or all 25 when it was not given.
///
/// Checked before the FASTA is opened, so a mistyped list fails at once rather
/// than after reading a genome.
fn requested_contigs(names: &[String]) -> Result<Vec<Contig>> {
    if names.is_empty() {
        return Ok(primary_contigs().to_vec());
    }
    let mut wanted: Vec<Contig> = Vec::with_capacity(names.len());
    for name in names {
        let contig = resolve_contig(name).ok_or_else(|| {
            anyhow!(
                "--contigs: {name:?} is not a GRCh38 primary contig \
                 (1-22, X, Y, MT, their chr forms, or an NC_ accession)"
            )
        })?;
        ensure!(
            !wanted.contains(&contig),
            "--contigs: {} is listed twice (again as {name:?})",
            contig.vareffect_name
        );
        wanted.push(contig);
    }
    Ok(wanted)
}

/// The contig a header line names, if it is one of `wanted`.
///
/// Only the first word counts (`>21 dna:chromosome ...` is `21`), and it must
/// resolve exactly: `chr21_KI270872v1_alt` and `chrUn_...` are not primary
/// contigs, and a primary contig nobody asked for is skipped like them.
fn wanted_by_header(header: &[u8], wanted: &[Contig]) -> Option<Contig> {
    let word = header.split(u8::is_ascii_whitespace).next()?;
    let contig = resolve_contig(std::str::from_utf8(word).ok()?)?;
    wanted.contains(&contig).then_some(contig)
}

/// Fail unless a kept record has the length GRCh38 gives its contig.
fn check_length(record: &Found) -> Result<()> {
    let got = record.seq.len() as u64;
    ensure!(
        got == record.contig.length,
        "contig {} is {got} bases long; {BUILD} {} is {}",
        record.contig.vareffect_name,
        record.contig.vareffect_name,
        record.contig.length
    );
    Ok(())
}

/// Read the records of `wanted` out of the FASTA, in the order it has them.
///
/// Every other record is skipped unread (not even its bases are checked), so
/// the cost of a whole-genome FASTA is a line scan. A kept record is checked
/// as soon as it ends, so a wrong-build FASTA fails after its first contig
/// rather than after the last.
fn read_contigs(mut input: impl BufRead, wanted: &[Contig]) -> Result<Vec<Found>> {
    let mut found: Vec<Found> = Vec::with_capacity(wanted.len());
    // Whether the record being read is one we keep, i.e. the last of `found`.
    let mut keeping = false;
    let mut seen_header = false;
    let mut line = Vec::new();
    let mut line_no = 0u64;

    loop {
        line.clear();
        let n = input
            .read_until(b'\n', &mut line)
            .with_context(|| format!("line {}", line_no + 1))?;
        if n == 0 {
            break;
        }
        line_no += 1;
        let text = line.trim_ascii();
        if text.is_empty() {
            continue;
        }

        if let Some(header) = text.strip_prefix(b">") {
            if keeping && let Some(done) = found.last() {
                check_length(done)?;
            }
            seen_header = true;
            keeping = false;
            if let Some(contig) = wanted_by_header(header, wanted) {
                ensure!(
                    !found.iter().any(|f| f.contig == contig),
                    "line {line_no}: contig {} appears twice (again as {:?})",
                    contig.vareffect_name,
                    String::from_utf8_lossy(header.trim_ascii())
                );
                // A hint that spares a chromosome-sized Vec its doublings.
                let capacity = usize::try_from(contig.length).unwrap_or(0);
                found.push(Found {
                    contig,
                    seq: Vec::with_capacity(capacity),
                });
                keeping = true;
            }
        } else {
            ensure!(
                seen_header,
                "line {line_no}: sequence before any '>' header, so this is not FASTA"
            );
            if keeping && let Some(record) = found.last_mut() {
                // Checked here rather than left to the writer, whose error
                // names an offset in the output and no line.
                if let Some(&bad) = text
                    .iter()
                    .find(|b| !is_iupac_nucleotide(b.to_ascii_uppercase()))
                {
                    bail!(
                        "line {line_no}: {:?} (0x{bad:02X}) is not a nucleotide code, in contig {}",
                        char::from(bad),
                        record.contig.vareffect_name
                    );
                }
                record.seq.extend_from_slice(text);
            }
        }
    }
    if keeping && let Some(done) = found.last() {
        check_length(done)?;
    }

    let missing: Vec<&str> = wanted
        .iter()
        .filter(|c| !found.iter().any(|f| f.contig == **c))
        .map(|c| c.vareffect_name)
        .collect();
    ensure!(
        missing.is_empty(),
        "requested contigs not found: {}",
        missing.join(", ")
    );
    Ok(found)
}

/// `path` with `suffix` appended to its file name (`genome.bin` to
/// `genome.bin.idx`), as vareffect finds a genome's index.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Write `found` as the genome binary `out` and its index `<out>.idx`.
///
/// Both go to `.tmp` names first. Then any old `<out>.idx` is removed, the
/// `.bin` is renamed into place, and the `.idx` last, because the index is
/// what vareffect takes as proof a `.bin` is complete: a crash anywhere among
/// these steps leaves a genome with no index, which is refused, never an
/// index beside a genome it does not describe (the old one beside the new).
fn write_output(found: &[Found], out: &Path) -> Result<()> {
    let idx = with_suffix(out, ".idx");
    let tmp_bin = with_suffix(out, ".tmp");
    // vareffect writes its index to `<path>.tmp` and renames that onto the
    // path it was given. A path already ending in `.tmp` makes that a rename
    // onto itself, so the index stays at `tmp_idx`, for the rename below.
    let tmp_idx = with_suffix(out, ".idx.tmp");

    let contigs: Vec<(&str, &[u8])> = found
        .iter()
        .map(|f| (f.contig.vareffect_name, f.seq.as_slice()))
        .collect();
    let result = write_genome_binary(&contigs, BUILD, &tmp_bin, &tmp_idx)
        .with_context(|| format!("writing {}", out.display()))
        .and_then(|()| match fs::remove_file(&idx) {
            Err(e) if e.kind() != ErrorKind::NotFound => {
                Err(e).with_context(|| format!("removing the old {}", idx.display()))
            }
            _ => Ok(()),
        })
        .and_then(|()| {
            fs::rename(&tmp_bin, out)
                .with_context(|| format!("renaming {} to {}", tmp_bin.display(), out.display()))
        })
        .and_then(|()| {
            fs::rename(&tmp_idx, &idx)
                .with_context(|| format!("renaming {} to {}", tmp_idx.display(), idx.display()))
        });
    if result.is_err() {
        // Whichever of the two is still there; one may never have been made.
        let _ = fs::remove_file(&tmp_bin);
        let _ = fs::remove_file(&tmp_idx);
    }
    result
}

/// Run `genome`: read the requested contigs from the FASTA, check them, write
/// the genome and print `contigs=<n> bases=<total>` on stderr.
pub(crate) fn genome(args: GenomeArgs) -> Result<ExitCode> {
    let wanted = requested_contigs(&args.contigs)?;
    let input = open_input(&args.fasta)?;
    let found = read_contigs(input, &wanted)
        .with_context(|| format!("reading {}", input_label(&args.fasta)))?;

    write_output(&found, &args.out)?;

    let bases: u64 = found.iter().map(|f| f.seq.len() as u64).sum();
    eprintln!("contigs={} bases={bases}", found.len());
    Ok(ExitCode::SUCCESS)
}
