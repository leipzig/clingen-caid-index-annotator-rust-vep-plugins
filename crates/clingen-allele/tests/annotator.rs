//! `Annotator` over a tiny index and the tiny hg38-window genome.
//!
//! The index holds two Registry keys on chr21:
//!
//! * `NC_000021.9:g.5030578C>T` is `CA749862939`
//! * `NC_000021.9:g.33000172dup` is `CA42`

use std::fs;
use std::path::Path;

use clingen_allele::test_support::tiny_genome;
use clingen_allele::{Annotator, Error, IndexBuilder, Lookup, Manifest, Stats, Status};
use tempfile::{TempDir, tempdir};

fn manifest(assembly: &str, genome: Option<&str>) -> Manifest {
    Manifest {
        format_version: 1,
        assembly: assembly.to_string(),
        sources: vec!["test".to_string()],
        harvest_date: "2026-10-23".to_string(),
        entries: 0,
        vareffect_version: "0.6.0".to_string(),
        genome: genome.map(str::to_string),
    }
}

fn build_index(dir: &Path, manifest: Manifest) {
    let mut builder = IndexBuilder::new();
    builder.add("NC_000021.9:g.5030578C>T", 749862939);
    builder.add("NC_000021.9:g.33000172dup", 42);
    builder.finish(dir, manifest).unwrap();
}

/// An index directory plus the tiny genome's `.bin` path. Keep both
/// `TempDir`s alive while the `Annotator` is in use.
struct Fixture {
    _genome_dir: TempDir,
    index_dir: TempDir,
    genome_bin: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Self::with_manifest(manifest("GRCh38", None))
    }

    fn with_manifest(manifest: Manifest) -> Self {
        let (genome_dir, _reader) = tiny_genome();
        let genome_bin = genome_dir.path().join("genome.bin");
        let index_dir = tempdir().unwrap();
        build_index(index_dir.path(), manifest);
        Self {
            _genome_dir: genome_dir,
            index_dir,
            genome_bin,
        }
    }

    fn open(&self) -> Result<Annotator, Error> {
        Annotator::open(self.index_dir.path(), &self.genome_bin)
    }

    fn annotator(&self) -> Annotator {
        self.open().expect("open annotator")
    }
}

/// Rewrite one top-level field of `manifest.json` in `dir`.
fn set_manifest_field(dir: &Path, field: &str, value: serde_json::Value) {
    let path = dir.join("manifest.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    json[field] = value;
    fs::write(&path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
}

#[test]
fn found_snv() {
    let fx = Fixture::new();
    let annotator = fx.annotator();
    assert_eq!(
        annotator.lookup("21", 5030578, b"C", b"T"),
        Lookup {
            hgvs: Some("NC_000021.9:g.5030578C>T".to_string()),
            caid: Some("CA749862939".to_string()),
            status: Status::Found,
        }
    );
    assert_eq!(
        annotator.stats(),
        Stats {
            found: 1,
            ..Stats::default()
        }
    );
}

#[test]
fn found_via_any_representation() {
    let fx = Fixture::new();
    let annotator = fx.annotator();
    // The same duplication, written left-aligned and right-aligned.
    for (pos1, ref_allele, alt_allele) in
        [(33000166, &b"T"[..], &b"TA"[..]), (33000172, b"A", b"AA")]
    {
        let lookup = annotator.lookup("21", pos1, ref_allele, alt_allele);
        assert_eq!(lookup.caid.as_deref(), Some("CA42"), "{pos1}");
        assert_eq!(lookup.hgvs.as_deref(), Some("NC_000021.9:g.33000172dup"));
        assert_eq!(lookup.status, Status::Found);
    }
    assert_eq!(annotator.stats().found, 2);
}

#[test]
fn not_in_index() {
    let fx = Fixture::new();
    let annotator = fx.annotator();
    let lookup = annotator.lookup("21", 33000166, b"TA", b"T");
    assert_eq!(lookup.status, Status::NotInIndex);
    assert_eq!(lookup.hgvs.as_deref(), Some("NC_000021.9:g.33000172del"));
    assert_eq!(lookup.caid, None);
    assert_eq!(annotator.stats().not_in_index, 1);
}

#[test]
fn ref_mismatch_counts() {
    let fx = Fixture::new();
    let annotator = fx.annotator();
    // The genome has C at chr21:5030578, not G.
    let lookup = annotator.lookup("21", 5030578, b"G", b"T");
    assert_eq!(
        lookup,
        Lookup {
            hgvs: None,
            caid: None,
            status: Status::RefMismatch,
        }
    );
    assert_eq!(annotator.stats().ref_mismatch, 1);
}

#[test]
fn unsupported_variants_count() {
    let fx = Fixture::new();
    let annotator = fx.annotator();
    for (chrom, alt_allele) in [
        ("GL000220.1", &b"G"[..]), // not a primary contig
        ("21", b"<DEL>"),          // symbolic ALT
        ("21", b"A,G"),            // multi-allelic ALT
    ] {
        assert_eq!(
            annotator.lookup(chrom, 5030578, b"C", alt_allele),
            Lookup {
                hgvs: None,
                caid: None,
                status: Status::Unsupported,
            },
            "{chrom} {alt_allele:?}"
        );
    }
    assert_eq!(annotator.stats().unsupported, 3);
}

#[test]
fn missing_contig_is_unsupported() {
    // The tiny genome has no chr1, so reading it is a genome fault, which must
    // be counted rather than abort the run.
    let fx = Fixture::new();
    let annotator = fx.annotator();
    for _ in 0..2 {
        assert_eq!(
            annotator.lookup("1", 1000, b"A", b"G"),
            Lookup {
                hgvs: None,
                caid: None,
                status: Status::Unsupported,
            }
        );
    }
    assert_eq!(
        annotator.stats(),
        Stats {
            unsupported: 2,
            ..Stats::default()
        }
    );
}

#[test]
fn stats_tally_every_status() {
    let fx = Fixture::new();
    let annotator = fx.annotator();
    annotator.lookup("21", 5030578, b"C", b"T"); // found
    annotator.lookup("21", 33000166, b"T", b"TA"); // found
    annotator.lookup("21", 33000166, b"TA", b"T"); // not in index
    annotator.lookup("21", 5030578, b"A", b"T"); // ref mismatch
    annotator.lookup("MT", 3243, b"A", b"G"); // not in index (mito, key absent)
    annotator.lookup("21", 5030578, b"C", b"<DEL>"); // unsupported
    annotator.lookup("1", 1000, b"A", b"G"); // unsupported (contig missing)
    let stats = annotator.stats();
    assert_eq!(
        stats,
        Stats {
            found: 2,
            not_in_index: 2,
            ref_mismatch: 1,
            unsupported: 2,
        }
    );
    assert_eq!(
        stats.to_string(),
        "ClinGen: found=2 not_in_index=2 ref_mismatch=1 unsupported=2"
    );
}

#[test]
fn stats_display_when_empty() {
    assert_eq!(
        Stats::default().to_string(),
        "ClinGen: found=0 not_in_index=0 ref_mismatch=0 unsupported=0"
    );
}

#[test]
fn lookups_from_many_threads_are_all_counted() {
    let fx = Fixture::new();
    let annotator = fx.annotator();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                for _ in 0..25 {
                    assert_eq!(
                        annotator.lookup("21", 5030578, b"C", b"T").status,
                        Status::Found
                    );
                    assert_eq!(
                        annotator.lookup("1", 1000, b"A", b"G").status,
                        Status::Unsupported
                    );
                }
            });
        }
    });
    let stats = annotator.stats();
    assert_eq!((stats.found, stats.unsupported), (100, 100));
}

#[test]
fn manifest_wrong_assembly_rejected() {
    let fx = Fixture::with_manifest(manifest("GRCh37", None));
    match fx.open().err().expect("a GRCh37 index must be rejected") {
        Error::Manifest(message) => assert!(message.contains("GRCh37"), "{message}"),
        other => panic!("expected Error::Manifest, got {other:?}"),
    }
}

#[test]
fn manifest_wrong_format_version_rejected() {
    let fx = Fixture::new();
    set_manifest_field(fx.index_dir.path(), "format_version", 2.into());
    match fx.open().err().expect("format_version 2 must be rejected") {
        Error::Manifest(message) => assert!(message.contains('2'), "{message}"),
        other => panic!("expected Error::Manifest, got {other:?}"),
    }
}

#[test]
fn missing_genome_is_error() {
    let fx = Fixture::new();
    let missing = fx.index_dir.path().join("no-such-genome.bin");
    assert!(Annotator::open(fx.index_dir.path(), &missing).is_err());
}

#[test]
fn missing_index_is_error() {
    let fx = Fixture::new();
    let missing = fx.index_dir.path().join("no-such-index");
    assert!(Annotator::open(&missing, &fx.genome_bin).is_err());
}

#[test]
fn base_at_reads_reference() {
    let fx = Fixture::new();
    let annotator = fx.annotator();
    assert_eq!(annotator.base_at("chr21", 33000166), Some(b'T'));
    assert_eq!(annotator.base_at("21", 33000166), Some(b'T'));
    assert_eq!(annotator.base_at("chrM", 3243), Some(b'A'));
    assert_eq!(annotator.base_at("MT", 3243), Some(b'A'));
    // 1-based: the T is followed by a run of six As at 33000167-33000172.
    assert_eq!(annotator.base_at("21", 33000167), Some(b'A'));
    assert_eq!(annotator.base_at("21", 0), None);
    assert_eq!(annotator.base_at("foo", 1), None);
    assert_eq!(annotator.base_at("foo", 3243), None);
    // Supported contig that the genome file does not carry.
    assert_eq!(annotator.base_at("1", 1000), None);
    // Past the end of chr21 (33,000,400 bases) and of chrM (16,569).
    assert_eq!(annotator.base_at("21", 33_000_401), None);
    assert_eq!(annotator.base_at("21", u64::MAX), None);
    assert_eq!(annotator.base_at("chrM", 16_570), None);
}

#[test]
fn genome_path_from_manifest_resolves_against_index_dir() {
    // No manifest at all.
    let empty = tempdir().unwrap();
    assert_eq!(Annotator::genome_path_from_manifest(empty.path()), None);

    // Unparseable manifest.
    let garbage = tempdir().unwrap();
    fs::write(garbage.path().join("manifest.json"), "not json").unwrap();
    assert_eq!(Annotator::genome_path_from_manifest(garbage.path()), None);

    // Manifest without a genome.
    let none = tempdir().unwrap();
    build_index(none.path(), manifest("GRCh38", None));
    assert_eq!(Annotator::genome_path_from_manifest(none.path()), None);

    // Relative genome path: resolved against the index directory.
    let relative = tempdir().unwrap();
    build_index(
        relative.path(),
        manifest("GRCh38", Some("../hg38/genome.bin")),
    );
    assert_eq!(
        Annotator::genome_path_from_manifest(relative.path()),
        Some(relative.path().join("../hg38/genome.bin"))
    );

    // Absolute genome path: used as written.
    let absolute = tempdir().unwrap();
    build_index(
        absolute.path(),
        manifest("GRCh38", Some("/data/hg38/genome.bin")),
    );
    assert_eq!(
        Annotator::genome_path_from_manifest(absolute.path()),
        Some(Path::new("/data/hg38/genome.bin").to_path_buf())
    );
}

#[test]
fn send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Annotator>();
}
