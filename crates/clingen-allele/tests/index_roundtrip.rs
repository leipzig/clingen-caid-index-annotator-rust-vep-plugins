//! `IndexBuilder` -> `caid.fst` + `manifest.json` -> `CaIndex` round trips.

use std::fs;
use std::path::Path;

use clingen_allele::{CaIndex, Error, IndexBuilder, Manifest, format_caid, parse_caid};
use tempfile::tempdir;

fn manifest() -> Manifest {
    Manifest {
        format_version: 1,
        assembly: "GRCh38".to_string(),
        sources: vec!["gnomAD v4.1 chr21 sites".to_string()],
        harvest_date: "2026-10-23".to_string(),
        entries: 0,
        vareffect_version: "0.6.0".to_string(),
        genome: None,
    }
}

/// Four Registry keys with fixed IDs, deliberately not in sorted order.
const KEYS: [(&str, u64); 4] = [
    ("NC_012920.1:m.3243A>G", 120560),
    ("NC_000021.9:g.33000172dup", 9001),
    ("NC_000021.9:g.5030578C>T", 749862939),
    ("NC_000021.9:g.33000172del", 9002),
];

fn build(dir: &Path, keys: &[(&str, u64)]) -> Result<u64, Error> {
    let mut builder = IndexBuilder::new();
    for (hgvs, caid) in keys {
        builder.add(hgvs, *caid);
    }
    builder.finish(dir, manifest())
}

#[test]
fn roundtrip() {
    let dir = tempdir().unwrap();
    assert_eq!(build(dir.path(), &KEYS).unwrap(), 4);

    let index = CaIndex::open(dir.path()).unwrap();
    for (hgvs, caid) in KEYS {
        assert_eq!(index.get(hgvs), Some(caid), "{hgvs}");
    }
    assert_eq!(index.get("NC_000021.9:g.1A>T"), None);
}

#[test]
fn get_is_exact_match_not_prefix() {
    let dir = tempdir().unwrap();
    build(dir.path(), &KEYS).unwrap();
    let index = CaIndex::open(dir.path()).unwrap();

    assert_eq!(index.get("NC_000021.9:g.5030578C>"), None);
    assert_eq!(index.get("NC_000021.9:g.5030578C>TT"), None);
    assert_eq!(index.get(""), None);
}

#[test]
fn exact_duplicate_ok() {
    let dir = tempdir().unwrap();
    let key = "NC_000021.9:g.5030578C>T";
    let entries = build(dir.path(), &[(key, 749862939), (key, 749862939)]).unwrap();
    assert_eq!(entries, 1);

    let index = CaIndex::open(dir.path()).unwrap();
    assert_eq!(index.get(key), Some(749862939));
    assert_eq!(index.manifest().entries, 1);
}

#[test]
fn conflicting_duplicate_fails() {
    let dir = tempdir().unwrap();
    let key = "NC_000021.9:g.5030578C>T";
    let err = build(dir.path(), &[(key, 1), (key, 2)]).unwrap_err();
    match err {
        Error::DuplicateConflict {
            hgvs,
            first,
            second,
        } => {
            assert_eq!(hgvs, key);
            assert_eq!((first, second), (1, 2));
        }
        other => panic!("expected DuplicateConflict, got {other:?}"),
    }
}

#[test]
fn conflict_is_reported_in_ascending_id_order_whatever_the_insertion_order() {
    let dir = tempdir().unwrap();
    let key = "NC_000021.9:g.5030578C>T";
    let err = build(dir.path(), &[(key, 2), (key, 1)]).unwrap_err();
    assert!(
        matches!(
            &err,
            Error::DuplicateConflict {
                first: 1,
                second: 2,
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn conflicting_duplicate_writes_nothing() {
    let dir = tempdir().unwrap();
    let out = dir.path().join("index");
    let key = "NC_000021.9:g.5030578C>T";
    build(&out, &[(key, 1), (key, 2)]).unwrap_err();
    assert!(!out.exists(), "a failed build must not leave files behind");
}

#[test]
fn caid_parse_format() {
    assert_eq!(parse_caid("CA120560"), Some(120560));
    assert_eq!(parse_caid("CA749862939"), Some(749862939));
    assert_eq!(parse_caid("CA0"), Some(0));
    assert_eq!(parse_caid("CA18446744073709551615"), Some(u64::MAX));

    assert_eq!(parse_caid("_:CA"), None);
    assert_eq!(parse_caid(""), None);
    assert_eq!(parse_caid("ERROR:IncorrectReferenceAllele"), None);
    assert_eq!(parse_caid("CA"), None);
    assert_eq!(parse_caid("CA12x"), None);
    assert_eq!(parse_caid("ca1"), None);
    assert_eq!(parse_caid("CA+1"), None);
    assert_eq!(parse_caid("CA-1"), None);
    assert_eq!(parse_caid(" CA1"), None);
    assert_eq!(parse_caid("CA1 "), None);
    assert_eq!(parse_caid("CA18446744073709551616"), None);

    assert_eq!(format_caid(120560), "CA120560");
    assert_eq!(parse_caid(&format_caid(749862939)), Some(749862939));
}

#[test]
fn manifest_persisted() {
    let dir = tempdir().unwrap();
    build(dir.path(), &KEYS).unwrap();

    let index = CaIndex::open(dir.path()).unwrap();
    let m = index.manifest();
    assert_eq!(m.assembly, "GRCh38");
    assert_eq!(m.sources, ["gnomAD v4.1 chr21 sites"]);
    assert_eq!(m.harvest_date, "2026-10-23");
    assert_eq!(m.vareffect_version, "0.6.0");
    assert_eq!(m.genome, None);
}

#[test]
fn finish_sets_entries_and_format_version() {
    let dir = tempdir().unwrap();
    let mut builder = IndexBuilder::new();
    let key = "NC_000021.9:g.5030578C>T";
    for (hgvs, caid) in KEYS {
        builder.add(hgvs, caid);
    }
    builder.add(key, 749862939); // exact duplicate of an entry in KEYS

    // Placeholders the caller passes for the two fields `finish` owns.
    let placeholder = Manifest {
        format_version: 99,
        entries: 12345,
        ..manifest()
    };
    assert_eq!(builder.finish(dir.path(), placeholder).unwrap(), 4);

    let index = CaIndex::open(dir.path()).unwrap();
    assert_eq!(index.manifest().entries, 4);
    assert_eq!(index.manifest().format_version, 1);
}

#[test]
fn genome_field_roundtrips_and_defaults_to_none() {
    let dir = tempdir().unwrap();
    let mut builder = IndexBuilder::new();
    builder.add("NC_000021.9:g.5030578C>T", 749862939);
    let with_genome = Manifest {
        genome: Some("GRCh38.p14 primary assembly".to_string()),
        ..manifest()
    };
    builder.finish(dir.path(), with_genome).unwrap();
    let index = CaIndex::open(dir.path()).unwrap();
    assert_eq!(
        index.manifest().genome.as_deref(),
        Some("GRCh38.p14 primary assembly")
    );

    // A manifest written before the field existed still opens.
    let json = fs::read_to_string(dir.path().join("manifest.json")).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value.as_object_mut().unwrap().remove("genome");
    fs::write(
        dir.path().join("manifest.json"),
        serde_json::to_string(&value).unwrap(),
    )
    .unwrap();
    assert_eq!(CaIndex::open(dir.path()).unwrap().manifest().genome, None);
}

#[test]
fn manifest_json_is_pretty_printed() {
    let dir = tempdir().unwrap();
    build(dir.path(), &KEYS).unwrap();
    let json = fs::read_to_string(dir.path().join("manifest.json")).unwrap();
    assert!(json.contains("\n  \"assembly\": \"GRCh38\""), "{json}");
}

#[test]
fn finish_creates_missing_directories() {
    let dir = tempdir().unwrap();
    let nested = dir.path().join("a").join("b");
    build(&nested, &KEYS).unwrap();
    assert!(nested.join("caid.fst").is_file());
    assert!(nested.join("manifest.json").is_file());
    assert_eq!(
        CaIndex::open(&nested).unwrap().get(KEYS[0].0),
        Some(KEYS[0].1)
    );
}

#[test]
fn rebuild_replaces_previous_index() {
    let dir = tempdir().unwrap();
    build(dir.path(), &KEYS).unwrap();
    assert_eq!(build(dir.path(), &[("NC_000021.9:g.1A>T", 7)]).unwrap(), 1);

    let index = CaIndex::open(dir.path()).unwrap();
    assert_eq!(index.get("NC_000021.9:g.1A>T"), Some(7));
    assert_eq!(index.get(KEYS[0].0), None);
    assert_eq!(index.manifest().entries, 1);
}

#[test]
fn failed_rebuild_leaves_no_manifest_behind() {
    let dir = tempdir().unwrap();
    build(dir.path(), &KEYS).unwrap();

    // Make writing caid.fst fail: a directory is squatting on its name.
    fs::remove_file(dir.path().join("caid.fst")).unwrap();
    fs::create_dir(dir.path().join("caid.fst")).unwrap();
    match build(dir.path(), &KEYS).unwrap_err() {
        Error::Io { path, .. } => assert_eq!(path, dir.path().join("caid.fst")),
        other => panic!("expected Io, got {other:?}"),
    }
    assert!(
        !dir.path().join("manifest.json").exists(),
        "a stale manifest must not describe a half-written index"
    );
}

#[test]
fn empty_index() {
    let dir = tempdir().unwrap();
    assert_eq!(
        IndexBuilder::new().finish(dir.path(), manifest()).unwrap(),
        0
    );
    let index = CaIndex::open(dir.path()).unwrap();
    assert_eq!(index.get("NC_000021.9:g.5030578C>T"), None);
    assert_eq!(index.manifest().entries, 0);
}

#[test]
fn open_missing_directory_is_io_error_naming_the_manifest() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("nope");
    match CaIndex::open(&missing).unwrap_err() {
        Error::Io { path, .. } => assert_eq!(path, missing.join("manifest.json")),
        other => panic!("expected Io, got {other:?}"),
    }
}

#[test]
fn open_without_fst_is_io_error_naming_the_fst() {
    let dir = tempdir().unwrap();
    build(dir.path(), &KEYS).unwrap();
    fs::remove_file(dir.path().join("caid.fst")).unwrap();
    match CaIndex::open(dir.path()).unwrap_err() {
        Error::Io { path, .. } => assert_eq!(path, dir.path().join("caid.fst")),
        other => panic!("expected Io, got {other:?}"),
    }
}

#[test]
fn open_malformed_manifest_is_manifest_error() {
    let dir = tempdir().unwrap();
    build(dir.path(), &KEYS).unwrap();
    fs::write(dir.path().join("manifest.json"), "{ not json").unwrap();
    match CaIndex::open(dir.path()).unwrap_err() {
        Error::Manifest(msg) => assert!(msg.contains("manifest.json"), "{msg}"),
        other => panic!("expected Manifest, got {other:?}"),
    }
}

#[test]
fn open_corrupt_fst_is_fst_error() {
    let dir = tempdir().unwrap();
    build(dir.path(), &KEYS).unwrap();
    fs::write(dir.path().join("caid.fst"), b"this is not an fst").unwrap();
    match CaIndex::open(dir.path()).unwrap_err() {
        Error::Fst(_) => {}
        other => panic!("expected Fst, got {other:?}"),
    }
}

#[test]
fn ca_index_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CaIndex>();
}
