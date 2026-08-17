//! Milestone 4: corpus manifest, split, hash, and sample-count gates.

use openvikey_core::lexicon::{Lexicon, LexiconArtifact};
use openvikey_lab::corpus::{
    EvaluationMode, RELEASE_MIN_CORRECT_TOKENS, RELEASE_MIN_ERROR_CASES,
    RELEASE_MIN_PER_ERROR_TYPE, VerifiedCorpus, build_lexicon, load_and_verify,
};
use openvikey_lab::metrics::{auto_precision, correct_token_fpr, wilson_interval};
use openvikey_lab::provenance::sha256_file;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn scratch_dir() -> PathBuf {
    let n = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("openvikey-m4-{}-{}", std::process::id(), n));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("data/fixtures/corpus")).unwrap();
    dir
}

fn item(id: &str, label: &str, error_type: Option<&str>, input: &str, gold: &str) -> String {
    match error_type {
        Some(kind) => format!(
            r#"{{"id":"{id}","label":"{label}","error_type":"{kind}","input":"{input}","gold":"{gold}"}}"#
        ),
        None => format!(r#"{{"id":"{id}","label":"{label}","input":"{input}","gold":"{gold}"}}"#),
    }
}

fn write_jsonl(path: &Path, lines: &[String]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, lines.join("\n") + "\n").unwrap();
}

fn approved_record(id: &str, artifact: &str, sha256: &str, split_role: &str) -> String {
    format!(
        r#"
[[records]]
id = "{id}"
kind = "project_fixtures"
source_url = "https://github.com/lekhoa221/openvikey"
revision = "HEAD"
sha256 = "{sha256}"
code_license = "MIT"
data_license = "MIT"
redistribution = "allowed"
purpose = "test"
split_role = "{split_role}"
status = "approved"
artifact = "{artifact}"
"#
    )
}

fn write_tree(
    root: &Path,
    train: &[String],
    calibration: &[String],
    held_out: &[String],
    mutate_record: Option<fn(&mut String)>,
    wrong_hash: bool,
) -> PathBuf {
    let train_rel = "data/fixtures/corpus/train.jsonl";
    let cal_rel = "data/fixtures/corpus/calibration.jsonl";
    let held_rel = "data/fixtures/corpus/held_out.jsonl";
    write_jsonl(&root.join(train_rel), train);
    write_jsonl(&root.join(cal_rel), calibration);
    write_jsonl(&root.join(held_rel), held_out);

    let train_hash = sha256_file(&root.join(train_rel)).unwrap();
    let cal_hash = sha256_file(&root.join(cal_rel)).unwrap();
    let held_hash = sha256_file(&root.join(held_rel)).unwrap();
    let declared_train = if wrong_hash {
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    } else {
        train_hash.as_str()
    };

    let mut provenance = String::new();
    provenance.push_str(&approved_record(
        "fix-train",
        train_rel,
        declared_train,
        "train",
    ));
    provenance.push_str(&approved_record(
        "fix-cal",
        cal_rel,
        &cal_hash,
        "calibration",
    ));
    provenance.push_str(&approved_record(
        "fix-held", held_rel, &held_hash, "held_out",
    ));
    if let Some(mutate) = mutate_record {
        mutate(&mut provenance);
    }
    fs::write(root.join("data/provenance.toml"), provenance).unwrap();

    let manifest = format!(
        r#"
version = "1.0.0"
provenance = "data/provenance.toml"

[evaluation]
min_correct_tokens_release = {RELEASE_MIN_CORRECT_TOKENS}
min_error_cases_release = {RELEASE_MIN_ERROR_CASES}
min_per_error_type_release = {RELEASE_MIN_PER_ERROR_TYPE}

[[assets]]
id = "train"
path = "{train_rel}"
sha256 = "{declared_train}"
split_role = "train"
provenance_id = "fix-train"

[[assets]]
id = "calibration"
path = "{cal_rel}"
sha256 = "{cal_hash}"
split_role = "calibration"
provenance_id = "fix-cal"

[[assets]]
id = "held_out"
path = "{held_rel}"
sha256 = "{held_hash}"
split_role = "held_out"
provenance_id = "fix-held"
"#
    );
    let manifest_path = root.join("data/corpus-manifest.toml");
    fs::write(&manifest_path, manifest).unwrap();
    manifest_path
}

fn tiny_valid(root: &Path) -> PathBuf {
    write_tree(
        root,
        &[item("tr-1", "error", Some("abbrev"), "ko", "không")],
        &[item("ca-1", "error", Some("tone"), "ch2ao", "chào")],
        &[
            item("ho-1", "correct", None, "xin", "xin"),
            item("ho-2", "error", Some("transpose"), "khọgn", "không"),
        ],
        None,
        false,
    )
}

#[test]
fn rejects_blocked_status_even_when_licenses_look_permissive() {
    let root = scratch_dir();
    let manifest = write_tree(
        &root,
        &[item("tr-1", "error", Some("abbrev"), "ko", "không")],
        &[item("ca-1", "error", Some("tone"), "ch2ao", "chào")],
        &[item("ho-1", "correct", None, "xin", "xin")],
        Some(|prov| {
            *prov = prov.replacen("status = \"approved\"", "status = \"blocked\"", 1);
        }),
        false,
    );
    let err = load_and_verify(&manifest, &root, EvaluationMode::Unit)
        .expect_err("blocked records must not ship via corpus verify");
    assert!(
        err.to_string().to_ascii_lowercase().contains("blocked")
            || err.to_string().to_ascii_lowercase().contains("approved"),
        "unexpected error: {err}"
    );
}

#[test]
fn rejects_unknown_revision() {
    let root = scratch_dir();
    let manifest = write_tree(
        &root,
        &[item("tr-1", "error", Some("abbrev"), "ko", "không")],
        &[item("ca-1", "error", Some("tone"), "ch2ao", "chào")],
        &[item("ho-1", "correct", None, "xin", "xin")],
        Some(|prov| {
            *prov = prov.replacen("revision = \"HEAD\"", "revision = \"unknown\"", 1);
        }),
        false,
    );
    let err = load_and_verify(&manifest, &root, EvaluationMode::Unit)
        .expect_err("unknown revision must fail");
    assert!(
        err.to_string().to_ascii_lowercase().contains("revision"),
        "unexpected error: {err}"
    );
}

#[test]
fn rejects_gpl_data_license() {
    let root = scratch_dir();
    let manifest = write_tree(
        &root,
        &[item("tr-1", "error", Some("abbrev"), "ko", "không")],
        &[item("ca-1", "error", Some("tone"), "ch2ao", "chào")],
        &[item("ho-1", "correct", None, "xin", "xin")],
        Some(|prov| {
            *prov = prov.replacen("data_license = \"MIT\"", "data_license = \"GPL-3.0\"", 1);
        }),
        false,
    );
    let err = load_and_verify(&manifest, &root, EvaluationMode::Unit)
        .expect_err("copyleft data license must fail");
    assert!(
        err.to_string().to_ascii_lowercase().contains("license"),
        "unexpected error: {err}"
    );
}

#[test]
fn rejects_unknown_or_unverified_data_license() {
    let root = scratch_dir();
    let manifest = write_tree(
        &root,
        &[item("tr-1", "error", Some("abbrev"), "ko", "không")],
        &[item("ca-1", "error", Some("tone"), "ch2ao", "chào")],
        &[item("ho-1", "correct", None, "xin", "xin")],
        Some(|prov| {
            *prov = prov.replacen("data_license = \"MIT\"", "data_license = \"Unverified\"", 1);
        }),
        false,
    );
    let err = load_and_verify(&manifest, &root, EvaluationMode::Unit)
        .expect_err("unverified data license must fail even if code license is MIT");
    let msg = err.to_string().to_ascii_lowercase();
    assert!(
        msg.contains("license") || msg.contains("unverified"),
        "unexpected error: {err}"
    );
}

#[test]
fn rejects_missing_revision_hash_or_redistribution() {
    let root = scratch_dir();
    let manifest = write_tree(
        &root,
        &[item("tr-1", "error", Some("abbrev"), "ko", "không")],
        &[item("ca-1", "error", Some("tone"), "ch2ao", "chào")],
        &[item("ho-1", "correct", None, "xin", "xin")],
        Some(|prov| {
            *prov = prov.replacen(
                "redistribution = \"allowed\"",
                "redistribution = \"unknown\"",
                1,
            );
        }),
        false,
    );
    let err = load_and_verify(&manifest, &root, EvaluationMode::Unit)
        .expect_err("unknown redistribution must fail");
    assert!(
        err.to_string()
            .to_ascii_lowercase()
            .contains("redistribution"),
        "unexpected error: {err}"
    );
}

#[test]
fn rejects_train_calibration_held_out_id_overlap() {
    let root = scratch_dir();
    let manifest = write_tree(
        &root,
        &[item("dup-1", "error", Some("abbrev"), "ko", "không")],
        &[item("ca-1", "error", Some("tone"), "ch2ao", "chào")],
        &[item("dup-1", "error", Some("abbrev"), "ko", "không")],
        None,
        false,
    );
    let err = load_and_verify(&manifest, &root, EvaluationMode::Unit)
        .expect_err("same id in train and held_out must fail");
    let msg = err.to_string().to_ascii_lowercase();
    assert!(
        msg.contains("overlap") || msg.contains("dup-1"),
        "unexpected error: {err}"
    );
}

#[test]
fn rejects_asset_file_hash_mismatch() {
    let root = scratch_dir();
    let manifest = write_tree(
        &root,
        &[item("tr-1", "error", Some("abbrev"), "ko", "không")],
        &[item("ca-1", "error", Some("tone"), "ch2ao", "chào")],
        &[item("ho-1", "correct", None, "xin", "xin")],
        None,
        true,
    );
    let err = load_and_verify(&manifest, &root, EvaluationMode::Unit)
        .expect_err("declared sha256 must match file bytes");
    let msg = err.to_string().to_ascii_lowercase();
    assert!(
        msg.contains("hash") || msg.contains("sha"),
        "unexpected error: {err}"
    );
}

#[test]
fn metric_fixture_precision_and_fpr_use_different_denominators() {
    let precision = auto_precision(10, 1).unwrap();
    let fpr = correct_token_fpr(1, 1000).unwrap();
    assert!((precision - 10.0 / 11.0).abs() < 1e-12);
    assert!((fpr - 0.001).abs() < 1e-12);
    assert!((precision - fpr).abs() > 0.5);
}

#[test]
fn wilson_interval_matches_hand_calculated_cases() {
    let (lo, hi) = wilson_interval(81, 87, 1.96).unwrap();
    assert!((lo - 0.857_601_338_6).abs() < 1e-9);
    assert!((hi - 0.968_011_596_4).abs() < 1e-9);
}

#[test]
fn release_mode_enforces_minimum_sample_counts_unit_mode_does_not() {
    let root = scratch_dir();
    let manifest = tiny_valid(&root);
    load_and_verify(&manifest, &root, EvaluationMode::Unit)
        .expect("authored fixtures are valid in unit mode");
    let err = load_and_verify(&manifest, &root, EvaluationMode::Release)
        .expect_err("release mode must reject tiny fixtures");
    let msg = err.to_string().to_ascii_lowercase();
    assert!(
        msg.contains("sample") || msg.contains("minimum") || msg.contains("50"),
        "unexpected error: {err}"
    );
}

#[test]
fn authored_workspace_manifest_verifies_in_unit_mode() {
    let root = workspace_root();
    let manifest = root.join("data/corpus-manifest.toml");
    let corpus: VerifiedCorpus =
        load_and_verify(&manifest, &root, EvaluationMode::Unit).expect("workspace unit corpus");
    assert!(!corpus.items_in("train").is_empty());
    assert!(!corpus.items_in("calibration").is_empty());
    assert!(!corpus.items_in("held_out").is_empty());
}

#[test]
fn workspace_release_mode_rejects_tiny_authored_corpus() {
    let root = workspace_root();
    let manifest = root.join("data/corpus-manifest.toml");
    load_and_verify(&manifest, &root, EvaluationMode::Release)
        .expect_err("authored fixtures must fail release sample floors");
}

#[test]
fn committed_lexicon_artifact_parses_and_pins_manifest_hash() {
    let root = workspace_root();
    let manifest = root.join("data/corpus-manifest.toml");
    let bytes = fs::read(root.join("data/fixtures/lexicon/authored.json")).unwrap();
    let artifact: LexiconArtifact = serde_json::from_slice(&bytes).expect("authored.json");
    assert_eq!(
        artifact.source_manifest_hash,
        sha256_file(&manifest).unwrap()
    );
    let lexicon = Lexicon::from_artifact(artifact);
    assert!(lexicon.contains("không"));
    assert!(lexicon.contains("nam"));
    assert!(lexicon.bigram("xin", "chào").is_some());
}

#[test]
fn lexicon_lookup_normalizes_nfd_to_nfc() {
    let nfd = "e\u{0301}";
    let nfc = "é";
    assert_ne!(nfd, nfc);
    let lexicon = Lexicon::from_entries(
        [openvikey_core::lexicon::LexiconEntry {
            token_nfc: nfd.to_string(),
            frequency: 1,
        }],
        [],
        None,
    );
    assert!(lexicon.contains(nfc));
    assert!(lexicon.lookup(nfd).is_some());
}

#[test]
fn lexicon_artifact_records_manifest_hash_and_is_deterministic() {
    let root = scratch_dir();
    let manifest = tiny_valid(&root);
    let corpus = load_and_verify(&manifest, &root, EvaluationMode::Unit).unwrap();
    let first = build_lexicon(&corpus, &manifest).expect("build lexicon");
    let second = build_lexicon(&corpus, &manifest).expect("rebuild");
    let expected_hash = sha256_file(&manifest).unwrap();
    assert_eq!(first.source_manifest_hash(), Some(expected_hash.as_str()));
    assert_eq!(first, second);
    assert!(first.contains("không"));
}
