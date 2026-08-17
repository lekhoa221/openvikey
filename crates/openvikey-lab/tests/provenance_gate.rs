use openvikey_lab::provenance::{
    EMPTY_STRING_SHA256, PENDING_SHA256, ProvenanceManifest, ProvenanceRecord, ProvenanceStatus,
};
use std::path::Path;

fn workspace_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn sample_record(sha256: &str, status: ProvenanceStatus) -> ProvenanceRecord {
    ProvenanceRecord {
        id: "sample".to_string(),
        kind: "corpus".to_string(),
        source_url: "https://example.com".to_string(),
        revision: "1.0".to_string(),
        sha256: sha256.to_string(),
        code_license: "MIT".to_string(),
        data_license: "MIT".to_string(),
        redistribution: "allowed".to_string(),
        purpose: "test".to_string(),
        split_role: "test".to_string(),
        status,
        artifact: None,
    }
}

#[test]
fn test_workspace_smoke_and_toolchain_pinning() {
    assert_eq!(openvikey_core::VERSION, "0.1.0");

    let content = std::fs::read_to_string(workspace_root().join("rust-toolchain.toml"))
        .expect("rust-toolchain.toml must exist at repo root");
    assert!(
        content.contains("1.96.0"),
        "Toolchain channel must be pinned to 1.96.0"
    );
}

#[test]
fn test_provenance_rejects_unknown_license() {
    let mut invalid = sample_record(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ProvenanceStatus::Candidate,
    );
    invalid.code_license = "unknown".to_string();
    assert!(invalid.validate().is_err());
}

#[test]
fn test_provenance_rejects_missing_or_invalid_hash() {
    let mut invalid = sample_record("", ProvenanceStatus::Candidate);
    assert!(invalid.validate().is_err());

    invalid.sha256 = "not_a_valid_hex_hash_at_all".to_string();
    assert!(invalid.validate().is_err());

    invalid.sha256 = EMPTY_STRING_SHA256.to_string();
    assert!(invalid.validate().is_err());
}

#[test]
fn test_provenance_rejects_moving_head_revision() {
    let mut invalid = sample_record(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ProvenanceStatus::Approved,
    );
    invalid.revision = "HEAD".to_string();
    invalid.artifact = Some("data/example.jsonl".to_string());
    assert!(invalid.validate().is_err());
}

#[test]
fn test_provenance_rejects_approved_with_prohibited_redistribution() {
    let mut invalid = sample_record(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ProvenanceStatus::Approved,
    );
    invalid.redistribution = "prohibited".to_string();
    assert!(invalid.validate().is_err());
}

#[test]
fn test_validate_rejects_sha256_of_revision_string_for_approved() {
    let mut invalid = sample_record(
        "db81dabf1c7ad0830bdbe3b5723f491e10297581507967c22bd1ea38fc766734",
        ProvenanceStatus::Approved,
    );
    invalid.id = "vi-rs".to_string();
    invalid.revision = "192e246d37e83094a1228e798b160f3cee14c879".to_string();
    let err = invalid
        .validate()
        .expect_err("hashing the git revision string must not count as a content checksum");
    assert!(
        err.contains("revision"),
        "error should name the revision-digest anti-pattern, got: {err}"
    );
}

#[test]
fn test_pending_hash_allowed_only_for_non_approved() {
    let blocked = sample_record(PENDING_SHA256, ProvenanceStatus::Blocked);
    assert!(blocked.validate().is_ok());

    let approved = sample_record(PENDING_SHA256, ProvenanceStatus::Approved);
    assert!(approved.validate().is_err());
}

#[test]
fn test_approved_record_requires_artifact() {
    let approved = sample_record(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ProvenanceStatus::Approved,
    );
    assert!(approved.validate().is_err());
}

#[test]
fn test_missing_hash_fixture_file_is_rejected() {
    let path = workspace_root()
        .join("data")
        .join("fixtures")
        .join("provenance")
        .join("missing-hash.toml");
    let err = ProvenanceManifest::from_file(&path)
        .expect_err("missing-hash fixture must fail provenance validation");
    let message = err.to_string();
    assert!(
        message.contains("sha256") || message.contains("hash"),
        "expected a hash validation error, got: {message}"
    );
}

#[test]
fn test_project_provenance_file_and_artifact_hashes() {
    let root = workspace_root();
    let manifest_path = root.join("data").join("provenance.toml");
    let manifest = ProvenanceManifest::from_file(&manifest_path)
        .expect("Failed to load and validate data/provenance.toml");
    manifest
        .verify_artifacts(&root)
        .expect("declared artifacts must match sha256");
    assert!(
        manifest
            .records
            .iter()
            .any(|r| r.id == "openvikey-fixtures-telex")
    );
    assert!(
        manifest
            .records
            .iter()
            .any(|r| r.id == "openvikey-fixtures-vni")
    );
    assert!(
        manifest
            .records
            .iter()
            .any(|r| r.id == "openvikey-abbrev-seed")
    );
    assert!(manifest.records.iter().all(|record| {
        record.status != ProvenanceStatus::Approved || !record.revision.eq_ignore_ascii_case("HEAD")
    }));
}
