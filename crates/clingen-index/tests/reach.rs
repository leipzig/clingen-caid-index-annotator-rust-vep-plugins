//! End-to-end tests of `clingen-index reach` over the tiny genome, on indexes
//! built with `build --sorted`.

use std::fs;
use std::path::PathBuf;
use std::process::Output;

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use clingen_allele::test_support::tiny_genome;
use tempfile::TempDir;

fn cli() -> Command {
    cargo_bin_cmd!("clingen-index")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

/// An index and the tiny genome to check it against, alive as long as this.
struct Fixture {
    tmp: TempDir,
    genome_dir: TempDir,
}

impl Fixture {
    fn index(&self) -> PathBuf {
        self.tmp.path().join("index")
    }

    fn genome(&self) -> PathBuf {
        self.genome_dir.path().join("genome.bin")
    }
}

/// Index `pairs` of HGVS and CA number with `build --sorted`, sorting them
/// into the byte order it requires first, and recording the tiny genome in
/// the manifest when `record_genome` is set.
fn fixture(pairs: &[(&str, u64)], record_genome: bool) -> Fixture {
    let tmp = TempDir::new().unwrap();
    let (genome_dir, _genome) = tiny_genome();
    let mut pairs = pairs.to_vec();
    pairs.sort_by_key(|&(hgvs, _)| hgvs);
    let text: String = pairs
        .iter()
        .map(|(hgvs, caid)| format!("{hgvs}\tCA{caid}\n"))
        .collect();
    let pairs_file = tmp.path().join("pairs.tsv");
    fs::write(&pairs_file, text).unwrap();

    let mut build = cli();
    build
        .args(["build", "--sorted"])
        .arg(&pairs_file)
        .arg("--out")
        .arg(tmp.path().join("index"));
    if record_genome {
        build
            .arg("--genome")
            .arg(genome_dir.path().join("genome.bin"));
    }
    build.assert().success();
    Fixture { tmp, genome_dir }
}

/// Run `reach --index <fixture> --genome <tiny genome>` plus `extra`.
fn reach(fixture: &Fixture, extra: &[&str]) -> Output {
    cli()
        .arg("reach")
        .arg("--index")
        .arg(fixture.index())
        .arg("--genome")
        .arg(fixture.genome())
        .args(extra)
        .output()
        .unwrap()
}

/// One key of each form `canonical_hgvs` writes, with the IDs the brief
/// gives them.
const EVERY_FORM: [(&str, u64); 7] = [
    ("NC_000021.9:g.5030578C>T", 749862939),
    ("NC_000021.9:g.33000172del", 1),
    ("NC_000021.9:g.33000172dup", 42),
    ("NC_000021.9:g.33000126_33000127dup", 2),
    ("NC_000021.9:g.33000167_33000168insGA", 3),
    ("NC_000021.9:g.33000017_33000018delinsTG", 4),
    ("NC_012920.1:m.3243A>G", 5),
];

/// The Registry places this dup one base before where `canonical_hgvs` does,
/// so no VCF record looks it up.
const DUP_ONE_BASE_EARLY: (&str, u64) = ("NC_000021.9:g.33000171dup", 43);

#[test]
fn every_form_on_the_tiny_genome_is_reachable() {
    let fixture = fixture(&EVERY_FORM, true);
    // No --genome: the one the manifest names is used.
    let output = cli()
        .arg("reach")
        .arg("--index")
        .arg(fixture.index())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert_eq!(
        stdout_of(&output),
        "keys\t7\n\
         reachable\t7\n\
         wrong_ca\t0\n\
         not_found\t0\n\
         ref_mismatch\t0\n\
         unsupported\t0\n\
         reference\t0\n\
         unparseable\t0\n\
         class\tkeys\treachable\n\
         substitution\t1\t1\n\
         del\t1\t1\n\
         dup\t2\t2\n\
         ins\t1\t1\n\
         delins\t1\t1\n\
         m\t1\t1\n\
         other\t0\t0\n"
    );
}

#[test]
fn a_key_written_differently_is_not_found() {
    let fixture = fixture(&[DUP_ONE_BASE_EARLY], false);
    let output = reach(&fixture, &[]);
    let (stdout, stderr) = (stdout_of(&output), stderr_of(&output));
    assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
    assert!(stdout.contains("\nnot_found\t1\n"), "stdout: {stdout}");
    assert!(stdout.contains("\ndup\t1\t0\n"), "stdout: {stdout}");
    assert!(
        stderr.contains("ours=NC_000021.9:g.33000172dup"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("NC_000021.9:g.33000171dup"),
        "stderr: {stderr}"
    );
}

#[test]
fn wrong_ca_is_reported() {
    let fixture = fixture(
        &[DUP_ONE_BASE_EARLY, ("NC_000021.9:g.33000172dup", 42)],
        false,
    );
    let output = reach(&fixture, &[]);
    let (stdout, stderr) = (stdout_of(&output), stderr_of(&output));
    assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
    assert!(stdout.contains("\nwrong_ca\t1\n"), "stdout: {stdout}");
    assert!(stdout.contains("\nreachable\t1\n"), "stdout: {stdout}");
    assert!(stdout.contains("\nnot_found\t0\n"), "stdout: {stdout}");
    // The example names the key's own ID and the one its record found.
    let example = stderr
        .lines()
        .find(|l| l.contains("NC_000021.9:g.33000171dup"))
        .unwrap_or_else(|| panic!("no example for the key; stderr: {stderr}"));
    assert!(example.contains("CA000043"), "stderr: {stderr}");
    assert!(example.contains("CA000042"), "stderr: {stderr}");
}

#[test]
fn reference_and_unparseable_are_counted_not_failed() {
    let fixture = fixture(
        &[
            ("NC_000021.9:g.5030578=", 6),
            ("NC_000021.9:g.33000167_33000168ins(10)", 7),
        ],
        false,
    );
    let output = reach(&fixture, &[]);
    let (stdout, stderr) = (stdout_of(&output), stderr_of(&output));
    assert_eq!(output.status.code(), Some(0), "stderr: {stderr}");
    assert!(
        stdout.starts_with("keys\t2\nreachable\t0\n"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("\nreference\t1\n"), "stdout: {stdout}");
    assert!(stdout.contains("\nunparseable\t1\n"), "stdout: {stdout}");
    assert!(stdout.contains("\nins\t1\t0\n"), "stdout: {stdout}");
    assert!(stdout.contains("\nother\t1\t0\n"), "stdout: {stdout}");
    // The example says why the key could not be converted.
    assert!(stderr.contains("sequence is not ACGT"), "stderr: {stderr}");
}

#[test]
fn a_delins_that_changes_nothing_and_a_key_over_n_are_counted_not_failed() {
    // Both forms the live Registry holds in chrM 3009-5731: a reference
    // allele written as a delins (33000017-33000018 are CA), and a deletion
    // whose record would hold an N, which no lookup takes (the tiny genome is
    // N at chrM 3107, outside its 3200-3300 window, as the rCRS is).
    let fixture = fixture(
        &[
            ("NC_000021.9:g.33000017_33000018delinsCA", 8),
            ("NC_012920.1:m.3107del", 9),
        ],
        false,
    );
    let output = reach(&fixture, &[]);
    let (stdout, stderr) = (stdout_of(&output), stderr_of(&output));
    assert_eq!(output.status.code(), Some(0), "stderr: {stderr}");
    assert!(
        stdout.starts_with(
            "keys\t2\nreachable\t0\nwrong_ca\t0\nnot_found\t0\nref_mismatch\t0\n\
             unsupported\t0\nreference\t1\nunparseable\t1\n"
        ),
        "stdout: {stdout}"
    );
    assert!(
        stderr.contains(
            "unparseable: NC_012920.1:m.3107del CA000009 (reference sequence there is not ACGT)"
        ),
        "stderr: {stderr}"
    );
}

#[test]
fn prefix_limits_the_walk() {
    let fixture = fixture(&EVERY_FORM, false);
    let output = reach(&fixture, &["--prefix", "NC_012920.1:"]);
    let stdout = stdout_of(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(
        stdout.starts_with("keys\t1\nreachable\t1\n"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("\nm\t1\t1\n"), "stdout: {stdout}");
}

#[test]
fn examples_caps_the_lines_per_outcome() {
    // Both dups sit one base before where `canonical_hgvs` puts them.
    let fixture = fixture(
        &[
            DUP_ONE_BASE_EARLY,
            ("NC_000021.9:g.33000170_33000171dup", 44),
        ],
        false,
    );
    for (extra, lines) in [
        (&[][..], 2),
        (&["--examples", "1"][..], 1),
        (&["--examples", "0"][..], 0),
    ] {
        let output = reach(&fixture, extra);
        let stderr = stderr_of(&output);
        assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
        assert!(
            stdout_of(&output).contains("\nnot_found\t2\n"),
            "{extra:?}: stdout: {}",
            stdout_of(&output)
        );
        assert_eq!(
            stderr.matches("ours=").count(),
            lines,
            "{extra:?}: stderr: {stderr}"
        );
    }
}

#[test]
fn missing_genome_exits_2() {
    // Neither --genome nor a genome in the manifest.
    let fixture = fixture(&EVERY_FORM, false);
    let output = cli()
        .arg("reach")
        .arg("--index")
        .arg(fixture.index())
        .output()
        .unwrap();
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    assert!(stderr.contains("--genome"), "stderr: {stderr}");
    assert!(stdout_of(&output).is_empty());

    // A --genome that is not there.
    let missing = fixture.tmp.path().join("no-such-genome.bin");
    let output = cli()
        .arg("reach")
        .arg("--index")
        .arg(fixture.index())
        .arg("--genome")
        .arg(&missing)
        .output()
        .unwrap();
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    assert!(stderr.contains("no-such-genome.bin"), "stderr: {stderr}");
}
