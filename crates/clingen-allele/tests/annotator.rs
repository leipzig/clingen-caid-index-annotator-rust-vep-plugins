//! `Annotator` over a tiny index and the tiny hg38-window genome.
//!
//! The index holds two Registry keys on chr21:
//!
//! * `NC_000021.9:g.5030578C>T` is `CA749862939`
//! * `NC_000021.9:g.33000172dup` is `CA42`

use std::fs;
use std::path::{Path, PathBuf};

use clingen_allele::test_support::{CHR21_LEN, tiny_genome};
use clingen_allele::{Annotator, Error, IndexBuilder, Lookup, Manifest, Stats, Status};
use tempfile::{TempDir, tempdir};
use vareffect::fasta::write_genome_binary;

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
    genome_bin: PathBuf,
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

/// A genome of `N`s with the given contigs (name, length), under a fresh
/// temporary directory; returns the directory and the `.bin` path.
fn genome_with(contigs: &[(&str, usize)], build: &str) -> (TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let seqs: Vec<(&str, Vec<u8>)> = contigs
        .iter()
        .map(|&(name, len)| (name, vec![b'N'; len]))
        .collect();
    let refs: Vec<(&str, &[u8])> = seqs.iter().map(|(n, s)| (*n, s.as_slice())).collect();
    let bin = dir.path().join("genome.bin");
    write_genome_binary(&refs, build, &bin, &dir.path().join("genome.bin.idx")).unwrap();
    (dir, bin)
}

/// The message of the error `Annotator::open` gives for this genome.
fn genome_rejection(fx: &Fixture, genome_bin: &Path) -> String {
    match Annotator::open(fx.index_dir.path(), genome_bin) {
        Ok(_) => panic!("{} must be rejected as not GRCh38", genome_bin.display()),
        Err(e @ Error::GenomeNotGrch38 { .. }) => e.to_string(),
        Err(other) => panic!("expected Error::GenomeNotGrch38, got {other:?}"),
    }
}

#[test]
fn tiny_genome_is_accepted_as_grch38() {
    let fx = Fixture::new();
    let (_dir, genome) = tiny_genome();
    assert_eq!(genome.chrom_length("chr21"), Some(CHR21_LEN as u64));
    assert!(fx.open().is_ok());
}

#[test]
fn genome_with_grch37_chr21_is_rejected() {
    let fx = Fixture::new();
    // 48,129,895 is chr21's GRCh37 (hg19) length.
    let (_dir, bin) = genome_with(&[("chr21", 48_129_895)], "GRCh38");
    let message = genome_rejection(&fx, &bin);
    for part in ["not GRCh38", "chr21", "48129895", "46709983"] {
        assert!(message.contains(part), "{part:?} not in {message}");
    }
    assert!(message.contains(&bin.display().to_string()), "{message}");
}

#[test]
fn every_primary_contig_present_is_checked_whatever_the_naming() {
    let fx = Fixture::new();
    // chr21 is right; chrM has hg19's 16,571 bases rather than the rCRS's.
    let (_dir, bin) = genome_with(&[("chr21", CHR21_LEN), ("chrM", 16_571)], "GRCh38");
    let message = genome_rejection(&fx, &bin);
    for part in ["chrM", "16571", "16569"] {
        assert!(message.contains(part), "{part:?} not in {message}");
    }

    // Ensembl-style names are resolved the way lookups resolve them.
    let (_dir, bin) = genome_with(&[("21", 48_129_895)], "GRCh37");
    let message = genome_rejection(&fx, &bin);
    for part in ["chr21", "48129895", "46709983"] {
        assert!(message.contains(part), "{part:?} not in {message}");
    }
}

#[test]
fn genome_without_a_primary_contig_is_rejected() {
    let fx = Fixture::new();
    let (_dir, bin) = genome_with(&[("chr1_KI270706v1_random", 175_055)], "GRCh38");
    let message = genome_rejection(&fx, &bin);
    for part in ["not GRCh38", "none of the 25"] {
        assert!(message.contains(part), "{part:?} not in {message}");
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
    // The last base of chr21, then past the end of it and of chrM (16,569).
    let chr21_len = CHR21_LEN as u64;
    assert_eq!(annotator.base_at("21", chr21_len), Some(b'N'));
    assert_eq!(annotator.base_at("21", chr21_len + 1), None);
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

/// The `Lookup` every key-less `Unsupported` outcome carries.
fn unsupported() -> Lookup {
    Lookup {
        hgvs: None,
        caid: None,
        status: Status::Unsupported,
    }
}

#[test]
fn trimmed_insertion_matches_vcf_form() {
    let fx = Fixture::new();
    let annotator = fx.annotator();
    // VEP's `T>TA` at 33000166 is start 33000167, REF `-`, ALT `A`.
    let trimmed = annotator.lookup_trimmed("21", 33000167, b"-", b"A");
    let vcf = annotator.lookup("21", 33000166, b"T", b"TA");
    assert_eq!(trimmed, vcf);
    assert_eq!(
        trimmed,
        Lookup {
            hgvs: Some("NC_000021.9:g.33000172dup".to_string()),
            caid: Some("CA42".to_string()),
            status: Status::Found,
        }
    );
}

#[test]
fn trimmed_deletion_matches_vcf_form() {
    let fx = Fixture::new();
    let annotator = fx.annotator();
    // VEP's `TA>T` at 33000166 is start 33000167, REF `A`, ALT `-`.
    let trimmed = annotator.lookup_trimmed("21", 33000167, b"A", b"-");
    assert_eq!(trimmed.hgvs.as_deref(), Some("NC_000021.9:g.33000172del"));
    assert_eq!(trimmed.caid, None);
    assert_eq!(trimmed.status, Status::NotInIndex);
    assert_eq!(trimmed, annotator.lookup("21", 33000166, b"TA", b"T"));
}

#[test]
fn trimmed_empty_slice_same_as_dash() {
    let fx = Fixture::new();
    let annotator = fx.annotator();
    assert_eq!(
        annotator.lookup_trimmed("21", 33000167, b"", b"A"),
        annotator.lookup_trimmed("21", 33000167, b"-", b"A")
    );
    assert_eq!(
        annotator.lookup_trimmed("21", 33000167, b"A", b""),
        annotator.lookup_trimmed("21", 33000167, b"A", b"-")
    );
}

#[test]
fn trimmed_snv_passthrough() {
    let fx = Fixture::new();
    let annotator = fx.annotator();
    let lookup = annotator.lookup_trimmed("21", 5030578, b"C", b"T");
    assert_eq!(lookup.status, Status::Found);
    assert_eq!(lookup.caid.as_deref(), Some("CA749862939"));
    assert_eq!(lookup, annotator.lookup("21", 5030578, b"C", b"T"));
}

#[test]
fn trimmed_lowercase() {
    let fx = Fixture::new();
    let annotator = fx.annotator();
    let upper = annotator.lookup_trimmed("21", 33000167, b"-", b"A");
    let lower = annotator.lookup_trimmed("21", 33000167, b"-", b"a");
    assert_eq!(lower.hgvs, upper.hgvs);
    assert_eq!(lower, upper);
    // A lowercase deletion REF is anchored and uppercased the same way.
    assert_eq!(
        annotator.lookup_trimmed("21", 33000167, b"a", b"-").hgvs,
        annotator.lookup_trimmed("21", 33000167, b"A", b"-").hgvs
    );
}

#[test]
fn trimmed_without_anchor_is_unsupported() {
    let fx = Fixture::new();
    let annotator = fx.annotator();
    // Start 1 has no base before it to anchor on.
    assert_eq!(annotator.lookup_trimmed("21", 1, b"-", b"A"), unsupported());
    // Not a contig at all.
    assert_eq!(
        annotator.lookup_trimmed("foo", 33000167, b"-", b"A"),
        unsupported()
    );
    // Nothing to insert or delete.
    assert_eq!(
        annotator.lookup_trimmed("21", 33000167, b"-", b"-"),
        unsupported()
    );
    assert_eq!(annotator.stats().unsupported, 3);
    assert_eq!(
        annotator.stats(),
        Stats {
            unsupported: 3,
            ..Stats::default()
        }
    );
}

#[test]
fn trimmed_anchor_edge_cases_never_panic() {
    let fx = Fixture::new();
    let annotator = fx.annotator();
    let past_end = CHR21_LEN as u64 + 2;
    for (chrom, start1, ref_allele, alt_allele) in [
        ("21", 0, &b"-"[..], &b"A"[..]), // position 0
        ("21", 0, b"A", b"-"),           // position 0, deletion
        ("21", 1, b"A", b"-"),           // deletion at the first base
        ("21", u64::MAX, b"-", b"A"),    // far past the contig end
        ("21", past_end, b"-", b"A"),    // anchor just past the end
        ("1", 1000, b"-", b"A"),         // contig the genome file lacks
        ("21", 33000167, b"", b""),      // both empty, as zero-length
        ("21", 0, b"", b""),             // both empty at position 0
    ] {
        assert_eq!(
            annotator.lookup_trimmed(chrom, start1, ref_allele, alt_allele),
            unsupported(),
            "{chrom}:{start1} {ref_allele:?}>{alt_allele:?}"
        );
    }
    assert_eq!(annotator.stats().unsupported, 8);
}

#[test]
fn trimmed_counts_once_per_call() {
    let fx = Fixture::new();
    let annotator = fx.annotator();
    annotator.lookup_trimmed("21", 33000167, b"-", b"A"); // found
    annotator.lookup_trimmed("21", 5030578, b"C", b"T"); // found
    annotator.lookup_trimmed("21", 33000167, b"A", b"-"); // not in index
    let stats = annotator.stats();
    assert_eq!(
        stats,
        Stats {
            found: 2,
            not_in_index: 1,
            ..Stats::default()
        }
    );
    assert_eq!(
        stats.found + stats.not_in_index + stats.ref_mismatch + stats.unsupported,
        3
    );
}

#[test]
fn send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Annotator>();
}
