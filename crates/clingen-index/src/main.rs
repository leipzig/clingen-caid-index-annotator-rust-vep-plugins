//! `clingen-index`: build the CA-ID index from harvested chunks, and check
//! `clingen-allele`'s canonical HGVS against what the Registry returned.
//!
//! Both subcommands read the harvester's chunk files: no header, seven
//! tab-separated columns `CHROM POS REF ALT CAID HGVS_GRCh38 HGVS_GRCh37`.
//! `CAID` is `CA<digits>` when registered, empty when not, or `ERROR:<type>`
//! when the Registry rejected the line.
//!
//! This file holds the command line and the dispatch; each subcommand lives in
//! its own module, and `chunks` is the reader they share.

mod build;
mod chunks;
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
    /// substitution mismatches (any class with --strict), 2 if the run could
    /// not complete.
    Validate(ValidateArgs),
}

#[derive(Args)]
pub(crate) struct ChunkArgs {
    /// Directory of `chunk_*.tsv` files (read in filename order); repeatable.
    #[arg(long, required = true, num_args = 1.., value_name = "DIR")]
    pub(crate) chunks: Vec<PathBuf>,
}

#[derive(Args)]
pub(crate) struct BuildArgs {
    #[command(flatten)]
    pub(crate) chunks: ChunkArgs,
    /// Directory to write `caid.fst` and `manifest.json` into. An index
    /// already there is replaced by rename, so running readers keep the old
    /// one and a failed build leaves it intact.
    #[arg(long, value_name = "DIR")]
    pub(crate) out: PathBuf,
    /// Label for the manifest's `sources`; repeatable.
    #[arg(long, value_name = "LABEL")]
    pub(crate) source: Vec<String>,
    /// vareffect genome binary (`genome.bin`) to record in the manifest. It
    /// must exist, and is recorded as an absolute path (directory resolved,
    /// file name kept), so the tools find it whatever directory they run in.
    #[arg(long, value_name = "PATH")]
    pub(crate) genome: Option<PathBuf>,
    /// Date the Registry was queried, recorded in the manifest.
    #[arg(long, value_name = "YYYY-MM-DD", default_value = "unknown")]
    pub(crate) harvest_date: String,
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
    }
}
