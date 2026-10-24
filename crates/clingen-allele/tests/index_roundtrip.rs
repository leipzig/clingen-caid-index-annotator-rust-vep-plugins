//! `IndexBuilder` / `SortedIndexWriter` -> `caid.fst` + `manifest.json` ->
//! `CaIndex` round trips.

use std::fs;
use std::ops::ControlFlow;
use std::path::Path;

use clingen_allele::{
    CaIndex, Error, IndexBuilder, Manifest, SortedIndexWriter, format_caid, parse_caid,
};
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

    // The Registry zero-pads to six digits: every harvested ID with fewer
    // than seven digits has exactly six, and no longer one starts with 0.
    assert_eq!(format_caid(68622), "CA068622");
    assert_eq!(format_caid(42), "CA000042");
    assert_eq!(format_caid(0), "CA000000");
    assert_eq!(format_caid(1019413128), "CA1019413128");
    for id in ["CA068622", "CA000042", "CA120560", "CA1019413128"] {
        assert_eq!(format_caid(parse_caid(id).unwrap()), id);
    }
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

/// The file names in `dir`, sorted.
fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// `n` keys `NC_000021.9:g.{5000000 + i}C>T`, each with ID `id_base + i`.
fn numbered_keys(n: u64, id_base: u64) -> Vec<(String, u64)> {
    (0..n)
        .map(|i| (format!("NC_000021.9:g.{}C>T", 5_000_000 + i), id_base + i))
        .collect()
}

fn build_owned(dir: &Path, keys: &[(String, u64)]) -> Result<u64, Error> {
    let mut builder = IndexBuilder::new();
    for (hgvs, caid) in keys {
        builder.add(hgvs, *caid);
    }
    builder.finish(dir, manifest())
}

/// Assert that `index` maps each of `keys` to its ID.
fn assert_answers(index: &CaIndex, keys: &[(String, u64)]) {
    for (hgvs, caid) in keys {
        assert_eq!(index.get(hgvs), Some(*caid), "{hgvs}");
    }
}

#[test]
fn open_index_keeps_its_answers_across_rebuilds_of_its_dir() {
    let dir = tempdir().unwrap();
    let old = numbered_keys(1000, 1_000);
    build_owned(dir.path(), &old).unwrap();
    let live = CaIndex::open(dir.path()).unwrap();

    // A bigger index with other IDs, then a much smaller one: rewriting the
    // mapped file in place returns wrong IDs, None or panics for the first,
    // and reads past the end of the file (SIGBUS on Linux) for the second.
    let bigger = numbered_keys(2000, 50_000);
    build_owned(dir.path(), &bigger).unwrap();
    assert_answers(&live, &old);
    assert_eq!(live.manifest().entries, 1000);

    let smaller = numbered_keys(10, 90_000);
    build_owned(dir.path(), &smaller).unwrap();
    assert_answers(&live, &old);

    // A fresh open serves the latest build, and no temporary file is left.
    let fresh = CaIndex::open(dir.path()).unwrap();
    assert_answers(&fresh, &smaller);
    assert_eq!(fresh.get(&old[500].0), None);
    assert_eq!(fresh.manifest().entries, 10);
    assert_eq!(file_names(dir.path()), ["caid.fst", "manifest.json"]);
}

#[test]
fn conflicting_rebuild_leaves_the_previous_index_usable() {
    let dir = tempdir().unwrap();
    build(dir.path(), &KEYS).unwrap();
    let key = "NC_000021.9:g.5030578C>T";
    assert!(matches!(
        build(dir.path(), &[(key, 1), (key, 2)]),
        Err(Error::DuplicateConflict { .. })
    ));

    let index = CaIndex::open(dir.path()).unwrap();
    for (hgvs, caid) in KEYS {
        assert_eq!(index.get(hgvs), Some(caid), "{hgvs}");
    }
    assert_eq!(file_names(dir.path()), ["caid.fst", "manifest.json"]);
}

#[test]
fn rebuild_failing_to_write_either_file_leaves_the_previous_index_usable() {
    for squatted in ["caid.fst.tmp", "manifest.json.tmp"] {
        let dir = tempdir().unwrap();
        build(dir.path(), &KEYS).unwrap();

        // A directory squatting on the temporary name makes that write fail.
        fs::create_dir(dir.path().join(squatted)).unwrap();
        match build(dir.path(), &[("NC_000021.9:g.1A>T", 7)]).unwrap_err() {
            Error::Io { path, .. } => assert_eq!(path, dir.path().join(squatted)),
            other => panic!("{squatted}: expected Io, got {other:?}"),
        }

        let index = CaIndex::open(dir.path()).unwrap();
        for (hgvs, caid) in KEYS {
            assert_eq!(index.get(hgvs), Some(caid), "{squatted}: {hgvs}");
        }
        assert_eq!(index.get("NC_000021.9:g.1A>T"), None, "{squatted}");
        assert_eq!(index.manifest().entries, 4, "{squatted}");
        // The failed build removed what it had written; only the squatter
        // is left next to the old index.
        let mut expected = vec!["caid.fst", "manifest.json", squatted];
        expected.sort_unstable();
        assert_eq!(file_names(dir.path()), expected, "{squatted}");
    }
}

#[test]
fn stale_temporary_files_from_a_crashed_build_are_overwritten() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("caid.fst.tmp"), b"half an fst").unwrap();
    fs::write(dir.path().join("manifest.json.tmp"), b"{ \"format_ver").unwrap();

    assert_eq!(build(dir.path(), &KEYS).unwrap(), 4);
    let index = CaIndex::open(dir.path()).unwrap();
    for (hgvs, caid) in KEYS {
        assert_eq!(index.get(hgvs), Some(caid), "{hgvs}");
    }
    assert_eq!(file_names(dir.path()), ["caid.fst", "manifest.json"]);
}

#[test]
fn reader_opening_between_the_two_renames_gets_an_error() {
    // `finish` renames the new caid.fst into place before the new manifest,
    // so a reader opening in between pairs the new FST with the old
    // manifest. The entry-count check turns that into an error.
    let dir = tempdir().unwrap();
    build(dir.path(), &KEYS).unwrap();
    let next = tempdir().unwrap();
    build(next.path(), &KEYS[..1]).unwrap();
    fs::rename(next.path().join("caid.fst"), dir.path().join("caid.fst")).unwrap();

    match CaIndex::open(dir.path()).unwrap_err() {
        Error::Manifest(msg) => {
            assert!(msg.contains("lists 4 entries"), "{msg}");
            assert!(msg.contains("contains 1"), "{msg}");
        }
        other => panic!("expected Manifest, got {other:?}"),
    }
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

/// A 1000-key index in `dir`, big enough that cutting bytes off the end lands
/// inside the FST body rather than in a tiny header.
fn build_thousand(dir: &Path) {
    let mut builder = IndexBuilder::new();
    for i in 0..1000u64 {
        builder.add(&format!("NC_000021.9:g.{}C>T", 5_000_000 + i), 1_000 + i);
    }
    assert_eq!(builder.finish(dir, manifest()).unwrap(), 1000);
}

#[test]
fn open_truncated_fst_is_an_error_not_a_later_panic() {
    let full = tempdir().unwrap();
    build_thousand(full.path());
    let bytes = fs::read(full.path().join("caid.fst")).unwrap();

    let len = bytes.len();
    for kept in [len - 1, len - 5, len - 30, len / 2] {
        let dir = tempdir().unwrap();
        fs::copy(
            full.path().join("manifest.json"),
            dir.path().join("manifest.json"),
        )
        .unwrap();
        fs::write(dir.path().join("caid.fst"), &bytes[..kept]).unwrap();
        match CaIndex::open(dir.path()) {
            Err(Error::Manifest(_) | Error::Fst(_)) => {}
            Err(other) => panic!("cut to {kept} of {len} bytes: unexpected {other:?}"),
            Ok(_) => panic!("cut to {kept} of {len} bytes opened successfully"),
        }
    }
}

#[test]
fn open_truncated_fst_names_both_files_and_counts() {
    let dir = tempdir().unwrap();
    build_thousand(dir.path());
    let fst_path = dir.path().join("caid.fst");
    let bytes = fs::read(&fst_path).unwrap();
    fs::write(&fst_path, &bytes[..bytes.len() - 5]).unwrap();
    match CaIndex::open(dir.path()).unwrap_err() {
        Error::Manifest(msg) => {
            assert!(msg.contains("manifest.json"), "{msg}");
            assert!(msg.contains("caid.fst"), "{msg}");
            assert!(msg.contains("lists 1000 entries"), "{msg}");
        }
        other => panic!("expected Manifest, got {other:?}"),
    }
}

#[test]
fn open_fst_from_another_build_is_a_manifest_error() {
    let dir = tempdir().unwrap();
    build(dir.path(), &KEYS[..2]).unwrap();
    let other = tempdir().unwrap();
    build(other.path(), &KEYS[..1]).unwrap();
    // Two keys in the FST, a manifest from a one-key build.
    fs::copy(
        other.path().join("manifest.json"),
        dir.path().join("manifest.json"),
    )
    .unwrap();
    match CaIndex::open(dir.path()).unwrap_err() {
        Error::Manifest(msg) => {
            assert!(msg.contains("lists 1 entries"), "{msg}");
            assert!(msg.contains("contains 2"), "{msg}");
        }
        other => panic!("expected Manifest, got {other:?}"),
    }
}

#[test]
fn ca_index_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CaIndex>();
}

#[test]
fn sorted_writer_matches_index_builder_bytes() {
    let built = tempdir().unwrap();
    build_thousand(built.path());

    // The same pairs `build_thousand` adds, already in byte order (every
    // position has seven digits).
    let streamed = tempdir().unwrap();
    let mut writer = SortedIndexWriter::create(streamed.path()).unwrap();
    for (hgvs, caid) in numbered_keys(1000, 1_000) {
        assert!(writer.push(&hgvs, caid).unwrap(), "{hgvs}");
    }
    assert_eq!(writer.finish(manifest()).unwrap(), 1000);

    assert_eq!(
        fs::read(streamed.path().join("caid.fst")).unwrap(),
        fs::read(built.path().join("caid.fst")).unwrap()
    );
    assert_eq!(
        CaIndex::open(streamed.path()).unwrap().manifest(),
        CaIndex::open(built.path()).unwrap().manifest()
    );
    assert_eq!(file_names(streamed.path()), ["caid.fst", "manifest.json"]);
}

#[test]
fn sorted_writer_skips_exact_repeats() {
    let dir = tempdir().unwrap();
    let key = "NC_000021.9:g.5030578C>T";
    let mut writer = SortedIndexWriter::create(dir.path()).unwrap();
    assert!(writer.push(key, 1).unwrap());
    assert!(!writer.push(key, 1).unwrap());
    assert_eq!(writer.finish(manifest()).unwrap(), 1);

    let index = CaIndex::open(dir.path()).unwrap();
    assert_eq!(index.get(key), Some(1));
    assert_eq!(index.manifest().entries, 1);
}

#[test]
fn sorted_writer_rejects_conflict() {
    let dir = tempdir().unwrap();
    build(dir.path(), &KEYS).unwrap();

    let key = "NC_000021.9:g.5030578C>T";
    let mut writer = SortedIndexWriter::create(dir.path()).unwrap();
    assert!(writer.push(key, 1).unwrap());
    match writer.push(key, 2).unwrap_err() {
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
    // The error itself discards the build, before the writer is dropped.
    assert_eq!(file_names(dir.path()), ["caid.fst", "manifest.json"]);

    let index = CaIndex::open(dir.path()).unwrap();
    for (hgvs, caid) in KEYS {
        assert_eq!(index.get(hgvs), Some(caid), "{hgvs}");
    }
    assert_eq!(index.manifest().entries, 4);
}

#[test]
fn sorted_writer_cannot_finish_after_an_error() {
    let dir = tempdir().unwrap();
    build(dir.path(), &KEYS).unwrap();

    let mut writer = SortedIndexWriter::create(dir.path()).unwrap();
    assert!(writer.push("b", 1).unwrap());
    writer.push("a", 2).unwrap_err();
    // Neither a later pair nor `finish` revives the discarded build.
    writer.push("c", 3).unwrap_err();
    match writer.finish(manifest()).unwrap_err() {
        Error::Io { path, .. } => assert_eq!(path, dir.path().join("caid.fst.tmp")),
        other => panic!("expected Io, got {other:?}"),
    }

    let index = CaIndex::open(dir.path()).unwrap();
    for (hgvs, caid) in KEYS {
        assert_eq!(index.get(hgvs), Some(caid), "{hgvs}");
    }
    assert_eq!(index.get("b"), None);
    assert_eq!(file_names(dir.path()), ["caid.fst", "manifest.json"]);
}

#[test]
fn sorted_writer_rejects_out_of_order() {
    let dir = tempdir().unwrap();
    let mut writer = SortedIndexWriter::create(dir.path()).unwrap();
    assert!(writer.push("b", 1).unwrap());
    let err = writer.push("a", 2).unwrap_err();
    assert_eq!(err.to_string(), "keys out of order: a after b");
    match err {
        Error::Unsorted { previous, key } => {
            assert_eq!(previous, "b");
            assert_eq!(key, "a");
        }
        other => panic!("expected Unsorted, got {other:?}"),
    }
}

#[test]
fn sorted_writer_accepts_prefix_keys() {
    let dir = tempdir().unwrap();
    let mut writer = SortedIndexWriter::create(dir.path()).unwrap();
    // A key sorts before every longer key it is a prefix of.
    assert!(writer.push("NC_000021.9:g.12del", 1).unwrap());
    assert!(writer.push("NC_000021.9:g.12delinsA", 2).unwrap());
    assert_eq!(writer.finish(manifest()).unwrap(), 2);

    let index = CaIndex::open(dir.path()).unwrap();
    assert_eq!(index.get("NC_000021.9:g.12del"), Some(1));
    assert_eq!(index.get("NC_000021.9:g.12delinsA"), Some(2));
}

#[test]
fn sorted_writer_dropped_without_finish_leaves_old_index() {
    let dir = tempdir().unwrap();
    build(dir.path(), &KEYS).unwrap();

    let mut writer = SortedIndexWriter::create(dir.path()).unwrap();
    assert!(writer.push("NC_000021.9:g.1A>T", 7).unwrap());
    assert!(dir.path().join("caid.fst.tmp").is_file());
    drop(writer);

    let index = CaIndex::open(dir.path()).unwrap();
    for (hgvs, caid) in KEYS {
        assert_eq!(index.get(hgvs), Some(caid), "{hgvs}");
    }
    assert_eq!(index.get("NC_000021.9:g.1A>T"), None);
    assert_eq!(file_names(dir.path()), ["caid.fst", "manifest.json"]);
}

#[test]
fn visit_prefix_walks_in_byte_order() {
    let dir = tempdir().unwrap();
    // Added out of order; the walk must come back in byte order.
    let keys = [
        ("NC_012920.1:m.3243A>G", 5),
        ("NC_000021.9:g.5030578C>T", 3),
        ("NC_000001.11:g.1000A>T", 1),
        ("NC_000021.9:g.33000172dup", 2),
        ("NC_000021.9:g.33000172del", 4),
    ];
    build(dir.path(), &keys).unwrap();
    let index = CaIndex::open(dir.path()).unwrap();

    let walk = |prefix: &[u8]| {
        let mut seen = Vec::new();
        index.visit_prefix(prefix, |key, caid| {
            seen.push((String::from_utf8(key.to_vec()).unwrap(), caid));
            ControlFlow::Continue(())
        });
        seen
    };
    let owned = |pairs: &[(&str, u64)]| -> Vec<(String, u64)> {
        pairs.iter().map(|&(k, v)| (k.to_string(), v)).collect()
    };

    assert_eq!(
        walk(b"NC_000021.9:"),
        owned(&[
            ("NC_000021.9:g.33000172del", 4),
            ("NC_000021.9:g.33000172dup", 2),
            ("NC_000021.9:g.5030578C>T", 3),
        ])
    );
    assert_eq!(
        walk(&[]),
        owned(&[
            ("NC_000001.11:g.1000A>T", 1),
            ("NC_000021.9:g.33000172del", 4),
            ("NC_000021.9:g.33000172dup", 2),
            ("NC_000021.9:g.5030578C>T", 3),
            ("NC_012920.1:m.3243A>G", 5),
        ])
    );
    assert_eq!(walk(b"NC_000022.11:"), owned(&[]));

    let mut visits = 0;
    index.visit_prefix(&[], |_, _| {
        visits += 1;
        ControlFlow::Break(())
    });
    assert_eq!(visits, 1);
}
