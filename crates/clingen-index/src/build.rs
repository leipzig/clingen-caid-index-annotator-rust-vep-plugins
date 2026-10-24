//! `clingen-index build`: turn harvested chunks, or pairs already in byte
//! order, into the HGVS to CA ID index.
//!
//! `--chunks` gathers every registered row in memory and sorts it, which
//! suits one chromosome's harvest. `--sorted` streams `HGVS<TAB>CAID` pairs
//! that arrive in byte order (`LC_ALL=C sort`) straight into the FST, in
//! memory that does not grow with their number, and checks that order
//! itself, so pairs sorted in another locale stop the build rather than
//! yield a bad index.

use std::fs;
use std::io::{BufRead, ErrorKind};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail, ensure};
use clap::{ArgGroup, Args};
use clingen_allele::{Error, IndexBuilder, Manifest, SortedIndexWriter, parse_caid};

use crate::chunks::{Verdict, chunk_files, read_rows};
use crate::input::{input_label, is_stdin, open_input};

/// Recorded in the manifest: the exact vareffect release Cargo.toml pins,
/// whose canonical HGVS the index keys are meant to match.
const VAREFFECT_VERSION: &str = "0.6.0";

/// `build`'s command line. Exactly one of `--chunks` and `--sorted` is given.
#[derive(Args)]
#[command(group(ArgGroup::new("input").required(true).args(["chunks", "sorted"])))]
pub(crate) struct BuildArgs {
    /// Directory of `chunk_*.tsv` files (read in filename order); repeatable.
    /// Their registered rows are gathered and sorted in memory.
    #[arg(long, num_args = 1.., value_name = "DIR")]
    chunks: Vec<PathBuf>,
    /// File of `HGVS<TAB>CAID` lines, `CAID` as `CA<digits>`, plain or gzip
    /// (told by its first bytes); `-` reads standard input; repeatable. Keys
    /// must be in byte order (`LC_ALL=C sort`) across the files in the order
    /// given, and are streamed into the index in constant memory; exact
    /// repeats are skipped.
    #[arg(long, num_args = 1.., value_name = "FILE")]
    sorted: Vec<PathBuf>,
    /// Directory to write `caid.fst` and `manifest.json` into. An index
    /// already there is replaced by rename, so running readers keep the old
    /// one and a failed build leaves it intact.
    #[arg(long, value_name = "DIR")]
    out: PathBuf,
    /// Label for the manifest's `sources`; repeatable.
    #[arg(long, value_name = "LABEL")]
    source: Vec<String>,
    /// vareffect genome binary (`genome.bin`) to record in the manifest. It
    /// must exist, and is recorded with its directory resolved and its file
    /// name kept, as an absolute path (or one relative to `--out`, with
    /// `--genome-relative`), so the tools find it whatever directory they run
    /// in.
    #[arg(long, value_name = "PATH")]
    genome: Option<PathBuf>,
    /// Record `--genome` relative to `--out` instead (both made canonical
    /// first; e.g. `../genome/GRCh38.bin`), so the index and genome
    /// directories can be moved together. Readers resolve it against the
    /// index directory.
    #[arg(long, requires = "genome")]
    genome_relative: bool,
    /// Date the Registry was queried, recorded in the manifest.
    #[arg(long, value_name = "YYYY-MM-DD", default_value = "unknown")]
    harvest_date: String,
}

/// `--genome` as the manifest records it: its directory made canonical, its
/// file name kept as given, and the whole made relative to `relative_to`
/// (`--out`, also made canonical) when that is given, absolute otherwise.
///
/// Consumers resolve a relative manifest path against the index directory,
/// so recording one relative to the working directory would name another
/// file. The file name is not resolved because vareffect finds the genome's
/// index at `<path>.idx`: when `genome.bin` and `genome.bin.idx` are separate
/// symlinks into a content-addressed store, the link's target has no `.idx`
/// next to it.
fn manifest_genome(path: &Path, relative_to: Option<&Path>) -> Result<String> {
    let name = path
        .file_name()
        .ok_or_else(|| anyhow!("--genome {} is not a file", path.display()))?;
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let canonical = fs::canonicalize(dir)
        .with_context(|| format!("cannot resolve --genome {}", path.display()))?
        .join(name);
    let meta = fs::metadata(&canonical)
        .with_context(|| format!("cannot resolve --genome {}", path.display()))?;
    ensure!(meta.is_file(), "--genome {} is not a file", path.display());
    let recorded = match relative_to {
        Some(out) => relative_path(&canonical, &canonical_out(out)?)?,
        None => canonical,
    };
    recorded
        .into_os_string()
        .into_string()
        .map_err(|p| anyhow!("--genome {} is not valid UTF-8", p.display()))
}

/// `out` made absolute and canonical, though the build has usually not
/// created it yet.
///
/// `fs::canonicalize` needs a path that exists, so the deepest ancestor that
/// does is resolved and the missing components appended. None of those
/// exists, so none is a symlink to resolve, and a `..` among them cancels the
/// component before it, as it does when the directory is created.
fn canonical_out(out: &Path) -> Result<PathBuf> {
    let cannot = || format!("cannot resolve --out {}", out.display());
    let absolute = std::path::absolute(out).with_context(cannot)?;
    let components: Vec<Component> = absolute.components().collect();
    for existing in (1..=components.len()).rev() {
        let ancestor: PathBuf = components[..existing].iter().collect();
        match fs::canonicalize(&ancestor) {
            Ok(mut resolved) => {
                for component in &components[existing..] {
                    match component {
                        Component::ParentDir => {
                            resolved.pop();
                        }
                        Component::CurDir => {}
                        other => resolved.push(other),
                    }
                }
                return Ok(resolved);
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(cannot),
        }
    }
    bail!("{}: none of its ancestors exists", cannot())
}

/// `target` relative to the directory `base`, both absolute and canonical: a
/// `..` for each component of `base` below their deepest common ancestor,
/// then the rest of `target`.
fn relative_path(target: &Path, base: &Path) -> Result<PathBuf> {
    let target: Vec<Component> = target.components().collect();
    let base: Vec<Component> = base.components().collect();
    let common = target.iter().zip(&base).take_while(|(t, b)| t == b).count();
    // Only Windows paths on two drives share nothing, not even a root.
    ensure!(
        common > 0,
        "--genome-relative: {} and {} have no common ancestor",
        target.iter().collect::<PathBuf>().display(),
        base.iter().collect::<PathBuf>().display()
    );
    let mut relative: PathBuf = base[common..]
        .iter()
        .map(|_| Component::ParentDir)
        .collect();
    relative.extend(&target[common..]);
    Ok(relative)
}

/// Run `build`: index `--chunks` or `--sorted` into `args.out` and print the
/// one-line summary on stderr.
pub(crate) fn build(args: BuildArgs) -> Result<ExitCode> {
    // Before any input is read, so a mistyped path fails fast.
    let genome = args
        .genome
        .as_deref()
        .map(|g| manifest_genome(g, args.genome_relative.then_some(args.out.as_path())))
        .transpose()?;
    let manifest = Manifest {
        // Writing the index fills in the format version and the entry count.
        format_version: 0,
        assembly: "GRCh38".to_owned(),
        sources: args.source,
        harvest_date: args.harvest_date,
        entries: 0,
        vareffect_version: VAREFFECT_VERSION.to_owned(),
        genome,
    };
    // clap lets exactly one of the two through.
    if args.sorted.is_empty() {
        build_from_chunks(&args.chunks, &args.out, manifest)?;
    } else {
        build_from_sorted(&args.sorted, &args.out, manifest)?;
    }
    Ok(ExitCode::SUCCESS)
}

/// `build --chunks`: gather every registered row of the chunks in memory,
/// write the index to `out` and print
/// `rows= indexed= unregistered= errors= malformed= entries=` on stderr.
fn build_from_chunks(dirs: &[PathBuf], out: &Path, manifest: Manifest) -> Result<()> {
    let files = chunk_files(dirs)?;

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

    let entries = builder
        .finish(out, manifest)
        .with_context(|| format!("building index in {}", out.display()))?;

    eprintln!(
        "rows={} indexed={indexed} unregistered={unregistered} errors={errors} malformed={} entries={entries}",
        read.rows, read.malformed.count
    );
    Ok(())
}

/// `build --sorted`: stream the pairs of `files`, in the order given, into
/// the index in `out` and print `pairs= repeats= entries=` on stderr.
///
/// One line is held at a time. Unlike `--chunks`, which counts a bad row and
/// goes on, any line that is not a pair stops the build: these files are
/// made by a pipeline, so a bad line means a broken step, not a quirk of the
/// Registry. Any error leaves an index already in `out` as it was.
fn build_from_sorted(files: &[PathBuf], out: &Path, manifest: Manifest) -> Result<()> {
    // Before the index directory is made, so a mistyped path fails fast
    // rather than after the files before it have streamed. Existence only:
    // a named pipe, as `<(sort ...)` gives, is not a regular file.
    for path in files.iter().filter(|p| !is_stdin(p)) {
        let meta = fs::metadata(path)
            .with_context(|| format!("cannot read --sorted {}", path.display()))?;
        ensure!(!meta.is_dir(), "--sorted {} is a directory", path.display());
    }

    let mut writer = SortedIndexWriter::create(out)
        .with_context(|| format!("building index in {}", out.display()))?;
    let (mut pairs, mut repeats) = (0u64, 0u64);
    let mut line = Vec::new();
    for path in files {
        let label = input_label(path);
        let mut input = open_input(path)?;
        let mut line_no = 0u64;
        loop {
            line.clear();
            let n = input
                .read_until(b'\n', &mut line)
                .with_context(|| format!("reading {label}"))?;
            if n == 0 {
                break;
            }
            line_no += 1;
            let text = line.strip_suffix(b"\n").unwrap_or(&line);
            if text.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let (hgvs, caid) = parse_pair(text)
                .map_err(|why| anyhow!("malformed pair at {label}:{line_no}: {why}"))?;
            pairs += 1;
            match writer.push(hgvs, caid) {
                Ok(true) => {}
                Ok(false) => repeats += 1,
                Err(Error::Unsorted { previous, key }) => {
                    bail!("keys out of order at {label}:{line_no}: {key} after {previous}")
                }
                Err(e) => return Err(e).with_context(|| format!("pair at {label}:{line_no}")),
            }
        }
    }
    let entries = writer
        .finish(manifest)
        .with_context(|| format!("building index in {}", out.display()))?;

    eprintln!("pairs={pairs} repeats={repeats} entries={entries}");
    Ok(())
}

/// A `--sorted` line as its HGVS key and CA ID number, or why it is not
/// `HGVS<TAB>CA<digits>`.
fn parse_pair(line: &[u8]) -> Result<(&str, u64), String> {
    let line = std::str::from_utf8(line).map_err(|_| "not valid UTF-8".to_owned())?;
    let (hgvs, caid) = line
        .split_once('\t')
        .ok_or_else(|| format!("no tab in {line:?}"))?;
    if hgvs.is_empty() {
        return Err(format!("empty HGVS before {caid:?}"));
    }
    let caid = parse_caid(caid).ok_or_else(|| format!("CAID {caid:?} is not CA<digits>"))?;
    Ok((hgvs, caid))
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

    #[test]
    fn relative_path_climbs_to_the_common_ancestor() {
        let rel =
            |target: &str, base: &str| relative_path(Path::new(target), Path::new(base)).unwrap();
        assert_eq!(
            rel("/r/genome/GRCh38.bin", "/r/index"),
            Path::new("../genome/GRCh38.bin")
        );
        assert_eq!(
            rel("/r/index/GRCh38.bin", "/r/index"),
            Path::new("GRCh38.bin")
        );
        assert_eq!(
            rel("/GRCh38.bin", "/r/a/index"),
            Path::new("../../../GRCh38.bin")
        );
        // A shared name prefix (`index` and `index2`) is not a shared directory.
        assert_eq!(
            rel("/r/index2/GRCh38.bin", "/r/index"),
            Path::new("../index2/GRCh38.bin")
        );
    }

    #[test]
    fn canonical_out_resolves_the_existing_part_and_appends_the_rest() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        fs::create_dir(root.join("made")).unwrap();

        assert_eq!(
            canonical_out(&root.join("made")).unwrap(),
            root.join("made")
        );
        assert_eq!(
            canonical_out(&root.join("made/new/index")).unwrap(),
            root.join("made/new/index")
        );
        // `..` after an existing directory is the kernel's to resolve, and
        // after a missing one cancels it.
        assert_eq!(
            canonical_out(&root.join("made/../index")).unwrap(),
            root.join("index")
        );
        assert_eq!(
            canonical_out(&root.join("made/new/../index")).unwrap(),
            root.join("made/index")
        );
    }
}
