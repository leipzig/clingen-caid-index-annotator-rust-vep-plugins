//! End-to-end tests of `clingen-index regions` over UCSC and BED fixtures
//! written here.
//!
//! The UCSC fixture is a handful of transcripts, in `ncbiRefSeq` form (with
//! its leading `bin` column) and `knownGene` form (without), chosen so each
//! region set, each merge distance and each clamp has a distinct, hand-checked
//! answer. Everything is 0-based half-open, as UCSC and the tool are.
//!
//! | transcript | contig | coding | exons (cds in brackets) |
//! |---|---|---|---|
//! | `NM_T1` (refseq) and `ENST_T1B` (gencode, a duplicate) | `chr21` | yes | `[1000,1200)` `[2000,2100)` `[4700,5000)`, cds `[1100,4800)` |
//! | `NM_GAPS` | `chr21` | yes | `[20000,20100)` `[20130,20200)` `[24200,24300)`, 30 and 4,000 bp apart |
//! | `ENST_T2` | `chr21` | no | `[10000,10300)` `[11500,12000)` |
//! | `ENST_T5` | `chr21` | no | gene `[200000,320000)` (120,000 bp), exons at its two ends |
//! | `NM_X` | `chrX` | yes | `[0,300)`, at the start of the contig |
//! | `ENST_MT` | `chrM` | yes | `[16000,16569)`, at the end of the contig |
//! | `NM_ALT`, `ENST_UN` | alt and unplaced | yes | skipped, 2 records |

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use flate2::Compression;
use flate2::write::GzEncoder;
use tempfile::TempDir;

fn cli() -> Command {
    cargo_bin_cmd!("clingen-index")
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

type Span = (u64, u64);

/// One UCSC row: `ncbiRefSeq` form with `bin` and the trailing score, name2,
/// status and frame columns, or `knownGene` form with its two trailing
/// (empty here) protein and alignment IDs.
fn row(bin: bool, name: &str, chrom: &str, tx: Span, cds: Span, exons: &[Span]) -> String {
    let starts: String = exons.iter().map(|(s, _)| format!("{s},")).collect();
    let ends: String = exons.iter().map(|(_, e)| format!("{e},")).collect();
    let body = format!(
        "{name}\t{chrom}\t+\t{}\t{}\t{}\t{}\t{}\t{starts}\t{ends}",
        tx.0,
        tx.1,
        cds.0,
        cds.1,
        exons.len()
    );
    if bin {
        format!("585\t{body}\t0\t{name}\tcmpl\tcmpl\t-1,\n")
    } else {
        format!("{body}\t\tuc001abc.1\n")
    }
}

fn refseq_text() -> String {
    [
        row(
            true,
            "NM_T1",
            "chr21",
            (1000, 5000),
            (1100, 4800),
            &[(1000, 1200), (2000, 2100), (4700, 5000)],
        ),
        row(
            true,
            "NM_ALT",
            "chr21_KI270872v1_alt",
            (100, 900),
            (100, 900),
            &[(100, 900)],
        ),
        row(true, "NM_X", "chrX", (0, 300), (0, 300), &[(0, 300)]),
        row(
            true,
            "NM_GAPS",
            "chr21",
            (20000, 24300),
            (20000, 24300),
            &[(20000, 20100), (20130, 20200), (24200, 24300)],
        ),
    ]
    .concat()
}

fn gencode_text() -> String {
    [
        row(
            false,
            "ENST_T1B",
            "chr21",
            (1000, 5000),
            (1100, 4800),
            &[(1000, 1200), (2000, 2100), (4700, 5000)],
        ),
        // Non-coding: UCSC sets cdsStart = cdsEnd = txEnd.
        row(
            false,
            "ENST_T2",
            "chr21",
            (10000, 12000),
            (12000, 12000),
            &[(10000, 10300), (11500, 12000)],
        ),
        row(
            false,
            "ENST_T5",
            "chr21",
            (200_000, 320_000),
            (320_000, 320_000),
            &[(200_000, 200_200), (319_800, 320_000)],
        ),
        row(
            false,
            "ENST_MT",
            "chrM",
            (16000, 16569),
            (16000, 16569),
            &[(16000, 16569)],
        ),
        row(
            false,
            "ENST_UN",
            "chrUn_KI270742v1",
            (0, 100),
            (0, 100),
            &[(0, 100)],
        ),
    ]
    .concat()
}

/// The two tables on disk under a temporary directory.
struct Fixture {
    tmp: TempDir,
    refseq: PathBuf,
    gencode: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let refseq = tmp.path().join("ncbiRefSeq.txt");
        let gencode = tmp.path().join("knownGene.txt");
        fs::write(&refseq, refseq_text()).unwrap();
        fs::write(&gencode, gencode_text()).unwrap();
        Fixture {
            tmp,
            refseq,
            gencode,
        }
    }

    /// A directory name inside the fixture that does not exist yet.
    fn out(&self, name: &str) -> PathBuf {
        self.tmp.path().join(name)
    }

    /// `regions` over the two tables with `extra` flags, into `out`.
    fn regions(&self, out: &Path, extra: &[&str]) -> Output {
        cli()
            .arg("regions")
            .arg("--refseq")
            .arg(&self.refseq)
            .arg("--gencode")
            .arg(&self.gencode)
            .args(extra)
            .arg("--out-dir")
            .arg(out)
            .output()
            .unwrap()
    }

    /// Run, require success, and give the output directory.
    fn ok(&self, name: &str, extra: &[&str]) -> PathBuf {
        let out = self.out(name);
        let output = self.regions(&out, extra);
        assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
        out
    }
}

fn gzip(text: &str) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(text.as_bytes()).unwrap();
    encoder.finish().unwrap()
}

/// File names in `dir`, sorted.
fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// The rows of every `windows_<chrom>_*.tsv` in `dir`, in file-name order.
fn window_rows(dir: &Path, chrom: &str) -> Vec<Vec<String>> {
    let prefix = format!("windows_{chrom}_");
    file_names(dir)
        .into_iter()
        .filter(|n| n.starts_with(&prefix) && n.ends_with(".tsv"))
        .flat_map(|n| {
            fs::read_to_string(dir.join(n))
                .unwrap()
                .lines()
                .map(|l| l.split('\t').map(str::to_owned).collect())
                .collect::<Vec<Vec<String>>>()
        })
        .collect()
}

/// `(core_begin, core_end)` of every window on `chrom`.
fn cores(dir: &Path, chrom: &str) -> Vec<Span> {
    window_rows(dir, chrom)
        .iter()
        .map(|r| (r[4].parse().unwrap(), r[5].parse().unwrap()))
        .collect()
}

/// `(begin, end)` of every window on `chrom`.
fn spans(dir: &Path, chrom: &str) -> Vec<Span> {
    window_rows(dir, chrom)
        .iter()
        .map(|r| (r[2].parse().unwrap(), r[3].parse().unwrap()))
        .collect()
}

#[test]
fn sets_select_the_right_intervals() {
    let fx = Fixture::new();
    let plain = ["--pad", "0", "--merge-gap", "0", "--flank", "0"];

    let run = |name: &str, set: &str| {
        let mut args = vec!["--set", set];
        args.extend(plain);
        fx.ok(name, &args)
    };

    // Coding transcripts' exons cut to the coding sequence: the UTR parts of
    // the first and last exon of NM_T1 drop out, and the duplicate gencode
    // copy of NM_T1 changes nothing.
    let cds = run("cds", "cds");
    assert_eq!(
        cores(&cds, "21"),
        [
            (1100, 1200),
            (2000, 2100),
            (4700, 4800),
            (20000, 20100),
            (20130, 20200),
            (24200, 24300),
        ]
    );
    assert_eq!(cores(&cds, "X"), [(0, 300)]);
    assert_eq!(cores(&cds, "MT"), [(16000, 16569)]);

    // Whole exons of coding transcripts only: no ENST_T2 or ENST_T5.
    let pc = run("pc", "pc_exons");
    assert_eq!(
        cores(&pc, "21"),
        [
            (1000, 1200),
            (2000, 2100),
            (4700, 5000),
            (20000, 20100),
            (20130, 20200),
            (24200, 24300),
        ]
    );
    assert_eq!(cores(&pc, "X"), [(0, 300)]);
    assert_eq!(cores(&pc, "MT"), [(16000, 16569)]);

    // Every exon, the non-coding ones too.
    let exons = run("exons", "exons");
    assert_eq!(
        cores(&exons, "21"),
        [
            (1000, 1200),
            (2000, 2100),
            (4700, 5000),
            (10000, 10300),
            (11500, 12000),
            (20000, 20100),
            (20130, 20200),
            (24200, 24300),
            (200_000, 200_200),
            (319_800, 320_000),
        ]
    );

    // txStart to txEnd of every transcript; ENST_T5 splits into three.
    let genes = run("genes", "genes");
    assert_eq!(
        cores(&genes, "21"),
        [
            (1000, 5000),
            (10000, 12000),
            (20000, 24300),
            (200_000, 240_000),
            (240_000, 280_000),
            (280_000, 320_000),
        ]
    );
    assert_eq!(cores(&genes, "X"), [(0, 300)]);
    assert_eq!(cores(&genes, "MT"), [(16000, 16569)]);

    // With no flank the query is the core.
    assert_eq!(spans(&genes, "21"), cores(&genes, "21"));
}

#[test]
fn padding_merging_and_splitting() {
    let fx = Fixture::new();
    let chr21_pc = |name: &str, extra: &[&str]| -> Vec<Span> {
        let mut args = vec!["--set", "pc_exons", "--chrom", "21"];
        args.extend(extra);
        cores(&fx.ok(name, &args), "21")
    };

    // The exons at 20000-20100 and 20130-20200 are 30 bp apart: they stay two
    // without padding, and become one once each side grows by 20.
    let unpadded = chr21_pc("p0", &["--pad", "0", "--merge-gap", "0"]);
    assert!(unpadded.contains(&(20000, 20100)) && unpadded.contains(&(20130, 20200)));
    let padded = chr21_pc("p20", &["--pad", "20", "--merge-gap", "0"]);
    assert!(padded.contains(&(19980, 20220)), "{padded:?}");
    assert!(!padded.contains(&(19980, 20120)), "{padded:?}");
    // Padding is symmetric and does not make the 4,000 bp gap vanish.
    assert!(padded.contains(&(24180, 24320)), "{padded:?}");

    // The last two of those exons are 4,000 bp apart: merged by a gap of
    // 4,000 (the distance is allowed to equal the gap) and not by 3,999.
    let gap4000 = chr21_pc("g4000", &["--pad", "0", "--merge-gap", "4000"]);
    assert!(gap4000.contains(&(20000, 24300)), "{gap4000:?}");
    let gap3999 = chr21_pc("g3999", &["--pad", "0", "--merge-gap", "3999"]);
    assert!(gap3999.contains(&(20000, 20200)), "{gap3999:?}");
    assert!(gap3999.contains(&(24200, 24300)), "{gap3999:?}");

    // The same at the distances of the plan: 5,000 merges them, 3,000 not.
    assert_eq!(
        chr21_pc("g5000", &["--pad", "0", "--merge-gap", "5000"]),
        [(1000, 5000), (20000, 24300)]
    );
    assert_eq!(
        chr21_pc("g3000", &["--pad", "0", "--merge-gap", "3000"]),
        [(1000, 5000), (20000, 20200), (24200, 24300)]
    );

    // A 120,000 bp gene at --max-window 50000 is 3 windows of 40,000, not
    // 50,000 + 50,000 + 20,000.
    let genes = fx.ok(
        "genes",
        &[
            "--set",
            "genes",
            "--pad",
            "0",
            "--merge-gap",
            "0",
            "--max-window",
            "50000",
            "--chrom",
            "21",
        ],
    );
    let big: Vec<Span> = cores(&genes, "21")
        .into_iter()
        .filter(|&(b, _)| b >= 200_000)
        .collect();
    assert_eq!(
        big,
        [(200_000, 240_000), (240_000, 280_000), (280_000, 320_000)]
    );
    // A merged interval at or under the limit is one window.
    let one = fx.ok(
        "one",
        &[
            "--set",
            "genes",
            "--pad",
            "0",
            "--merge-gap",
            "0",
            "--max-window",
            "120000",
        ],
    );
    assert!(cores(&one, "21").contains(&(200_000, 320_000)));
}

#[test]
fn flank_is_clamped_at_contig_ends() {
    let fx = Fixture::new();
    let out = fx.ok(
        "flank",
        &[
            "--set",
            "pc_exons",
            "--pad",
            "0",
            "--merge-gap",
            "0",
            "--flank",
            "200",
        ],
    );

    // Away from an end the query is the core plus 200 on each side.
    assert_eq!(spans(&out, "21")[0], (800, 1400));
    assert_eq!(cores(&out, "21")[0], (1000, 1200));
    // A core at position 0 queries from 0, not from -200.
    let x = window_rows(&out, "X");
    assert_eq!(x.len(), 1);
    assert_eq!(x[0][2..], ["0", "500", "0", "300"]);
    // chrM is 16,569 bases: the query ends there, not at 16,769.
    let mt = window_rows(&out, "MT");
    assert_eq!(mt[0][2..], ["15800", "16569", "16000", "16569"]);

    // Padding clamps at both ends too, before the flank is added.
    let padded = fx.ok(
        "padded",
        &[
            "--set",
            "pc_exons",
            "--pad",
            "20",
            "--merge-gap",
            "0",
            "--flank",
            "200",
        ],
    );
    assert_eq!(window_rows(&padded, "X")[0][2..], ["0", "520", "0", "320"]);
    assert_eq!(
        window_rows(&padded, "MT")[0][2..],
        ["15780", "16569", "15980", "16569"]
    );
}

#[test]
fn chrom_filter_and_batches() {
    let fx = Fixture::new();

    let out = fx.ok(
        "batch2",
        &[
            "--set",
            "pc_exons",
            "--pad",
            "0",
            "--merge-gap",
            "0",
            "--chrom",
            "21",
            "--batch",
            "2",
        ],
    );
    // Six windows, two to a file, only for chr21, numbered from 0.
    assert_eq!(
        file_names(&out),
        [
            "summary.tsv",
            "windows_21_0000.tsv",
            "windows_21_0001.tsv",
            "windows_21_0002.tsv",
        ]
    );
    for n in ["0000", "0001", "0002"] {
        let text = fs::read_to_string(out.join(format!("windows_21_{n}.tsv"))).unwrap();
        assert_eq!(text.lines().count(), 2, "windows_21_{n}.tsv");
    }
    // The files continue one another: all six windows, in order.
    assert_eq!(cores(&out, "21").len(), 6);
    assert!(cores(&out, "21").is_sorted());
    // `chr21` is the same filter.
    let by_ucsc = fx.ok(
        "chr21",
        &[
            "--set",
            "pc_exons",
            "--pad",
            "0",
            "--merge-gap",
            "0",
            "--chrom",
            "chr21",
            "--batch",
            "2",
        ],
    );
    assert_eq!(file_names(&by_ucsc), file_names(&out));

    // A batch that does not divide the windows leaves a short last file.
    let four = fx.ok(
        "batch4",
        &[
            "--set",
            "pc_exons",
            "--pad",
            "0",
            "--merge-gap",
            "0",
            "--chrom",
            "21",
            "--batch",
            "4",
        ],
    );
    assert_eq!(
        fs::read_to_string(four.join("windows_21_0000.tsv"))
            .unwrap()
            .lines()
            .count(),
        4
    );
    assert_eq!(
        fs::read_to_string(four.join("windows_21_0001.tsv"))
            .unwrap()
            .lines()
            .count(),
        2
    );

    // chrM: bare name `MT` in the file name and the first column, the
    // RefSeq accession in the second.
    let mt = fx.ok(
        "mt",
        &[
            "--set",
            "pc_exons",
            "--chrom",
            "chrM",
            "--pad",
            "0",
            "--merge-gap",
            "0",
        ],
    );
    assert_eq!(file_names(&mt), ["summary.tsv", "windows_MT_0000.tsv"]);
    let rows = window_rows(&mt, "MT");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][..2], ["MT", "NC_012920.1"]);

    // Every chromosome by default, with its own accession and bare name.
    let all = fx.ok(
        "all",
        &["--set", "pc_exons", "--pad", "0", "--merge-gap", "0"],
    );
    assert_eq!(
        file_names(&all),
        [
            "summary.tsv",
            "windows_21_0000.tsv",
            "windows_MT_0000.tsv",
            "windows_X_0000.tsv",
        ]
    );
    assert_eq!(window_rows(&all, "21")[0][..2], ["21", "NC_000021.9"]);
    assert_eq!(window_rows(&all, "X")[0][..2], ["X", "NC_000023.11"]);
    // `--chrom all` is the default spelled out.
    let spelled = fx.ok(
        "spelled",
        &[
            "--set",
            "pc_exons",
            "--pad",
            "0",
            "--merge-gap",
            "0",
            "--chrom",
            "all",
        ],
    );
    assert_eq!(file_names(&spelled), file_names(&all));
}

#[test]
fn bed_input_with_ensembl_names() {
    let tmp = TempDir::new().unwrap();
    let bed = tmp.path().join("regions.bed");
    fs::write(
        &bed,
        "track name=\"my regions\"\n\
         browser position chr21:1-1000\n\
         # a comment\n\
         21\t100\t200\n\
         chr21\t150\t300\tgeneA\t0\t+\n\
         NC_000021.9\t1000\t1100\n\
         X\t0\t10\n\
         MT\t10\t50\n\
         HSCHR6_MHC_COX\t1\t5\n\
         chrUn_KI270742v1\t0\t10\n\
         \n",
    )
    .unwrap();
    let out = tmp.path().join("out");

    let output = cli()
        .args(["regions", "--bed"])
        .arg(&bed)
        .args([
            "--pad",
            "0",
            "--merge-gap",
            "0",
            "--flank",
            "0",
            "--out-dir",
        ])
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    // Ensembl, UCSC and accession names land on the same chromosome, and the
    // overlapping first two intervals merge.
    assert_eq!(cores(&out, "21"), [(100, 300), (1000, 1100)]);
    assert_eq!(cores(&out, "X"), [(0, 10)]);
    assert_eq!(cores(&out, "MT"), [(10, 50)]);
    assert_eq!(window_rows(&out, "MT")[0][..2], ["MT", "NC_012920.1"]);
    // The two non-primary lines are counted, the track and comment lines not.
    assert_eq!(
        stderr_of(&output).trim(),
        "contigs=3 windows=4 core_bp=350 query_bp=350 skipped_records=2"
    );

    // BED takes the same --pad and --merge-gap: the 700 bp between the two
    // intervals on 21 closes at a gap of 700.
    let merged = tmp.path().join("merged");
    let output = cli()
        .args(["regions", "--bed"])
        .arg(&bed)
        .args([
            "--pad",
            "0",
            "--merge-gap",
            "700",
            "--chrom",
            "21",
            "--out-dir",
        ])
        .arg(&merged)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert_eq!(cores(&merged, "21"), [(100, 1100)]);
}

#[test]
fn non_primary_records_are_skipped_and_counted() {
    let fx = Fixture::new();
    let out = fx.out("skipped");
    let output = fx.regions(
        &out,
        &["--set", "pc_exons", "--pad", "0", "--merge-gap", "0"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    // The alt-contig record in the refseq table and the unplaced one in the
    // gencode table, whatever the set: none of them has a window.
    assert_eq!(
        stderr_of(&output).trim(),
        "contigs=3 windows=8 core_bp=1739 query_bp=4539 skipped_records=2"
    );
    assert!(
        file_names(&out)
            .iter()
            .all(|n| n == "summary.tsv" || ["_21_", "_X_", "_MT_"].iter().any(|c| n.contains(c)))
    );

    // Records on a primary contig that --chrom leaves out are not skipped
    // records: they are simply not wanted.
    let only = fx.out("only21");
    let output = fx.regions(
        &only,
        &[
            "--set",
            "pc_exons",
            "--pad",
            "0",
            "--merge-gap",
            "0",
            "--chrom",
            "21",
        ],
    );
    assert!(
        stderr_of(&output).trim().ends_with("skipped_records=2"),
        "{}",
        stderr_of(&output)
    );
}

#[test]
fn summary_tsv_reports_each_chromosome() {
    let fx = Fixture::new();
    let out = fx.ok(
        "summary",
        &["--set", "pc_exons", "--pad", "0", "--merge-gap", "0"],
    );
    // 21 has 9 padded intervals (NM_T1, its duplicate and NM_GAPS), 6 after
    // merging, 6 windows of 870 bp in all; the 200 bp flanks of neighbours
    // overlap, and are counted once per query.
    assert_eq!(
        fs::read_to_string(out.join("summary.tsv")).unwrap(),
        "chrom\tintervals\tmerged\twindows\tcore_bp\tquery_bp\n\
         21\t9\t6\t6\t870\t3270\n\
         X\t1\t1\t1\t300\t500\n\
         MT\t1\t1\t1\t569\t769\n"
    );
}

#[test]
fn gzipped_tables_read_like_plain_ones() {
    let fx = Fixture::new();
    let plain = fx.ok("plain", &["--set", "cds", "--pad", "20"]);

    let refseq_gz = fx.tmp.path().join("ncbiRefSeq.txt.gz");
    let gencode_gz = fx.tmp.path().join("knownGene.txt.gz");
    fs::write(&refseq_gz, gzip(&refseq_text())).unwrap();
    fs::write(&gencode_gz, gzip(&gencode_text())).unwrap();
    let out = fx.out("gz");
    let output = cli()
        .args(["regions", "--set", "cds", "--pad", "20", "--refseq"])
        .arg(&refseq_gz)
        .arg("--gencode")
        .arg(&gencode_gz)
        .arg("--out-dir")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    for name in file_names(&plain) {
        assert_eq!(
            fs::read(out.join(&name)).unwrap(),
            fs::read(plain.join(&name)).unwrap(),
            "{name}"
        );
    }
}

#[test]
fn malformed_table_rows_name_the_file_and_line() {
    let fx = Fixture::new();
    let mut bad = refseq_text();
    bad = bad.replacen("\t1000\t5000\t", "\t1000\tfive\t", 1);
    fs::write(&fx.refseq, &bad).unwrap();
    let output = fx.regions(&fx.out("o"), &["--set", "cds"]);
    assert_eq!(output.status.code(), Some(2));
    let err = stderr_of(&output);
    assert!(err.contains("ncbiRefSeq.txt:1:"), "{err}");
    assert!(err.contains("txEnd") && err.contains("\"five\""), "{err}");

    // An exon list that disagrees with exonCount.
    let fx = Fixture::new();
    let short = gencode_text().replacen("\t2\t10000,11500,\t", "\t3\t10000,11500,\t", 1);
    fs::write(&fx.gencode, short).unwrap();
    let output = fx.regions(&fx.out("o"), &["--set", "exons"]);
    assert_eq!(output.status.code(), Some(2));
    let err = stderr_of(&output);
    assert!(err.contains("knownGene.txt:2:"), "{err}");
    assert!(err.contains("exonCount"), "{err}");

    // A row with too few columns.
    let fx = Fixture::new();
    fs::write(&fx.gencode, "ENST1\tchr21\t+\t1\t2\n").unwrap();
    let output = fx.regions(&fx.out("o"), &["--set", "exons"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr_of(&output).contains("knownGene.txt:1:"));

    // A backwards exon.
    let fx = Fixture::new();
    let backwards = gencode_text().replacen("\t10300,12000,", "\t9000,12000,", 1);
    fs::write(&fx.gencode, backwards).unwrap();
    let output = fx.regions(&fx.out("o"), &["--set", "exons"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr_of(&output).contains("knownGene.txt:2:"));
}

#[test]
fn swapped_tables_are_refused() {
    // The tables the wrong way round shift every column by one, which would
    // otherwise read as "no primary contigs"; the first row gives it away.
    let fx = Fixture::new();
    let run = |refseq: &Path, gencode: &Path| {
        cli()
            .arg("regions")
            .arg("--refseq")
            .arg(refseq)
            .arg("--gencode")
            .arg(gencode)
            .args(["--set", "cds", "--out-dir"])
            .arg(fx.out("o"))
            .output()
            .unwrap()
    };

    // knownGene read as ncbiRefSeq: its first column is a name, not a bin.
    let output = run(&fx.gencode, &fx.gencode);
    assert_eq!(output.status.code(), Some(2));
    let err = stderr_of(&output);
    assert!(
        err.contains("knownGene.txt:1:") && err.contains("bin"),
        "{err}"
    );

    // ncbiRefSeq read as knownGene: the strand column holds a contig name.
    let output = run(&fx.refseq, &fx.refseq);
    assert_eq!(output.status.code(), Some(2));
    let err = stderr_of(&output);
    assert!(
        err.contains("ncbiRefSeq.txt:1:") && err.contains("strand"),
        "{err}"
    );
}

#[test]
fn intervals_past_the_contig_end_are_refused() {
    let tmp = TempDir::new().unwrap();
    let bed = tmp.path().join("regions.bed");
    fs::write(&bed, "MT\t10\t50\nMT\t16000\t17000\n").unwrap();
    let out = tmp.path().join("out");
    let output = cli()
        .args(["regions", "--bed"])
        .arg(&bed)
        .arg("--out-dir")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let err = stderr_of(&output);
    assert!(err.contains("regions.bed:2:"), "{err}");
    assert!(err.contains("16569"), "{err}");
    assert!(!out.exists() || file_names(&out).is_empty());

    // Empty, backwards and non-numeric intervals.
    for (text, expect) in [
        ("MT\t50\t50\n", "regions.bed:1:"),
        ("MT\t50\t10\n", "regions.bed:1:"),
        ("MT\t10\tx\n", "\"x\""),
        ("MT\t10\n", "regions.bed:1:"),
    ] {
        fs::write(&bed, text).unwrap();
        let output = cli()
            .args(["regions", "--bed"])
            .arg(&bed)
            .arg("--out-dir")
            .arg(tmp.path().join("out2"))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{text:?}");
        assert!(
            stderr_of(&output).contains(expect),
            "{text:?}: {}",
            stderr_of(&output)
        );
    }
}

#[test]
fn no_windows_is_an_error_and_writes_nothing() {
    let fx = Fixture::new();
    let out = fx.out("none");
    // Nothing of the fixture is on chr22.
    let output = fx.regions(&out, &["--set", "cds", "--chrom", "22"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr_of(&output).contains("no intervals"),
        "{}",
        stderr_of(&output)
    );
    assert!(!out.exists() || file_names(&out).is_empty());
}

#[test]
fn a_directory_with_windows_in_it_is_refused() {
    let fx = Fixture::new();
    let out = fx.ok("again", &["--set", "cds"]);
    let before: Vec<Vec<u8>> = file_names(&out)
        .iter()
        .map(|n| fs::read(out.join(n)).unwrap())
        .collect();
    // A second run into the same directory would leave the first run's files
    // among the second's; it is refused, and touches nothing.
    let output = fx.regions(&out, &["--set", "genes"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr_of(&output).contains("already holds"),
        "{}",
        stderr_of(&output)
    );
    let after: Vec<Vec<u8>> = file_names(&out)
        .iter()
        .map(|n| fs::read(out.join(n)).unwrap())
        .collect();
    assert_eq!(before, after);

    // An existing directory with other things in it is fine.
    let other = fx.out("other");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("notes.txt"), "x").unwrap();
    let output = fx.regions(&other, &["--set", "cds"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
}

#[test]
fn argument_errors_exit_2() {
    let fx = Fixture::new();
    let out = fx.out("o");
    let bed = fx.tmp.path().join("x.bed");
    fs::write(&bed, "21\t1\t2\n").unwrap();

    let expect_usage = |args: &[&str]| {
        let output = cli().arg("regions").args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        stderr_of(&output)
    };
    let out_s = out.to_str().unwrap();
    let refseq = fx.refseq.to_str().unwrap();
    let gencode = fx.gencode.to_str().unwrap();
    let bed_s = bed.to_str().unwrap();

    // Neither input, or only half of the tables.
    expect_usage(&["--out-dir", out_s]);
    expect_usage(&["--refseq", refseq, "--set", "cds", "--out-dir", out_s]);
    expect_usage(&["--gencode", gencode, "--set", "cds", "--out-dir", out_s]);
    expect_usage(&["--refseq", refseq, "--gencode", gencode, "--out-dir", out_s]);
    // BED and tables together; BED with a set.
    expect_usage(&[
        "--bed",
        bed_s,
        "--refseq",
        refseq,
        "--gencode",
        gencode,
        "--set",
        "cds",
        "--out-dir",
        out_s,
    ]);
    expect_usage(&["--bed", bed_s, "--set", "cds", "--out-dir", out_s]);
    // Unknown set; zero window and batch sizes.
    expect_usage(&[
        "--refseq",
        refseq,
        "--gencode",
        gencode,
        "--set",
        "utrs",
        "--out-dir",
        out_s,
    ]);
    expect_usage(&["--bed", bed_s, "--max-window", "0", "--out-dir", out_s]);
    expect_usage(&["--bed", bed_s, "--batch", "0", "--out-dir", out_s]);
    // No output directory.
    expect_usage(&["--bed", bed_s]);

    // A chromosome that is not a primary contig, however it is spelled.
    for chrom in ["22x", "chr21_KI270872v1_alt", "Un"] {
        let err = expect_usage_run(&bed, &out, chrom);
        assert!(err.contains("--chrom") && err.contains(chrom), "{err}");
    }
    assert!(!out.exists());
}

/// Run `regions --bed <bed> --chrom <chrom>`; expect exit 2; give stderr.
fn expect_usage_run(bed: &Path, out: &Path, chrom: &str) -> String {
    let output = cli()
        .args(["regions", "--bed"])
        .arg(bed)
        .args(["--chrom", chrom, "--out-dir"])
        .arg(out)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{chrom}");
    stderr_of(&output)
}

#[test]
fn only_one_table_can_come_from_standard_input() {
    let fx = Fixture::new();
    let output = cli()
        .arg("regions")
        .args([
            "--refseq",
            "-",
            "--gencode",
            "-",
            "--set",
            "cds",
            "--out-dir",
        ])
        .arg(fx.out("o"))
        .write_stdin(refseq_text())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr_of(&output).contains("standard input"));

    // One of them may be: the other table is read from its file.
    let out = fx.out("stdin");
    let output = cli()
        .arg("regions")
        .arg("--refseq")
        .arg("-")
        .arg("--gencode")
        .arg(&fx.gencode)
        .args(["--set", "cds", "--pad", "0", "--out-dir"])
        .arg(&out)
        .write_stdin(refseq_text())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert_eq!(cores(&out, "X"), [(0, 300)]);
}
