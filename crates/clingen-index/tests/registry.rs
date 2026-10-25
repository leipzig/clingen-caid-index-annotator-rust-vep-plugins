//! End-to-end tests of `clingen-index registry enumerate` and `registry lookup`
//! against a scripted stand-in for the Registry (`common/fake_registry.rs`).
//!
//! The JSON replies copy the shapes the live Registry returned on 2026-10-24:
//! an allele is `{"@id": "http://reg.genome.network/allele/CA…",
//! "genomicAlleles": [{"hgvs": [...], "referenceGenome": "GRCh38"}, …]}`, an
//! unregistered lookup result has `"@id": "_:CA"`, and a rejected VCF line is
//! `{"errorType": "…", …}`. Every run passes `--retry-base-ms 1`, so six
//! attempts take milliseconds instead of the production 15.5 minutes.
//!
//! `live_enumerate_finds_a_known_allele` asks the real Registry and runs only
//! with `CLINGEN_LIVE_REGISTRY=1 cargo test -- --ignored`.

mod common {
    pub mod fake_registry;
}

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use common::fake_registry::FakeRegistry;
use tempfile::TempDir;

/// The proxy variables ureq reads; a proxy set in the environment would
/// otherwise carry the requests for `127.0.0.1` away from the fake server.
const PROXY_VARS: [&str; 6] = [
    "ALL_PROXY",
    "all_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
];

/// The `fields` parameter of every `enumerate` request, `+` and `@` unencoded.
const ENUMERATE_FIELDS: &str = "fields=none+@id+genomicAlleles.hgvs+genomicAlleles.referenceGenome";

fn cli() -> Command {
    let mut cmd = cargo_bin_cmd!("clingen-index");
    for var in PROXY_VARS {
        cmd.env_remove(var);
    }
    cmd
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

/// `<path>.partial`, where a run writes before renaming.
fn partial(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap().to_owned();
    name.push(".partial");
    path.with_file_name(name)
}

/// One genomic allele entry: the reference genome and its HGVS list.
type Genomic<'a> = (&'a str, &'a [&'a str]);

/// A Registry allele with `@id` `id` and these genomic alleles.
fn allele(id: &str, genomic: &[Genomic]) -> String {
    let entries: Vec<String> = genomic
        .iter()
        .map(|(genome, hgvs)| {
            let list: Vec<String> = hgvs.iter().map(|h| format!("\"{h}\"")).collect();
            format!(
                "{{\"hgvs\":[{}],\"referenceGenome\":\"{genome}\"}}",
                list.join(",")
            )
        })
        .collect();
    format!(
        "{{\"@id\":\"{id}\",\"genomicAlleles\":[{}]}}",
        entries.join(",")
    )
}

/// A registered allele, `ca` being its CA ID.
fn registered(ca: &str, genomic: &[Genomic]) -> String {
    allele(&format!("http://reg.genome.network/allele/{ca}"), genomic)
}

/// A registered chr21 SNV whose only HGVS is `NC_000021.9:g.<pos>A>G`.
fn snv(ca: &str, pos: u64) -> String {
    registered(ca, &[("GRCh38", &[&format!("NC_000021.9:g.{pos}A>G")])])
}

/// A JSON array of `alleles`.
fn page(alleles: &[String]) -> String {
    format!("[{}]", alleles.join(","))
}

/// A scratch directory for one test's inputs and outputs.
struct Scratch {
    tmp: TempDir,
}

impl Scratch {
    fn new() -> Self {
        Scratch {
            tmp: TempDir::new().unwrap(),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.tmp.path().join(name)
    }

    /// A windows file as `regions` writes it: no header, columns `chrom
    /// accession begin end core_begin core_end`, the core equal to the span.
    fn windows(&self, name: &str, rows: &[(&str, &str, u64, u64)]) -> PathBuf {
        let text: String = rows
            .iter()
            .map(|(chrom, acc, b, e)| format!("{chrom}\t{acc}\t{b}\t{e}\t{b}\t{e}\n"))
            .collect();
        let path = self.path(name);
        fs::write(&path, text).unwrap();
        path
    }

    /// Run `registry enumerate` on `windows` against `server`.
    fn enumerate(&self, server: &FakeRegistry, windows: &[PathBuf], extra: &[&str]) -> Output {
        cli()
            .args(["registry", "enumerate", "--windows"])
            .args(windows)
            .arg("--out")
            .arg(self.path("enum.tsv"))
            .args(["--base-url", server.base_url(), "--retry-base-ms", "1"])
            .args(extra)
            .output()
            .unwrap()
    }

    /// Run `registry lookup` on a sites file holding `sites` against `server`.
    fn lookup(&self, server: &FakeRegistry, sites: &str, extra: &[&str]) -> Output {
        let sites_path = self.path("sites.tsv");
        fs::write(&sites_path, sites).unwrap();
        cli()
            .args(["registry", "lookup", "--sites"])
            .arg(&sites_path)
            .arg("--out")
            .arg(self.path("lookup.tsv"))
            .args(["--base-url", server.base_url(), "--retry-base-ms", "1"])
            .args(extra)
            .output()
            .unwrap()
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.path(name)).unwrap()
    }

    /// Assert that neither `name` nor `name.partial` exists.
    fn assert_absent(&self, name: &str) {
        let path = self.path(name);
        assert!(!path.exists(), "{} exists", path.display());
        assert!(
            !partial(&path).exists(),
            "{}.partial exists",
            path.display()
        );
    }
}

/// The request line of an `enumerate` request for `(begin, end]` on
/// `NC_000021.9`, asking for at most `limit` alleles.
fn enumerate_line(begin: u64, end: u64, limit: u64) -> String {
    format!(
        "GET /alleles?refseq=NC_000021.9&begin={begin}&end={end}&{ENUMERATE_FIELDS}&limit={limit} HTTP/1.1"
    )
}

/// The request lines `server` received, in order.
fn request_lines(server: &FakeRegistry) -> Vec<String> {
    server.requests().into_iter().map(|r| r.line).collect()
}

#[test]
fn enumerate_splits_a_full_window() {
    // `--page-size 3 --split-overlap 2`: the first answer is full, so it is
    // dropped and (5030570, 5030590] is asked for again as (5030570, 5030582]
    // and (5030578, 5030590], split at 5030580 and widened by 2 each way.
    // CA2 lies in the overlap and comes back twice; it is written once.
    let server = FakeRegistry::start(vec![
        (
            200,
            page(&[
                snv("CA1", 5030571),
                snv("CA2", 5030579),
                snv("CA3", 5030585),
            ]),
        ),
        (200, page(&[snv("CA1", 5030571), snv("CA2", 5030579)])),
        (200, page(&[snv("CA2", 5030579), snv("CA3", 5030585)])),
    ]);
    let s = Scratch::new();
    let windows = s.windows("w.tsv", &[("21", "NC_000021.9", 5030570, 5030590)]);
    let output = s.enumerate(
        &server,
        &[windows],
        &["--page-size", "3", "--split-overlap", "2"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    assert_eq!(
        s.read("enum.tsv"),
        "NC_000021.9:g.5030571A>G\tCA1\n\
         NC_000021.9:g.5030579A>G\tCA2\n\
         NC_000021.9:g.5030585A>G\tCA3\n"
    );
    assert!(!partial(&s.path("enum.tsv")).exists());
    let requests = server.requests();
    let lines: Vec<&str> = requests.iter().map(|r| r.line.as_str()).collect();
    assert_eq!(
        lines,
        [
            enumerate_line(5030570, 5030590, 3),
            enumerate_line(5030570, 5030582, 3),
            enumerate_line(5030578, 5030590, 3),
        ]
    );
    // The Registry's order is not stable from one request to the next, so
    // `skip` would page through a moving list.
    assert!(lines.iter().all(|line| !line.contains("skip")), "{lines:?}");
    let agent = format!("clingen-index/{}", env!("CARGO_PKG_VERSION"));
    assert!(
        requests
            .iter()
            .all(|r| r.header("user-agent") == Some(agent.as_str()))
    );
    // The dropped answer's alleles are not counted, nor CA2's second coming.
    assert!(
        stderr_of(&output).contains(
            "windows=1 requests=3 splits=1 alleles=3 reference=0 no_grch38_hgvs=0 written=3"
        ),
        "{}",
        stderr_of(&output)
    );
}

#[test]
fn enumerate_splits_a_full_half_again_first_half_first() {
    // `--page-size 2 --split-overlap 0`: (5030570, 5030590] is full and
    // splits into (5030570, 5030580] and (5030580, 5030590]; the first half is
    // full too and splits into (5030570, 5030575] and (5030575, 5030580],
    // both asked for before the second half.
    let full = page(&[snv("CA8", 5030571), snv("CA9", 5030589)]);
    let server = FakeRegistry::start(vec![
        (200, full.clone()),
        (200, full),
        (200, page(&[snv("CA1", 5030571)])),
        (200, page(&[snv("CA2", 5030578)])),
        (200, page(&[snv("CA3", 5030589)])),
    ]);
    let s = Scratch::new();
    let windows = s.windows("w.tsv", &[("21", "NC_000021.9", 5030570, 5030590)]);
    let output = s.enumerate(
        &server,
        &[windows],
        &["--page-size", "2", "--split-overlap", "0"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    assert_eq!(
        s.read("enum.tsv"),
        "NC_000021.9:g.5030571A>G\tCA1\n\
         NC_000021.9:g.5030578A>G\tCA2\n\
         NC_000021.9:g.5030589A>G\tCA3\n"
    );
    assert_eq!(
        request_lines(&server),
        [
            enumerate_line(5030570, 5030590, 2),
            enumerate_line(5030570, 5030580, 2),
            enumerate_line(5030570, 5030575, 2),
            enumerate_line(5030575, 5030580, 2),
            enumerate_line(5030580, 5030590, 2),
        ]
    );
    assert!(
        stderr_of(&output).contains(
            "windows=1 requests=5 splits=2 alleles=3 reference=0 no_grch38_hgvs=0 written=3"
        ),
        "{}",
        stderr_of(&output)
    );
}

#[test]
fn enumerate_fails_when_a_full_window_cannot_shrink() {
    // `--page-size 2 --split-overlap 10`. The first window is answered in
    // full. The second, (6000000, 6000022], is 22 = 2 * 10 + 2 bases, so it
    // still splits, into (6000000, 6000021] and (6000001, 6000022]; the first
    // half, 21 = 2 * 10 + 1 bases, is full again and could only be split into
    // halves no shorter than itself, so the run fails, leaving no output.
    let full = page(&[snv("CA8", 6000005), snv("CA9", 6000015)]);
    let server = FakeRegistry::start(vec![
        (200, page(&[snv("CA1", 5030571)])),
        (200, full.clone()),
        (200, full),
    ]);
    let s = Scratch::new();
    let windows = s.windows(
        "w.tsv",
        &[
            ("21", "NC_000021.9", 5030570, 5030590),
            ("21", "NC_000021.9", 6000000, 6000022),
        ],
    );
    let output = s.enumerate(
        &server,
        std::slice::from_ref(&windows),
        &["--page-size", "2", "--split-overlap", "10"],
    );
    assert_eq!(output.status.code(), Some(2), "{}", stderr_of(&output));

    s.assert_absent("enum.tsv");
    assert_eq!(
        request_lines(&server),
        [
            enumerate_line(5030570, 5030590, 2),
            enumerate_line(6000000, 6000022, 2),
            enumerate_line(6000000, 6000021, 2),
        ]
    );
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains(&format!("{}:2", windows.display())),
        "{stderr}"
    );
    assert!(
        stderr.contains("window NC_000021.9 6000000-6000022"),
        "{stderr}"
    );
    assert!(stderr.contains("6000000-6000021"), "{stderr}");
    assert!(stderr.contains("--page-size 2"), "{stderr}");
}

#[test]
fn enumerate_keeps_only_the_windows_grch38_hgvs() {
    let alleles = [
        // The GRCh38 accession first, the GenBank one and GRCh37 after it.
        registered(
            "CA749862939",
            &[
                (
                    "GRCh38",
                    &["NC_000021.9:g.5030578C>T", "CM000683.2:g.5030578C>T"],
                ),
                ("GRCh37", &["NC_000021.8:g.6403151C>T"]),
            ],
        ),
        // A reference allele: dropped and counted.
        registered(
            "CA2377472496",
            &[
                (
                    "GRCh38",
                    &["NC_000021.9:g.5030577G=", "CM000683.2:g.5030577G="],
                ),
                ("GRCh37", &["NC_000021.8:g.6403150G="]),
            ],
        ),
        // No NC_000021.9 HGVS at all: counted.
        registered(
            "CA900000001",
            &[
                ("GRCh38", &["CM000683.2:g.5030585A>G"]),
                ("GRCh37", &["NC_000021.8:g.6403158A>G"]),
            ],
        ),
        // The GRCh38 accession second; the CA ID keeps its leading zero.
        registered(
            "CA068622",
            &[(
                "GRCh38",
                &["CM000683.2:g.5030580del", "NC_000021.9:g.5030580del"],
            )],
        ),
    ];
    let server = FakeRegistry::start(vec![(200, page(&alleles))]);
    let s = Scratch::new();
    let windows = s.windows("w.tsv", &[("21", "NC_000021.9", 5030570, 5030590)]);
    let output = s.enumerate(&server, &[windows], &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    assert_eq!(
        s.read("enum.tsv"),
        "NC_000021.9:g.5030578C>T\tCA749862939\nNC_000021.9:g.5030580del\tCA068622\n"
    );
    assert!(
        stderr_of(&output).contains(
            "windows=1 requests=1 splits=0 alleles=4 reference=1 no_grch38_hgvs=1 written=2"
        ),
        "{}",
        stderr_of(&output)
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].line.contains("&limit=200000 HTTP/1.1"),
        "{}",
        requests[0].line
    );
}

#[test]
fn enumerate_keeps_reference_alleles_when_asked() {
    // The reference allele comes first, as the live Registry returns them
    // mixed in with the rest. Two runs over the same answer: by default it is
    // dropped and counted; with `--keep-reference` it is written in its place.
    let answer = || {
        page(&[
            registered(
                "CA2375273242",
                &[
                    (
                        "GRCh38",
                        &["NC_000021.9:g.5030577G=", "CM000683.2:g.5030577G="],
                    ),
                    ("GRCh37", &["NC_000021.8:g.6403150G="]),
                ],
            ),
            snv("CA1", 5030578),
        ])
    };
    let s = Scratch::new();
    let windows = s.windows("w.tsv", &[("21", "NC_000021.9", 5030570, 5030590)]);

    let server = FakeRegistry::start(vec![(200, answer())]);
    let output = s.enumerate(&server, std::slice::from_ref(&windows), &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert_eq!(s.read("enum.tsv"), "NC_000021.9:g.5030578A>G\tCA1\n");
    assert!(
        stderr_of(&output).contains(
            "windows=1 requests=1 splits=0 alleles=2 reference=1 no_grch38_hgvs=0 written=1"
        ),
        "{}",
        stderr_of(&output)
    );

    let server = FakeRegistry::start(vec![(200, answer())]);
    let output = s.enumerate(&server, &[windows], &["--keep-reference"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert_eq!(
        s.read("enum.tsv"),
        "NC_000021.9:g.5030577G=\tCA2375273242\nNC_000021.9:g.5030578A>G\tCA1\n"
    );
    // `reference=` counts them either way; only `written=` differs.
    assert!(
        stderr_of(&output).contains(
            "windows=1 requests=1 splits=0 alleles=2 reference=1 no_grch38_hgvs=0 written=2"
        ),
        "{}",
        stderr_of(&output)
    );
}

#[test]
fn enumerate_keep_reference_needs_a_ca_id_for_a_reference_allele() {
    // A reference allele is written like any other row, so with
    // `--keep-reference` one without a CA ID stops the run, naming its HGVS.
    // Without the flag it is dropped before its `@id` is looked at.
    let answer = || page(&[allele("_:CA", &[("GRCh38", &["NC_000021.9:g.5030577G="])])]);
    let server = FakeRegistry::start(vec![(200, answer())]);
    let s = Scratch::new();
    let windows = s.windows("w.tsv", &[("21", "NC_000021.9", 5030570, 5030590)]);
    let output = s.enumerate(&server, &[windows], &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert_eq!(s.read("enum.tsv"), "");

    let server = FakeRegistry::start(vec![(200, answer())]);
    let s = Scratch::new();
    let windows = s.windows("w.tsv", &[("21", "NC_000021.9", 5030570, 5030590)]);
    let output = s.enumerate(&server, &[windows], &["--keep-reference"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr_of(&output));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("allele NC_000021.9:g.5030577G= has no CA ID"),
        "{stderr}"
    );
    s.assert_absent("enum.tsv");
}

#[test]
#[allow(non_snake_case)]
fn enumerate_keeps_m_hgvs_on_chrM() {
    // Both entries carry the same NC_012920.1 HGVS: one row, not two.
    let alleles = [
        registered(
            "CA337095722",
            &[
                ("GRCh38", &["NC_012920.1:m.73A>G"]),
                ("GRCh37", &["NC_012920.1:m.73A>G", "NC_001807.4:m.73A>G"]),
            ],
        ),
        registered("CA337095723", &[("GRCh38", &["NC_012920.1:m.74T="])]),
    ];
    let server = FakeRegistry::start(vec![(200, page(&alleles))]);
    let s = Scratch::new();
    let windows = s.windows("w.tsv", &[("MT", "NC_012920.1", 60, 80)]);
    let output = s.enumerate(&server, &[windows], &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    assert_eq!(s.read("enum.tsv"), "NC_012920.1:m.73A>G\tCA337095722\n");
    assert!(
        server.requests()[0]
            .line
            .starts_with("GET /alleles?refseq=NC_012920.1&begin=60&end=80&"),
    );
    assert!(
        stderr_of(&output).contains(
            "windows=1 requests=1 splits=0 alleles=2 reference=1 no_grch38_hgvs=0 written=1"
        ),
        "{}",
        stderr_of(&output)
    );
}

#[test]
fn enumerate_retries_429_and_503_then_succeeds() {
    let server = FakeRegistry::start(vec![
        (429, "slow down".to_owned()),
        (503, "busy".to_owned()),
        (200, page(&[snv("CA1", 5030571)])),
        (200, page(&[snv("CA2", 6000001)])),
    ]);
    let s = Scratch::new();
    let first = s.windows("a.tsv", &[("21", "NC_000021.9", 5030570, 5030590)]);
    let second = s.windows("b.tsv", &[("21", "NC_000021.9", 6000000, 6000100)]);
    let output = s.enumerate(&server, &[first, second], &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    assert_eq!(
        s.read("enum.tsv"),
        "NC_000021.9:g.5030571A>G\tCA1\nNC_000021.9:g.6000001A>G\tCA2\n"
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 4);
    // The two retries repeat the first window's request exactly.
    assert!(requests[..3].iter().all(|r| r.line == requests[0].line));
    assert!(requests[3].line.contains("begin=6000000&end=6000100&"));
    let stderr = stderr_of(&output);
    assert!(stderr.contains("HTTP 429: slow down"), "{stderr}");
    assert!(stderr.contains("HTTP 503: busy"), "{stderr}");
    assert!(
        stderr.contains(
            "windows=2 requests=2 splits=0 alleles=2 reference=0 no_grch38_hgvs=0 written=2"
        ),
        "{stderr}"
    );
}

#[test]
fn enumerate_fails_on_400_and_writes_nothing() {
    // The first window's row reaches the partial file before the second
    // window fails; the partial file must go too.
    let server = FakeRegistry::start(vec![
        (200, page(&[snv("CA1", 5030571)])),
        (
            400,
            "{\"errorType\":\"InvalidRequest\",\"description\":\"bad end\"}".to_owned(),
        ),
    ]);
    let s = Scratch::new();
    let windows = s.windows(
        "w.tsv",
        &[
            ("21", "NC_000021.9", 5030570, 5030590),
            ("21", "NC_000021.9", 6000000, 6000100),
        ],
    );
    let output = s.enumerate(&server, std::slice::from_ref(&windows), &[]);
    assert_eq!(output.status.code(), Some(2));

    s.assert_absent("enum.tsv");
    // A 400 is not retried.
    assert_eq!(server.requests().len(), 2);
    let stderr = stderr_of(&output);
    assert!(stderr.contains("HTTP 400"), "{stderr}");
    assert!(stderr.contains("InvalidRequest"), "{stderr}");
    assert!(
        stderr.contains(&format!("{}:2", windows.display())),
        "{stderr}"
    );
}

#[test]
fn enumerate_fails_on_truncated_json_after_six_attempts() {
    let truncated = format!(
        "[{}",
        &snv("CA1", 5030571)[..40] // cut inside the first allele
    );
    let server = FakeRegistry::start(vec![(200, truncated); 6]);
    let s = Scratch::new();
    let windows = s.windows("w.tsv", &[("21", "NC_000021.9", 5030570, 5030590)]);
    let output = s.enumerate(&server, &[windows], &[]);
    assert_eq!(output.status.code(), Some(2));

    s.assert_absent("enum.tsv");
    assert_eq!(server.requests().len(), 6);
    let stderr = stderr_of(&output);
    assert!(stderr.contains("6 attempts"), "{stderr}");
}

#[test]
fn enumerate_refuses_a_malformed_window_before_any_request() {
    let server = FakeRegistry::start(Vec::new());
    let s = Scratch::new();
    let windows = s.path("w.tsv");
    fs::write(
        &windows,
        "21\tNC_000021.9\t5030570\t5030590\t5030570\t5030590\n21\tNC_000021.9\t5030590\tx\t1\t2\n",
    )
    .unwrap();
    let output = s.enumerate(&server, std::slice::from_ref(&windows), &[]);
    assert_eq!(output.status.code(), Some(2));

    s.assert_absent("enum.tsv");
    assert!(server.requests().is_empty());
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains(&format!("{}:2", windows.display())),
        "{stderr}"
    );
    assert!(stderr.contains("\"x\""), "{stderr}");
}

/// The POST request line `registry lookup` sends.
const LOOKUP_LINE: &str = "POST /alleles?file=vcf&fields=none+@id+genomicAlleles.referenceGenome+genomicAlleles.hgvs HTTP/1.1";

/// The VCF header line naming the columns.
const VCF_COLUMNS: &str = "#CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO";

#[test]
fn lookup_writes_the_harvester_columns() {
    let sites = "21\t5030578\tC\tT\n21\t5030600\tA\tG\nX\t1000\tA\tT\n21\t5030610\tG\tGA\n";
    // `--batch 2`: two requests, each declaring only the contigs it holds.
    let first = page(&[
        registered(
            "CA749862939",
            &[
                (
                    "GRCh38",
                    &["NC_000021.9:g.5030578C>T", "CM000683.2:g.5030578C>T"],
                ),
                ("GRCh37", &["NC_000021.8:g.6403151C>T"]),
            ],
        ),
        allele(
            "_:CA",
            &[
                (
                    "GRCh38",
                    &["NC_000021.9:g.5030600A>G", "CM000683.2:g.5030600A>G"],
                ),
                ("GRCh37", &["NC_000021.8:g.6403173A>G"]),
            ],
        ),
    ]);
    let second = format!(
        "[{{\"errorType\":\"IncorrectReferenceAllele\",\"description\":\"no\",\"inputLine\":\"X\\t1000\"}},{}]",
        registered(
            "CA068622",
            &[(
                "GRCh38",
                &["CM000683.2:g.5030611dup", "NC_000021.9:g.5030611dup"],
            )],
        )
    );
    let server = FakeRegistry::start(vec![(200, first), (200, second)]);
    let s = Scratch::new();
    let output = s.lookup(&server, sites, &["--batch", "2"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));

    assert_eq!(
        s.read("lookup.tsv"),
        "21\t5030578\tC\tT\tCA749862939\tNC_000021.9:g.5030578C>T\tNC_000021.8:g.6403151C>T\n\
         21\t5030600\tA\tG\t\tNC_000021.9:g.5030600A>G\tNC_000021.8:g.6403173A>G\n\
         X\t1000\tA\tT\tERROR:IncorrectReferenceAllele\t\t\n\
         21\t5030610\tG\tGA\tCA068622\tNC_000021.9:g.5030611dup\t\n"
    );
    assert!(!partial(&s.path("lookup.tsv")).exists());

    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|r| r.line == LOOKUP_LINE));
    assert!(
        requests
            .iter()
            .all(|r| { r.header("content-type") == Some("application/x-www-form-urlencoded") })
    );
    assert_eq!(
        requests[0].body,
        format!(
            "##fileformat=VCFv4.2\n\
             ##contig=<ID=21,length=46709983,assembly=GRCh38>\n\
             {VCF_COLUMNS}\n\
             21\t5030578\t.\tC\tT\t.\t.\t.\n\
             21\t5030600\t.\tA\tG\t.\t.\t.\n"
        )
    );
    assert_eq!(
        requests[1].body,
        format!(
            "##fileformat=VCFv4.2\n\
             ##contig=<ID=21,length=46709983,assembly=GRCh38>\n\
             ##contig=<ID=X,length=156040895,assembly=GRCh38>\n\
             {VCF_COLUMNS}\n\
             X\t1000\t.\tA\tT\t.\t.\t.\n\
             21\t5030610\t.\tG\tGA\t.\t.\t.\n"
        )
    );
    assert!(
        stderr_of(&output)
            .contains("sites=4 requests=2 registered=2 unregistered=1 error=1 written=4"),
        "{}",
        stderr_of(&output)
    );
}

#[test]
fn lookup_result_count_mismatch_fails() {
    let server = FakeRegistry::start(vec![(
        200,
        page(&[registered(
            "CA749862939",
            &[("GRCh38", &["NC_000021.9:g.5030578C>T"])],
        )]),
    )]);
    let s = Scratch::new();
    let output = s.lookup(&server, "21\t5030578\tC\tT\n21\t5030600\tA\tG\n", &[]);
    assert_eq!(output.status.code(), Some(2));

    s.assert_absent("lookup.tsv");
    // A complete answer of the wrong length is not a transient failure.
    assert_eq!(server.requests().len(), 1);
    let stderr = stderr_of(&output);
    assert!(stderr.contains("expected 2 results, got 1"), "{stderr}");
}

#[test]
fn lookup_refuses_a_site_on_an_unknown_contig() {
    let server = FakeRegistry::start(Vec::new());
    let s = Scratch::new();
    let output = s.lookup(&server, "21\t5030578\tC\tT\nchr21\t5030600\tA\tG\n", &[]);
    assert_eq!(output.status.code(), Some(2));

    s.assert_absent("lookup.tsv");
    assert!(server.requests().is_empty());
    let stderr = stderr_of(&output);
    assert!(stderr.contains("sites.tsv:2"), "{stderr}");
    assert!(stderr.contains("\"chr21\""), "{stderr}");
}

#[test]
#[ignore = "asks the live Registry; run with CLINGEN_LIVE_REGISTRY=1 cargo test -- --ignored"]
fn live_enumerate_finds_a_known_allele() {
    if std::env::var("CLINGEN_LIVE_REGISTRY").as_deref() != Ok("1") {
        eprintln!("skipped: set CLINGEN_LIVE_REGISTRY=1 to ask the live Registry");
        return;
    }
    let s = Scratch::new();
    let windows = s.windows("w.tsv", &[("21", "NC_000021.9", 5030570, 5030590)]);
    // The environment's proxy, if any, is kept: it may be the way out.
    let output = cargo_bin_cmd!("clingen-index")
        .args(["registry", "enumerate", "--windows"])
        .arg(&windows)
        .arg("--out")
        .arg(s.path("enum.tsv"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    let rows = s.read("enum.tsv");
    assert!(
        rows.lines()
            .any(|line| line == "NC_000021.9:g.5030578C>T\tCA749862939"),
        "{rows}"
    );
}
