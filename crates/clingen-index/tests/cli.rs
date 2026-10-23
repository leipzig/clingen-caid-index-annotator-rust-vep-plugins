//! End-to-end tests of the `clingen-index` binary over temp-dir fixtures.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use clingen_allele::test_support::tiny_genome;
use clingen_allele::{Annotator, CaIndex};
use tempfile::TempDir;

fn cli() -> Command {
    cargo_bin_cmd!("clingen-index")
}

/// Write `name` under `dir` as the harvester would: no header, one row per line.
fn write_chunk(dir: &Path, name: &str, rows: &[&str]) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    let mut text = rows.join("\n");
    text.push('\n');
    fs::write(&path, text).unwrap();
    path
}

fn stdout_of(output: &std::process::Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

const SNV: &str = "21\t5030578\tC\tT\tCA749862939\tNC_000021.9:g.5030578C>T\t";
const DUP: &str = "21\t33000166\tT\tTA\tCA100\tNC_000021.9:g.33000172dup\t";
const MITO: &str = "MT\t3243\tA\tG\tCA200\tNC_012920.1:m.3243A>G\t";
const UNREGISTERED_DEL: &str =
    "21\t33000166\tTA\tT\t\tNC_000021.9:g.33000172del\tNC_000021.8:g.34372480del";
const REGISTRY_ERROR: &str = "21\t10000000\tA\tG\tERROR:IncorrectReferenceAllele\t\t";

#[test]
fn build_from_chunks() {
    let tmp = TempDir::new().unwrap();
    let chunks = tmp.path().join("chunks");
    write_chunk(
        &chunks,
        "chunk_00000.tsv",
        &[SNV, UNREGISTERED_DEL, REGISTRY_ERROR, DUP],
    );
    // The same allele again with the same ID counts once.
    write_chunk(&chunks, "chunk_00001.tsv", &[DUP, MITO]);
    // Neither of these is a finished chunk, so neither may be read.
    write_chunk(
        &chunks,
        "chunk_00002.tsv.partial",
        &["21\t1\tA\tC\tCA999\tNC_000021.9:g.1A>C\t"],
    );
    write_chunk(&chunks, "notes.txt", &["not a chunk"]);
    let out = tmp.path().join("index");

    let output = cli()
        .args(["build", "--chunks"])
        .arg(&chunks)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(
        stderr_of(&output)
            .contains("rows=6 indexed=4 unregistered=1 errors=1 malformed=0 entries=3"),
        "stderr: {}",
        stderr_of(&output)
    );

    let index = CaIndex::open(&out).unwrap();
    assert_eq!(index.manifest().entries, 3);
    assert_eq!(index.manifest().assembly, "GRCh38");
    assert_eq!(index.manifest().format_version, 1);
    assert_eq!(index.manifest().vareffect_version, "0.6.0");
    assert_eq!(index.manifest().harvest_date, "unknown");
    assert!(index.manifest().sources.is_empty());
    assert_eq!(index.manifest().genome, None);
    assert_eq!(index.get("NC_000021.9:g.5030578C>T"), Some(749862939));
    assert_eq!(index.get("NC_000021.9:g.33000172dup"), Some(100));
    assert_eq!(index.get("NC_012920.1:m.3243A>G"), Some(200));
    // Unregistered and errored rows are not indexed; neither is a partial.
    assert_eq!(index.get("NC_000021.9:g.33000172del"), None);
    assert_eq!(index.get("NC_000021.9:g.1A>C"), None);
}

/// An empty stand-in genome file at `dir/refs/hg38.bin`; `build` only
/// records the path, it does not read the genome.
fn stand_in_genome(dir: &Path) -> PathBuf {
    let refs = dir.join("refs");
    fs::create_dir_all(&refs).unwrap();
    let path = refs.join("hg38.bin");
    fs::write(&path, b"").unwrap();
    path
}

#[test]
fn build_records_provenance_in_manifest() {
    let tmp = TempDir::new().unwrap();
    let chunks = tmp.path().join("chunks");
    write_chunk(&chunks, "chunk_00000.tsv", &[SNV]);
    let out = tmp.path().join("index");
    let genome = stand_in_genome(tmp.path());

    cli()
        .args(["build", "--chunks"])
        .arg(&chunks)
        .arg("--out")
        .arg(&out)
        .args(["--source", "gnomad4.1-chr21", "--source", "clinvar"])
        .arg("--genome")
        .arg(&genome)
        .args(["--harvest-date", "2026-10-23"])
        .assert()
        .success();

    let manifest = CaIndex::open(&out).unwrap().manifest().clone();
    assert_eq!(manifest.sources, ["gnomad4.1-chr21", "clinvar"]);
    let canonical = fs::canonicalize(&genome).unwrap();
    assert_eq!(manifest.genome.as_deref(), canonical.to_str());
    assert_eq!(manifest.harvest_date, "2026-10-23");
    assert_eq!(manifest.entries, 1);
}

#[test]
fn build_records_a_relative_genome_as_an_absolute_canonical_path() {
    let tmp = TempDir::new().unwrap();
    write_chunk(&tmp.path().join("chunks"), "chunk_00000.tsv", &[SNV]);
    let genome = stand_in_genome(tmp.path());

    // Relative to the working directory, with a `..` to resolve.
    cli()
        .current_dir(tmp.path())
        .args(["build", "--chunks", "chunks", "--out", "index"])
        .args(["--genome", "refs/../refs/hg38.bin"])
        .assert()
        .success();

    let index_dir = tmp.path().join("index");
    let recorded = CaIndex::open(&index_dir).unwrap().manifest().genome.clone();
    let canonical = fs::canonicalize(&genome).unwrap();
    assert!(canonical.is_absolute());
    assert_eq!(recorded.as_deref(), canonical.to_str());
    // Consumers resolve the recorded path against the index directory; an
    // absolute one comes back as it is, naming the genome given.
    assert_eq!(
        Annotator::genome_path_from_manifest(&index_dir),
        Some(canonical)
    );
}

#[test]
fn build_with_a_missing_or_non_file_genome_exits_2_and_writes_nothing() {
    let tmp = TempDir::new().unwrap();
    let chunks = tmp.path().join("chunks");
    write_chunk(&chunks, "chunk_00000.tsv", &[SNV]);
    let not_a_file = tmp.path().join("a-directory.bin");
    fs::create_dir_all(&not_a_file).unwrap();

    for (genome, says) in [
        (tmp.path().join("no-such-genome.bin"), "no-such-genome.bin"),
        (not_a_file, "is not a file"),
    ] {
        let out = tmp.path().join("index");
        let output = cli()
            .args(["build", "--chunks"])
            .arg(&chunks)
            .arg("--out")
            .arg(&out)
            .arg("--genome")
            .arg(&genome)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{}", genome.display());
        let stderr = stderr_of(&output);
        assert!(stderr.contains("--genome"), "stderr: {stderr}");
        assert!(stderr.contains(says), "stderr: {stderr}");
        assert!(
            !out.exists(),
            "{}: no index may be written",
            genome.display()
        );
    }
}

#[test]
fn build_reads_several_chunk_dirs_in_order() {
    let tmp = TempDir::new().unwrap();
    let first = tmp.path().join("first");
    let second = tmp.path().join("second");
    write_chunk(&first, "chunk_00000.tsv", &[SNV]);
    write_chunk(&second, "chunk_00000.tsv", &[MITO]);
    let out = tmp.path().join("index");

    cli()
        .args(["build", "--chunks"])
        .arg(&first)
        .arg(&second)
        .arg("--out")
        .arg(&out)
        .assert()
        .success();
    let index = CaIndex::open(&out).unwrap();
    assert_eq!(index.manifest().entries, 2);

    // The flag may also be repeated.
    let out2 = tmp.path().join("index2");
    cli()
        .arg("build")
        .arg("--chunks")
        .arg(&first)
        .arg("--chunks")
        .arg(&second)
        .arg("--out")
        .arg(&out2)
        .assert()
        .success();
    assert_eq!(CaIndex::open(&out2).unwrap().manifest().entries, 2);
}

#[test]
fn build_tolerates_crlf_and_blank_lines_and_counts_malformed_rows() {
    let tmp = TempDir::new().unwrap();
    let chunks = tmp.path().join("chunks");
    fs::create_dir_all(&chunks).unwrap();
    let text = format!(
        "{SNV}\r\n\r\n   \n21\t5\tA\n{DUP}\r\n\
         21\t6\tA\tC\tCAxyz\tNC_000021.9:g.6A>C\t\n\
         21\t7\tA\tC\tCA7\t\t\n"
    );
    fs::write(chunks.join("chunk_00000.tsv"), text).unwrap();
    let out = tmp.path().join("index");

    let output = cli()
        .args(["build", "--chunks"])
        .arg(&chunks)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let stderr = stderr_of(&output);
    // Blank lines are not rows; the short row, the unreadable CAID and the
    // registered row with no HGVS are malformed.
    assert!(
        stderr.contains("rows=5 indexed=2 unregistered=0 errors=0 malformed=3 entries=2"),
        "stderr: {stderr}"
    );
    let short_row_at = format!("{}:4", chunks.join("chunk_00000.tsv").display());
    assert!(stderr.contains(&short_row_at), "stderr: {stderr}");
    assert_eq!(
        CaIndex::open(&out).unwrap().get("NC_000021.9:g.5030578C>T"),
        Some(749862939)
    );
}

#[test]
fn build_shows_at_most_five_malformed_examples() {
    let tmp = TempDir::new().unwrap();
    let chunks = tmp.path().join("chunks");
    let rows: Vec<String> = (1..=8).map(|n| format!("21\t{n}\tA")).collect();
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    write_chunk(&chunks, "chunk_00000.tsv", &rows);

    let output = cli()
        .args(["build", "--chunks"])
        .arg(&chunks)
        .arg("--out")
        .arg(tmp.path().join("index"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = stderr_of(&output);
    assert!(stderr.contains("malformed=8"), "stderr: {stderr}");
    assert_eq!(
        stderr.matches("chunk_00000.tsv:").count(),
        5,
        "stderr: {stderr}"
    );
}

#[test]
fn build_conflict_exits_nonzero() {
    let tmp = TempDir::new().unwrap();
    let chunks = tmp.path().join("chunks");
    write_chunk(
        &chunks,
        "chunk_00000.tsv",
        &[
            "21\t33000166\tT\tTA\tCA100\tNC_000021.9:g.33000172dup\t",
            "21\t33000172\tA\tAA\tCA200\tNC_000021.9:g.33000172dup\t",
        ],
    );

    let output = cli()
        .args(["build", "--chunks"])
        .arg(&chunks)
        .arg("--out")
        .arg(tmp.path().join("index"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("NC_000021.9:g.33000172dup"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("CA100"), "stderr: {stderr}");
    assert!(stderr.contains("CA200"), "stderr: {stderr}");
    assert!(!tmp.path().join("index").join("caid.fst").exists());
}

#[test]
fn missing_or_empty_chunk_dir_is_an_error() {
    let tmp = TempDir::new().unwrap();
    let empty = tmp.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    write_chunk(&empty, "chunk_00000.tsv.partial", &[SNV]);

    for dir in [tmp.path().join("nope"), empty] {
        let output = cli()
            .args(["build", "--chunks"])
            .arg(&dir)
            .arg("--out")
            .arg(tmp.path().join("index"))
            .output()
            .unwrap();
        // 2, not 1: exit 1 is reserved for "a substitution failed validation".
        assert_eq!(output.status.code(), Some(2), "{}", dir.display());
        assert!(
            stderr_of(&output).contains(&dir.display().to_string()),
            "stderr: {}",
            stderr_of(&output)
        );
    }
}

/// Run `validate` over `rows` against the tiny genome; returns the process
/// output and the mismatch TSV text.
fn validate(rows: &[&str], extra: &[&str]) -> (std::process::Output, String) {
    let tmp = TempDir::new().unwrap();
    let (genome_dir, _genome) = tiny_genome();
    let chunks = tmp.path().join("chunks");
    write_chunk(&chunks, "chunk_00000.tsv", rows);
    let mismatches = tmp.path().join("mismatches.tsv");

    let output = cli()
        .args(["validate", "--chunks"])
        .arg(&chunks)
        .arg("--genome")
        .arg(genome_dir.path().join("genome.bin"))
        .arg("--mismatches")
        .arg(&mismatches)
        .args(extra)
        .output()
        .unwrap();
    let tsv = fs::read_to_string(&mismatches).unwrap_or_default();
    (output, tsv)
}

#[test]
fn validate_reports_classes() {
    let (output, tsv) = validate(
        &[
            SNV,
            DUP,
            MITO,
            // A del whose Registry HGVS we deliberately cannot reproduce.
            "21\t33000166\tTA\tT\tCA300\tNC_000021.9:g.33000999del\t",
            REGISTRY_ERROR,
        ],
        &[],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert_eq!(
        stdout_of(&output),
        "class\ttotal\tmatch\tpct\n\
         substitution\t1\t1\t100.0\n\
         del\t1\t0\t0.0\n\
         dup\t1\t1\t100.0\n\
         ins\t0\t0\t0.0\n\
         delins\t0\t0\t0.0\n\
         m\t1\t1\t100.0\n\
         other\t0\t0\t0.0\n"
    );
    assert_eq!(
        tsv,
        "CHROM\tPOS\tREF\tALT\tCLASS\tREGISTRY\tOURS\n\
         21\t33000166\tTA\tT\tdel\tNC_000021.9:g.33000999del\tNC_000021.9:g.33000172del\n"
    );
}

#[test]
fn validate_compares_unregistered_rows_and_skips_registry_errors() {
    let (output, tsv) = validate(&[UNREGISTERED_DEL, REGISTRY_ERROR], &[]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = stdout_of(&output);
    assert!(stdout.contains("\ndel\t1\t1\t100.0\n"), "stdout: {stdout}");
    assert_eq!(tsv.lines().count(), 1, "header only, got: {tsv}");
}

#[test]
fn validate_exits_1_when_a_substitution_mismatches() {
    let (output, tsv) = validate(
        &[
            // REF is wrong for this genome, so we cannot produce a key.
            "21\t5030578\tG\tT\tCA1\tNC_000021.9:g.5030578G>T\t",
            // The Registry names another allele altogether.
            "21\t5030578\tC\tT\tCA2\tNC_000021.9:g.5030579C>T\t",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout_of(&output).contains("\nsubstitution\t2\t0\t0.0\n"),
        "stdout: {}",
        stdout_of(&output)
    );
    let data: Vec<&str> = tsv.lines().skip(1).collect();
    assert_eq!(
        data,
        [
            "21\t5030578\tG\tT\tsubstitution\tNC_000021.9:g.5030578G>T\tRefMismatch",
            "21\t5030578\tC\tT\tsubstitution\tNC_000021.9:g.5030579C>T\tNC_000021.9:g.5030578C>T",
        ]
    );
}

#[test]
fn validate_counts_unsupported_and_genome_errors_as_mismatches() {
    let (output, tsv) = validate(
        &[
            // The tiny genome has no chr7, so the lookup itself fails.
            "7\t100\tA\tC\tCA5\tNC_000007.14:g.100A>C\t",
            // An ambiguity code is not a variant we key.
            "21\t5030578\tC\tN\tCA6\tNC_000021.9:g.5030578C>N\t",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(1));
    let ours: Vec<&str> = tsv
        .lines()
        .skip(1)
        .map(|l| l.split('\t').nth(6).unwrap())
        .collect();
    assert!(ours[0].starts_with("Error:"), "ours: {ours:?}");
    assert_eq!(ours[1], "Unsupported");
}

#[test]
fn validate_sample_takes_the_first_n_compared_rows() {
    let (output, _) = validate(
        &[
            REGISTRY_ERROR,
            SNV,
            // A substitution mismatch after the sample must not be seen.
            "21\t5030578\tC\tT\tCA2\tNC_000021.9:g.5030579C>T\t",
        ],
        &["--sample", "1"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(
        stdout_of(&output).contains("\nsubstitution\t1\t1\t100.0\n"),
        "stdout: {}",
        stdout_of(&output)
    );
}

#[test]
fn validate_classifies_by_registry_string() {
    // None of these can match (REF is not what the genome has there), so each
    // one lands in the mismatch TSV with the class taken from its REGISTRY string.
    let (output, tsv) = validate(
        &[
            "21\t1\tA\tC\tCA1\tNC_000021.9:g.10_11insAC\t",
            "21\t1\tA\tC\tCA2\tNC_000021.9:g.10_12delinsAC\t",
            "21\t1\tA\tC\tCA3\tNC_000021.9:g.10_12del\t",
            "21\t1\tA\tC\tCA4\tNC_000021.9:g.10_12dup\t",
            "MT\t1\tA\tC\tCA5\tNC_012920.1:m.10A>C\t",
            "21\t1\tA\tC\tCA6\tNC_000021.9:g.10inv\t",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(0));
    let classes: Vec<&str> = tsv
        .lines()
        .skip(1)
        .map(|l| l.split('\t').nth(4).unwrap())
        .collect();
    assert_eq!(classes, ["ins", "delins", "del", "dup", "m", "other"]);
}

#[test]
fn validate_counts_malformed_rows_without_aborting() {
    let (output, _) = validate(
        &[
            "21\t5030578",
            SNV,
            "21\tnot-a-number\tA\tC\tCA9\tNC_000021.9:g.1A>C\t",
        ],
        &[],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stderr = stderr_of(&output);
    assert!(stderr.contains("malformed=2"), "stderr: {stderr}");
    assert!(stdout_of(&output).contains("\nsubstitution\t1\t1\t100.0\n"));
}
