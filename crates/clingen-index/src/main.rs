//! `clingen-index`: build the CA-ID index from harvested chunks or sorted
//! pairs, check `clingen-allele`'s canonical HGVS against what the Registry
//! returned, and make the GRCh38 genome both of those read.
//!
//! `build --chunks` and `validate` read the harvester's chunk files: no
//! header, seven tab-separated columns
//! `CHROM POS REF ALT CAID HGVS_GRCh38 HGVS_GRCh37`. `CAID` is `CA<digits>`
//! when registered, empty when not, or `ERROR:<type>` when the Registry
//! rejected the line. `build --sorted` reads `HGVS<TAB>CAID` pairs instead,
//! and `genome` a FASTA.
//!
//! This file holds the command line and the dispatch; each subcommand lives in
//! its own module. `chunks` is the reader `build` and `validate` share, and
//! `input` opens the plain-or-gzip inputs of `build --sorted` and `genome`.

mod build;
mod chunks;
mod genome;
mod input;
mod validate;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

/// Exit code for a run that could not complete (bad input, I/O, conflicting
/// IDs). Exit 1 is reserved for "a check failed", such as a substitution
/// failing validation.
pub(crate) const EXIT_FAILURE: u8 = 2;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the HGVS to CA ID index from harvested chunks or sorted pairs.
    ///
    /// With --chunks, indexes every row whose CAID parses and whose
    /// HGVS_GRCh38 is non-empty, sorting them in memory. With --sorted,
    /// streams `HGVS<TAB>CAID` pairs already in byte order into the index in
    /// constant memory. Prints a one-line summary on stderr. Exits 2 if one
    /// HGVS has two different CA IDs, or a --sorted pair is malformed or out
    /// of order.
    Build(build::BuildArgs),
    /// Compare canonical HGVS computed from the genome with the Registry's.
    ///
    /// Compares every row that has an HGVS_GRCh38 and no ERROR, registered or
    /// not, and prints per-class match rates on stdout. Exits 1 if any
    /// substitution mismatches (any class with --strict), 2 if the run could
    /// not complete.
    Validate(ValidateArgs),
    /// Write a GRCh38 FASTA as the vareffect genome the other commands read.
    ///
    /// Keeps the requested primary contigs (all 25 by default), whatever
    /// their FASTA names, under vareffect's names and in FASTA order, and
    /// prints `contigs=<n> bases=<total>` on stderr. Exits 2 if a requested
    /// contig is missing or repeated, or is not the length GRCh38 gives it.
    Genome(genome::GenomeArgs),
}

#[derive(Args)]
pub(crate) struct ChunkArgs {
    /// Directory of `chunk_*.tsv` files (read in filename order); repeatable.
    #[arg(long, required = true, num_args = 1.., value_name = "DIR")]
    pub(crate) chunks: Vec<PathBuf>,
}

#[derive(Args)]
pub(crate) struct ValidateArgs {
    #[command(flatten)]
    pub(crate) chunks: ChunkArgs,
    /// vareffect genome binary (`genome.bin`).
    #[arg(long, value_name = "BIN")]
    pub(crate) genome: PathBuf,
    /// Stop after this many compared rows (the first N in file order).
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) sample: Option<u64>,
    /// Write each mismatching row here as TSV.
    #[arg(long, value_name = "TSV")]
    pub(crate) mismatches: PathBuf,
    /// Exit 1 on a mismatch in any class. Without it only substitution
    /// mismatches fail the run; the others are still tallied and written to
    /// the mismatches TSV.
    #[arg(long)]
    pub(crate) strict: bool,
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
        Command::Build(args) => build::build(args),
        Command::Validate(args) => validate::validate(args),
        Command::Genome(args) => genome::genome(args),
    }
}
