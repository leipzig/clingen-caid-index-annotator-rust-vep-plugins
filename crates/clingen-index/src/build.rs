//! `clingen-index build`: turn harvested chunks into the HGVS to CA ID index.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, ensure};
use clingen_allele::{IndexBuilder, Manifest, parse_caid};

use crate::BuildArgs;
use crate::chunks::{Verdict, chunk_files, read_rows};

/// Recorded in the manifest: the exact vareffect release Cargo.toml pins,
/// whose canonical HGVS the index keys are meant to match.
const VAREFFECT_VERSION: &str = "0.6.0";

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

/// Run `build`: index every registered row of the chunks into `args.out` and
/// print the one-line summary on stderr.
pub(crate) fn build(args: BuildArgs) -> Result<ExitCode> {
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
