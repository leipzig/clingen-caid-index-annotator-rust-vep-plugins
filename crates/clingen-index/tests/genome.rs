//! End-to-end tests of `clingen-index genome` over FASTA fixtures built here.
//!
//! The full-size fixtures are GRCh38's `chr21` and `chrM` as `N` with the
//! committed reference windows pasted in, the same shape as
//! `clingen_allele::test_support::tiny_genome`; the cases that do not need a
//! full-size contig use `chrM` or a few hundred bases.

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::OnceLock;

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use clingen_allele::test_support::{CHR21_LEN, CHRM_LEN, windows};
use flate2::Compression;
use flate2::write::GzEncoder;
use tempfile::TempDir;
use vareffect::FastaReader;
use vareffect::fasta::write_genome_binary;

fn cli() -> Command {
    cargo_bin_cmd!("clingen-index")
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

/// `N` x `len` with every window on `chrom` pasted in at its 1-based offset.
fn contig_with_windows(chrom: &str, len: usize) -> Vec<u8> {
    let mut seq = vec![b'N'; len];
    for w in windows().iter().filter(|w| w.chrom == chrom) {
        let start = usize::try_from(w.start1 - 1).unwrap();
        seq[start..start + w.seq.len()].copy_from_slice(&w.seq);
    }
    seq
}

/// The two full-size contigs, built once for the whole test binary.
fn full_contigs() -> &'static (Vec<u8>, Vec<u8>) {
    static CONTIGS: OnceLock<(Vec<u8>, Vec<u8>)> = OnceLock::new();
    CONTIGS.get_or_init(|| {
        (
            contig_with_windows("chr21", CHR21_LEN),
            contig_with_windows("chrM", CHRM_LEN),
        )
    })
}

/// One FASTA record: the header line, then the sequence in 60-base lines.
fn record(header: &str, seq: &[u8]) -> Vec<u8> {
    let mut out = format!(">{header}\n").into_bytes();
    for line in seq.chunks(60) {
        out.extend_from_slice(line);
        out.push(b'\n');
    }
    out
}

/// Records one after another, as a FASTA file holds them.
fn fasta(records: &[(&str, &[u8])]) -> Vec<u8> {
    records
        .iter()
        .flat_map(|(header, seq)| record(header, seq))
        .collect()
}

/// `data` as a BGZF-shaped gzip file: many members, cut mid-line, and the
/// empty member BGZF ends with. `GzDecoder` would stop after the first one.
fn bgzf_like(data: &[u8]) -> Vec<u8> {
    let member = |block: &[u8]| {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(block).unwrap();
        encoder.finish().unwrap()
    };
    let mut out: Vec<u8> = data.chunks(1 << 20).flat_map(member).collect();
    out.extend(member(b""));
    out
}

/// `<path><suffix>`, the way the tool names the index and temporary files.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// The `.bin` and `.bin.idx` bytes at `bin`.
fn read_genome_files(bin: &Path) -> (Vec<u8>, Vec<u8>) {
    (
        fs::read(bin).unwrap(),
        fs::read(with_suffix(bin, ".idx")).unwrap(),
    )
}

/// What `write_genome_binary` makes of `contigs` (build `GRCh38`): the bytes
/// the tool's output must equal.
fn reference_genome(dir: &Path, contigs: &[(&str, &[u8])]) -> (Vec<u8>, Vec<u8>) {
    let bin = dir.join("reference.bin");
    write_genome_binary(contigs, "GRCh38", &bin, &with_suffix(&bin, ".idx")).unwrap();
    read_genome_files(&bin)
}

/// Nothing at `out` or beside it: neither the genome, its index nor a
/// leftover temporary file.
fn assert_nothing_written(out: &Path) {
    for suffix in ["", ".idx", ".tmp", ".idx.tmp"] {
        let path = with_suffix(out, suffix);
        assert!(!path.exists(), "{} must not exist", path.display());
    }
}

fn run_genome(fasta: &Path, extra: &[&str], out: &Path) -> Output {
    cli()
        .arg("genome")
        .arg("--fasta")
        .arg(fasta)
        .args(extra)
        .arg("--out")
        .arg(out)
        .output()
        .unwrap()
}

/// Run `genome` on `text` as a FASTA file; assert it exits 2 and wrote
/// nothing; return stderr.
fn failure_of(tmp: &TempDir, text: &[u8], extra: &[&str]) -> String {
    let path = tmp.path().join("input.fa");
    fs::write(&path, text).unwrap();
    let out = tmp.path().join("out").join("genome.bin");
    fs::create_dir_all(out.parent().unwrap()).unwrap();
    let output = run_genome(&path, extra, &out);
    assert_eq!(output.status.code(), Some(2), "{}", stderr_of(&output));
    assert_nothing_written(&out);
    stderr_of(&output)
}

#[test]
fn genome_writes_requested_contigs() {
    let (chr21, chrm) = full_contigs();
    let tmp = TempDir::new().unwrap();
    // chrM first and soft-masked: the output follows the FASTA's order, not
    // the contig table's, and writes uppercase.
    let chrm_soft_masked = chrm.to_ascii_lowercase();
    let text = fasta(&[("chrM", &chrm_soft_masked), ("chr21", chr21)]);
    let input = tmp.path().join("hg38.fa");
    fs::write(&input, &text).unwrap();
    let out = tmp.path().join("genome.bin");

    let output = run_genome(&input, &["--contigs", "21,MT"], &out);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(
        stderr_of(&output).contains("contigs=2 bases=46726552"),
        "stderr: {}",
        stderr_of(&output)
    );

    let genome = FastaReader::open(&out).unwrap();
    assert_eq!(genome.chrom_length("chr21"), Some(46_709_983));
    assert_eq!(genome.chrom_length("chrM"), Some(16_569));
    assert_eq!(genome.fetch_base("chr21", 5_030_577).unwrap(), b'C');
    assert_eq!(genome.fetch_base("chrM", 3_242).unwrap(), b'A');
    // Byte for byte what vareffect writes for those contigs in that order.
    let reference = reference_genome(tmp.path(), &[("chrM", chrm), ("chr21", chr21)]);
    assert!(
        read_genome_files(&out) == reference,
        "output differs from write_genome_binary(chrM, chr21)"
    );
    // The temporary files were renamed away.
    assert!(!with_suffix(&out, ".tmp").exists());
    assert!(!with_suffix(&out, ".idx.tmp").exists());
}

#[test]
fn genome_reads_gzip_from_stdin() {
    let (chr21, chrm) = full_contigs();
    let tmp = TempDir::new().unwrap();
    let text = fasta(&[("chr21", chr21), ("chrM", chrm)]);
    let input = tmp.path().join("hg38.fa");
    fs::write(&input, &text).unwrap();
    let plain_out = tmp.path().join("plain.bin");
    let gz_out = tmp.path().join("gz.bin");

    let plain = run_genome(&input, &["--contigs", "21,MT"], &plain_out);
    assert!(plain.status.success(), "stderr: {}", stderr_of(&plain));
    let gz = cli()
        .args(["genome", "--fasta", "-", "--contigs", "21,MT", "--out"])
        .arg(&gz_out)
        .write_stdin(bgzf_like(&text))
        .output()
        .unwrap();
    assert!(gz.status.success(), "stderr: {}", stderr_of(&gz));

    assert!(stderr_of(&gz).contains("contigs=2 bases=46726552"));
    assert!(
        read_genome_files(&gz_out) == read_genome_files(&plain_out),
        "gzip and plain input must give identical files"
    );
}

#[test]
fn genome_reads_plain_fasta_from_stdin() {
    // The shape scripts/build_chr21_genome.sh uses: `gunzip -c | genome`.
    let (_, chrm) = full_contigs();
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("genome.bin");

    let output = cli()
        .args(["genome", "--fasta", "-", "--contigs", "chrM", "--out"])
        .arg(&out)
        .write_stdin(fasta(&[("chrM", chrm)]))
        .output()
        .unwrap();
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(stderr_of(&output).contains("contigs=1 bases=16569"));
    let genome = FastaReader::open(&out).unwrap();
    assert_eq!(genome.chrom_length("chrM"), Some(16_569));
}

#[test]
fn genome_refuses_a_wrong_length() {
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("input.fa");
    fs::write(&input, fasta(&[("chr21", &[b'A'; 1000])])).unwrap();
    let out = tmp.path().join("genome.bin");
    // A genome already there must survive the failed run.
    fs::write(&out, b"previous genome").unwrap();
    fs::write(with_suffix(&out, ".idx"), b"previous index").unwrap();

    let output = run_genome(&input, &["--contigs", "21"], &out);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr_of(&output).contains("contig chr21 is 1000 bases long; GRCh38 chr21 is 46709983"),
        "stderr: {}",
        stderr_of(&output)
    );
    assert_eq!(fs::read(&out).unwrap(), b"previous genome");
    assert_eq!(
        fs::read(with_suffix(&out, ".idx")).unwrap(),
        b"previous index"
    );
    assert!(!with_suffix(&out, ".tmp").exists());
    assert!(!with_suffix(&out, ".idx.tmp").exists());
}

#[test]
fn genome_refuses_a_wrong_length_with_no_previous_output() {
    let tmp = TempDir::new().unwrap();
    // Too long as well as too short: chrM is 16569 bases.
    let stderr = failure_of(
        &tmp,
        &fasta(&[("chrM", &[b'C'; 16_570])]),
        &["--contigs", "MT"],
    );
    assert!(
        stderr.contains("contig chrM is 16570 bases long; GRCh38 chrM is 16569"),
        "stderr: {stderr}"
    );
}

#[test]
fn genome_missing_or_duplicate_contig_exits_2() {
    let (_, chrm) = full_contigs();
    let tmp = TempDir::new().unwrap();

    // A requested contig that is not in the FASTA, named.
    let stderr = failure_of(&tmp, &fasta(&[("chrM", chrm)]), &["--contigs", "21,MT"]);
    assert!(stderr.contains("chr21"), "stderr: {stderr}");
    assert!(stderr.contains("not found"), "stderr: {stderr}");

    // The default is all 25 primary contigs, so chrM alone is not enough.
    let stderr = failure_of(&tmp, &fasta(&[("chrM", chrm)]), &[]);
    assert!(stderr.contains("chr1,"), "stderr: {stderr}");
    assert!(stderr.contains("chrY"), "stderr: {stderr}");
    assert!(!stderr.contains("chrM"), "chrM was given: {stderr}");

    // The same contig twice, under two spellings.
    let twice = fasta(&[("MT", chrm), ("chrM", chrm)]);
    let stderr = failure_of(&tmp, &twice, &["--contigs", "MT"]);
    assert!(stderr.contains("chrM"), "stderr: {stderr}");
    assert!(stderr.contains("twice"), "stderr: {stderr}");
    assert!(stderr.contains("line "), "stderr: {stderr}");
}

#[test]
fn genome_skips_unrequested_and_non_primary_records() {
    let (chr21, chrm) = full_contigs();
    let tmp = TempDir::new().unwrap();
    // The alt scaffold comes before and the unplaced one after chr21; neither
    // may be taken for chr21 (or for a second copy of it). chr22 is a primary
    // contig, but not a requested one, so its short length is nobody's
    // business.
    let text = fasta(&[
        ("chr21_KI270872v1_alt", b"ACGTACGTAC"),
        ("chr21", chr21),
        ("chrUn_GL000220v1", b"ACGTACGTAC"),
        ("chr22", &[b'A'; 50]),
        ("chrM", chrm),
    ]);
    let input = tmp.path().join("hg38.fa");
    fs::write(&input, &text).unwrap();
    let out = tmp.path().join("genome.bin");

    let output = run_genome(&input, &["--contigs", "21,MT"], &out);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert!(stderr_of(&output).contains("contigs=2 bases=46726552"));

    let genome = FastaReader::open(&out).unwrap();
    assert_eq!(genome.chrom_length("chr21"), Some(46_709_983));
    assert_eq!(genome.chrom_length("chrM"), Some(16_569));
    assert_eq!(genome.chrom_length("chr22"), None);
    assert_eq!(genome.chrom_length("chr21_KI270872v1_alt"), None);
    assert_eq!(genome.chrom_length("chrUn_GL000220v1"), None);
    let reference = reference_genome(tmp.path(), &[("chr21", chr21), ("chrM", chrm)]);
    assert!(read_genome_files(&out) == reference);
}

#[test]
fn genome_accepts_ensembl_and_accession_headers() {
    let (chr21, chrm) = full_contigs();
    let tmp = TempDir::new().unwrap();
    let ensembl = fasta(&[
        (
            "21 dna:chromosome chromosome:GRCh38:21:1:46709983:1 REF",
            chr21,
        ),
        ("MT dna:chromosome chromosome:GRCh38:MT:1:16569:1 REF", chrm),
    ]);
    let input = tmp.path().join("ensembl.fa");
    fs::write(&input, &ensembl).unwrap();
    let out = tmp.path().join("ensembl.bin");

    let output = run_genome(&input, &["--contigs", "21,MT"], &out);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    // Written under vareffect's names, whatever the FASTA called them.
    let genome = FastaReader::open(&out).unwrap();
    assert_eq!(genome.chrom_length("chr21"), Some(46_709_983));
    assert_eq!(genome.chrom_length("chrM"), Some(16_569));
    assert_eq!(genome.fetch_base("chr21", 5_030_577).unwrap(), b'C');
    let reference = reference_genome(tmp.path(), &[("chr21", chr21), ("chrM", chrm)]);
    assert!(read_genome_files(&out) == reference);

    // An NC_ accession header, requested by accession.
    let refseq = fasta(&[(
        "NC_012920.1 Homo sapiens mitochondrion, complete genome",
        chrm,
    )]);
    let input = tmp.path().join("refseq.fa");
    fs::write(&input, &refseq).unwrap();
    let out = tmp.path().join("refseq.bin");
    let output = run_genome(&input, &["--contigs", "NC_012920.1"], &out);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    assert_eq!(
        FastaReader::open(&out).unwrap().chrom_length("chrM"),
        Some(16_569)
    );
}

#[test]
fn genome_names_the_line_of_a_bad_base() {
    let (_, chrm) = full_contigs();
    let tmp = TempDir::new().unwrap();
    let mut seq = chrm.clone();
    // Base 130 is on line 3 of the record, so line 4 of the file.
    seq[129] = b'X';

    let stderr = failure_of(&tmp, &fasta(&[("chrM", &seq)]), &["--contigs", "MT"]);
    assert!(stderr.contains("line 4"), "stderr: {stderr}");
    assert!(stderr.contains("'X'"), "stderr: {stderr}");
    assert!(stderr.contains("chrM"), "stderr: {stderr}");
    assert!(stderr.contains("input.fa"), "stderr: {stderr}");
}

#[test]
fn genome_refuses_input_that_is_not_fasta() {
    let tmp = TempDir::new().unwrap();
    let stderr = failure_of(&tmp, b"<html>404 Not Found</html>\n", &["--contigs", "MT"]);
    assert!(stderr.contains("line 1"), "stderr: {stderr}");
    assert!(stderr.contains("header"), "stderr: {stderr}");
}

#[test]
fn genome_refuses_a_truncated_gzip() {
    let (_, chrm) = full_contigs();
    let tmp = TempDir::new().unwrap();
    let gz = bgzf_like(&fasta(&[("chrM", chrm)]));
    let input = tmp.path().join("cut.fa.gz");
    fs::write(&input, &gz[..gz.len() / 2]).unwrap();
    let out = tmp.path().join("genome.bin");

    let output = run_genome(&input, &["--contigs", "MT"], &out);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr_of(&output).contains("cut.fa.gz"),
        "{}",
        stderr_of(&output)
    );
    assert_nothing_written(&out);
}

#[test]
fn genome_rejects_a_bad_contig_list_before_reading() {
    let tmp = TempDir::new().unwrap();
    let missing_input = tmp.path().join("no-such.fa");
    let out = tmp.path().join("genome.bin");

    for (list, says) in [
        ("21,chr99", "chr99"),
        ("21,,MT", "\"\""),
        ("21,chr21", "twice"),
    ] {
        let output = run_genome(&missing_input, &["--contigs", list], &out);
        assert_eq!(output.status.code(), Some(2), "{list}");
        let stderr = stderr_of(&output);
        assert!(stderr.contains("--contigs"), "{list}: {stderr}");
        assert!(stderr.contains(says), "{list}: {stderr}");
        assert!(
            !stderr.contains("no-such.fa"),
            "{list}: the list is checked before the FASTA is opened: {stderr}"
        );
        assert_nothing_written(&out);
    }
}

#[test]
fn genome_with_a_missing_fasta_exits_2() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("genome.bin");
    let output = run_genome(&tmp.path().join("no-such.fa"), &["--contigs", "MT"], &out);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr_of(&output).contains("no-such.fa"),
        "{}",
        stderr_of(&output)
    );
    assert_nothing_written(&out);
}

#[test]
fn genome_replaces_a_previous_genome_and_its_index() {
    let (_, chrm) = full_contigs();
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("chrM.fa");
    fs::write(&input, fasta(&[("chrM", chrm)])).unwrap();
    let out = tmp.path().join("genome.bin");
    fs::write(&out, b"previous genome").unwrap();
    fs::write(with_suffix(&out, ".idx"), b"previous index").unwrap();

    let output = run_genome(&input, &["--contigs", "MT"], &out);
    assert!(output.status.success(), "stderr: {}", stderr_of(&output));
    let reference = reference_genome(tmp.path(), &[("chrM", chrm)]);
    assert!(
        read_genome_files(&out) == reference,
        "the previous genome and index must be replaced by write_genome_binary(chrM)"
    );
    assert!(!with_suffix(&out, ".tmp").exists());
    assert!(!with_suffix(&out, ".idx.tmp").exists());
}

#[test]
fn genome_removes_the_old_index_before_the_new_genome_goes_in() {
    // Replacing a genome takes two renames, and vareffect takes the index as
    // proof its .bin is complete. The old index is removed first, so a run
    // that stops before the second rename leaves no index at all, which is
    // refused, never the old index beside a genome it does not describe. Here
    // the .bin rename fails (`out` is a directory), after the old index went.
    let (_, chrm) = full_contigs();
    let tmp = TempDir::new().unwrap();
    let input = tmp.path().join("chrM.fa");
    fs::write(&input, fasta(&[("chrM", chrm)])).unwrap();
    let out = tmp.path().join("genome.bin");
    fs::create_dir(&out).unwrap();
    fs::write(out.join("not-a-genome"), b"x").unwrap();
    fs::write(with_suffix(&out, ".idx"), b"previous index").unwrap();

    let output = run_genome(&input, &["--contigs", "MT"], &out);
    assert_eq!(
        output.status.code(),
        Some(2),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(
        stderr_of(&output).contains("renaming"),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(
        !with_suffix(&out, ".idx").exists(),
        "the old index must be gone before the new genome is renamed into place"
    );
    assert!(!with_suffix(&out, ".tmp").exists());
    assert!(!with_suffix(&out, ".idx.tmp").exists());
}
