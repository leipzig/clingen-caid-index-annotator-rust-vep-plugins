//! End-to-end tests of `clingen-index build --sorted` and `--genome-relative`
//! over temp-dir fixtures.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use clingen_allele::{Annotator, CaIndex};
use flate2::Compression;
use flate2::write::GzEncoder;
use tempfile::TempDir;

fn cli() -> Command {
    cargo_bin_cmd!("clingen-index")
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

/// Fail unless the run exited 2 (could not complete); return its stderr.
fn stderr_of_exit_2(output: &Output) -> String {
    let stderr = stderr_of(output);
    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    stderr
}

/// Write `lines` to `dir/name`, one per line, and return the path.
fn write_lines(dir: &Path, name: &str, lines: &[&str]) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    let mut text = lines.join("\n");
    text.push('\n');
    fs::write(&path, text).unwrap();
    path
}

/// Run `build --sorted <files> --out <out>` plus `extra`.
fn build_sorted(files: &[&Path], out: &Path, extra: &[&str]) -> Output {
    cli()
        .args(["build", "--sorted"])
        .args(files)
        .arg("--out")
        .arg(out)
        .args(extra)
        .output()
        .unwrap()
}

/// Fail unless `dir` holds no index files, finished or temporary. The
/// directory itself may exist: the writer makes it before the first pair.
fn assert_no_index_files(dir: &Path) {
    for name in [
        "caid.fst",
        "manifest.json",
        "caid.fst.tmp",
        "manifest.json.tmp",
    ] {
        assert!(
            !dir.join(name).exists(),
            "{} must not exist",
            dir.join(name).display()
        );
    }
}

// The registered rows of `cli.rs`'s `build_from_chunks`, as chunk rows and as
// the pairs they index, in byte order.
const SNV: &str = "21\t5030578\tC\tT\tCA749862939\tNC_000021.9:g.5030578C>T\t";
const DUP: &str = "21\t33000166\tT\tTA\tCA100\tNC_000021.9:g.33000172dup\t";
const MITO: &str = "MT\t3243\tA\tG\tCA200\tNC_012920.1:m.3243A>G\t";
const PAIRS: [&str; 3] = [
    "NC_000021.9:g.33000172dup\tCA100",
    "NC_000021.9:g.5030578C>T\tCA749862939",
    "NC_012920.1:m.3243A>G\tCA200",
];

#[test]
fn sorted_build_equals_chunk_build() {
    let tmp = TempDir::new().unwrap();
    let chunks = tmp.path().join("chunks");
    write_lines(&chunks, "chunk_00000.tsv", &[SNV, DUP, MITO]);
    let from_chunks = tmp.path().join("from_chunks");
    cli()
        .args(["build", "--chunks"])
        .arg(&chunks)
        .arg("--out")
        .arg(&from_chunks)
        .assert()
        .success();

    let pairs = write_lines(tmp.path(), "pairs.tsv", &PAIRS);
    let from_sorted = tmp.path().join("from_sorted");
    let output = build_sorted(&[&pairs], &from_sorted, &[]);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("pairs=3 repeats=0 entries=3"),
        "stderr: {stderr}"
    );

    assert_eq!(
        fs::read(from_sorted.join("caid.fst")).unwrap(),
        fs::read(from_chunks.join("caid.fst")).unwrap(),
        "caid.fst must be byte-identical"
    );
    let sorted = CaIndex::open(&from_sorted).unwrap();
    let chunked = CaIndex::open(&from_chunks).unwrap();
    assert_eq!(sorted.manifest().entries, 3);
    assert_eq!(sorted.manifest(), chunked.manifest());
    assert_eq!(sorted.get("NC_000021.9:g.5030578C>T"), Some(749862939));
}

#[test]
fn sorted_build_across_files_in_accession_order() {
    let tmp = TempDir::new().unwrap();
    let shards = tmp.path().join("shards");
    let chr9 = write_lines(
        &shards,
        "NC_000009.12.tsv",
        &["NC_000009.12:g.100A>G\tCA1", "NC_000009.12:g.200C>T\tCA2"],
    );
    let chr10 = write_lines(&shards, "NC_000010.11.tsv", &["NC_000010.11:g.50G>A\tCA3"]);
    let mito = write_lines(
        &shards,
        "NC_012920.1.tsv",
        &[
            "NC_012920.1:m.3243A>G\tCA200",
            "NC_012920.1:m.8993T>G\tCA201",
        ],
    );

    let out = tmp.path().join("index");
    let output = build_sorted(&[&chr9, &chr10, &mito], &out, &[]);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(
        stderr_of(&output).contains("pairs=5 repeats=0 entries=5"),
        "stderr: {}",
        stderr_of(&output)
    );
    let index = CaIndex::open(&out).unwrap();
    assert_eq!(index.get("NC_000010.11:g.50G>A"), Some(3));
    assert_eq!(index.get("NC_012920.1:m.8993T>G"), Some(201));

    // Reversed, the second file's first key sorts below the first file's last.
    let reversed = tmp.path().join("reversed");
    let output = build_sorted(&[&mito, &chr10, &chr9], &reversed, &[]);
    let stderr = stderr_of_exit_2(&output);
    let at = format!("keys out of order at {}:1", chr10.display());
    assert!(stderr.contains(&at), "stderr: {stderr}");
    assert!(
        stderr.contains("NC_000010.11:g.50G>A after NC_012920.1:m.8993T>G"),
        "stderr: {stderr}"
    );
    assert_no_index_files(&reversed);
}

#[test]
fn sorted_build_skips_exact_repeats_and_rejects_conflicts() {
    let tmp = TempDir::new().unwrap();
    // A blank and a whitespace-only line are skipped, not counted as pairs.
    let pairs = write_lines(
        tmp.path(),
        "pairs.tsv",
        &[
            "NC_000021.9:g.10A>C\tCA1",
            "",
            "NC_000021.9:g.10A>C\tCA1",
            "  ",
            "NC_000021.9:g.20G>T\tCA2",
        ],
    );
    let out = tmp.path().join("index");
    let output = build_sorted(&[&pairs], &out, &[]);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(
        stderr_of(&output).contains("pairs=3 repeats=1 entries=2"),
        "stderr: {}",
        stderr_of(&output)
    );
    let fst_before = fs::read(out.join("caid.fst")).unwrap();
    let manifest_before = fs::read(out.join("manifest.json")).unwrap();

    // The same key with another ID, rebuilt over the index just made.
    let conflicting = write_lines(
        tmp.path(),
        "conflicting.tsv",
        &["NC_000021.9:g.10A>C\tCA1", "NC_000021.9:g.10A>C\tCA3"],
    );
    let output = build_sorted(&[&conflicting], &out, &[]);
    let stderr = stderr_of_exit_2(&output);
    assert!(stderr.contains("NC_000021.9:g.10A>C"), "stderr: {stderr}");
    assert!(stderr.contains("CA000001"), "stderr: {stderr}");
    assert!(stderr.contains("CA000003"), "stderr: {stderr}");
    let at = format!("{}:2", conflicting.display());
    assert!(stderr.contains(&at), "stderr: {stderr}");

    // The old index is intact and nothing temporary is left beside it.
    assert_eq!(fs::read(out.join("caid.fst")).unwrap(), fst_before);
    assert_eq!(
        fs::read(out.join("manifest.json")).unwrap(),
        manifest_before
    );
    assert!(!out.join("caid.fst.tmp").exists());
    assert!(!out.join("manifest.json.tmp").exists());
    let index = CaIndex::open(&out).unwrap();
    assert_eq!(index.manifest().entries, 2);
    assert_eq!(index.get("NC_000021.9:g.20G>T"), Some(2));
}

#[test]
fn sorted_build_rejects_locale_order() {
    let tmp = TempDir::new().unwrap();
    // Natural order; in byte order `100A` sorts before `10A`.
    let pairs = write_lines(
        tmp.path(),
        "pairs.tsv",
        &["NC_000021.9:g.10A>C\tCA1", "NC_000021.9:g.100A>C\tCA2"],
    );
    let out = tmp.path().join("index");
    let output = build_sorted(&[&pairs], &out, &[]);
    let stderr = stderr_of_exit_2(&output);
    let at = format!(
        "keys out of order at {}:2: NC_000021.9:g.100A>C after NC_000021.9:g.10A>C",
        pairs.display()
    );
    assert!(stderr.contains(&at), "stderr: {stderr}");
    assert_no_index_files(&out);
}

#[test]
fn sorted_build_malformed_pair_exits_2() {
    let tmp = TempDir::new().unwrap();
    for (bad, says) in [
        ("NC_000021.9:g.20G>T CA2", "no tab"),
        ("NC_000021.9:g.20G>T\tCA", "\"CA\""),
        ("\tCA2", "empty HGVS"),
    ] {
        // Line 2 is blank: skipped, but still counted for the line number.
        let pairs = write_lines(
            tmp.path(),
            "pairs.tsv",
            &["NC_000021.9:g.10A>C\tCA1", "", bad],
        );
        let out = tmp.path().join("index");
        let output = build_sorted(&[&pairs], &out, &[]);
        let stderr = stderr_of_exit_2(&output);
        let at = format!("malformed pair at {}:3", pairs.display());
        assert!(stderr.contains(&at), "{bad:?}: stderr: {stderr}");
        assert!(stderr.contains(says), "{bad:?}: stderr: {stderr}");
        assert_no_index_files(&out);
    }
}

#[test]
fn sorted_build_reads_gzip_and_standard_input() {
    let tmp = TempDir::new().unwrap();
    let plain = write_lines(tmp.path(), "pairs.tsv", &PAIRS);
    let expected = tmp.path().join("plain");
    let output = build_sorted(&[&plain], &expected, &[]);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let expected = fs::read(expected.join("caid.fst")).unwrap();

    // Told by the magic bytes, not the name: no `.gz` here.
    let gzipped = tmp.path().join("pairs.bin");
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(&fs::read(&plain).unwrap()).unwrap();
    fs::write(&gzipped, gz.finish().unwrap()).unwrap();
    let from_gzip = tmp.path().join("gzip");
    let output = build_sorted(&[&gzipped], &from_gzip, &[]);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert_eq!(fs::read(from_gzip.join("caid.fst")).unwrap(), expected);

    let from_stdin = tmp.path().join("stdin");
    let output = cli()
        .args(["build", "--sorted", "-", "--out"])
        .arg(&from_stdin)
        .write_stdin(fs::read(&plain).unwrap())
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert_eq!(fs::read(from_stdin.join("caid.fst")).unwrap(), expected);
}

#[test]
fn sorted_build_with_a_missing_file_exits_2_before_making_out() {
    let tmp = TempDir::new().unwrap();
    let pairs = write_lines(tmp.path(), "pairs.tsv", &PAIRS);
    let missing = tmp.path().join("no-such-pairs.tsv");
    let out = tmp.path().join("index");

    let output = build_sorted(&[&pairs, &missing], &out, &[]);
    let stderr = stderr_of_exit_2(&output);
    assert!(stderr.contains("no-such-pairs.tsv"), "stderr: {stderr}");
    // Checked before the first file is read, so not even `--out` is made.
    assert!(!out.exists());
}

#[test]
fn genome_relative_is_recorded_relative_to_out() {
    let tmp = TempDir::new().unwrap();
    let pairs = write_lines(tmp.path(), "pairs.tsv", &PAIRS);
    // `build` only records the genome's path; an empty stand-in will do.
    let genome_dir = tmp.path().join("genome");
    fs::create_dir_all(&genome_dir).unwrap();
    let genome = genome_dir.join("GRCh38.bin");
    fs::write(&genome, b"").unwrap();
    let out = tmp.path().join("index");

    let output = build_sorted(
        &[&pairs],
        &out,
        &["--genome", genome.to_str().unwrap(), "--genome-relative"],
    );
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let index = CaIndex::open(&out).unwrap();
    assert_eq!(
        index.manifest().genome.as_deref(),
        Some("../genome/GRCh38.bin")
    );
    // Consumers resolve it against the index directory.
    let resolved = Annotator::genome_path_from_manifest(&out).unwrap();
    assert!(resolved.is_file(), "{}", resolved.display());
    assert_eq!(
        fs::canonicalize(&resolved).unwrap(),
        fs::canonicalize(&genome).unwrap()
    );

    // Without `--genome` there is nothing to record.
    let output = build_sorted(
        &[&pairs],
        &tmp.path().join("index2"),
        &["--genome-relative"],
    );
    let stderr = stderr_of_exit_2(&output);
    assert!(stderr.contains("--genome"), "stderr: {stderr}");
}

#[cfg(unix)]
#[test]
fn genome_relative_resolves_a_symlinked_out_not_yet_made() {
    // `--out` names a directory the build has yet to make, under a symlink:
    // made canonical, it sits beside `genome/`, so the recorded path is the
    // same as when `--out` is given directly.
    let tmp = TempDir::new().unwrap();
    let pairs = write_lines(tmp.path(), "pairs.tsv", &PAIRS);
    let genome_dir = tmp.path().join("genome");
    fs::create_dir_all(&genome_dir).unwrap();
    fs::write(genome_dir.join("GRCh38.bin"), b"").unwrap();
    let elsewhere = TempDir::new().unwrap();
    let link = elsewhere.path().join("link");
    std::os::unix::fs::symlink(tmp.path(), &link).unwrap();
    let out = link.join("index");

    let output = build_sorted(
        &[&pairs],
        &out,
        &[
            "--genome",
            genome_dir.join("GRCh38.bin").to_str().unwrap(),
            "--genome-relative",
        ],
    );
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert_eq!(
        CaIndex::open(&out).unwrap().manifest().genome.as_deref(),
        Some("../genome/GRCh38.bin")
    );
}

#[test]
fn chunks_and_sorted_are_exclusive() {
    let tmp = TempDir::new().unwrap();
    let chunks = tmp.path().join("chunks");
    write_lines(&chunks, "chunk_00000.tsv", &[SNV]);
    let pairs = write_lines(tmp.path(), "pairs.tsv", &PAIRS);
    let out = tmp.path().join("index");

    let output = cli()
        .args(["build", "--chunks"])
        .arg(&chunks)
        .arg("--sorted")
        .arg(&pairs)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    let stderr = stderr_of_exit_2(&output);
    assert!(stderr.contains("cannot be used with"), "stderr: {stderr}");
    assert!(!out.exists());

    // One of the two is required.
    let output = cli().args(["build", "--out"]).arg(&out).output().unwrap();
    let stderr = stderr_of_exit_2(&output);
    assert!(
        stderr.contains("--chunks") && stderr.contains("--sorted"),
        "stderr: {stderr}"
    );
    assert!(!out.exists());
}
