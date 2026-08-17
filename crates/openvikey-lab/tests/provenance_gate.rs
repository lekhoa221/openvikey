use openvikey_lab::provenance::{
    EMPTY_STRING_SHA256, ProvenanceManifest, ProvenanceRecord, ProvenanceStatus,
};
use std::path::Path;

#[test]
fn test_workspace_smoke_and_toolchain_pinning() {
    assert_eq!(openvikey_core::VERSION, "0.1.0");

    let toolchain_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("rust-toolchain.toml");
    let content = std::fs::read_to_string(&toolchain_path)
        .expect("rust-toolchain.toml must exist at repo root");
    assert!(
        content.contains("1.96.0"),
        "Toolchain channel must be pinned to 1.96.0"
    );
}

#[test]
fn test_provenance_rejects_unknown_license() {
    let invalid = ProvenanceRecord {
        id: "bad-asset".to_string(),
        kind: "corpus".to_string(),
        source_url: "https://example.com".to_string(),
        revision: "1.0".to_string(),
        sha256: "db81dabf1c7ad0830bdbe3b5723f491e10297581507967c22bd1ea38fc766734".to_string(),
        code_license: "unknown".to_string(),
        data_license: "MIT".to_string(),
        redistribution: "allowed".to_string(),
        purpose: "test".to_string(),
        split_role: "test".to_string(),
        status: ProvenanceStatus::Candidate,
    };
    assert!(invalid.validate().is_err());
}

#[test]
fn test_provenance_rejects_missing_or_invalid_hash() {
    let mut invalid = ProvenanceRecord {
        id: "bad-asset".to_string(),
        kind: "corpus".to_string(),
        source_url: "https://example.com".to_string(),
        revision: "1.0".to_string(),
        sha256: String::new(),
        code_license: "MIT".to_string(),
        data_license: "MIT".to_string(),
        redistribution: "allowed".to_string(),
        purpose: "test".to_string(),
        split_role: "test".to_string(),
        status: ProvenanceStatus::Candidate,
    };
    assert!(invalid.validate().is_err());

    // Non-hex hash
    invalid.sha256 = "not_a_valid_hex_hash_at_all".to_string();
    assert!(invalid.validate().is_err());

    // Dummy empty-string SHA-256 hash
    invalid.sha256 = EMPTY_STRING_SHA256.to_string();
    assert!(invalid.validate().is_err());
}

#[test]
fn test_provenance_rejects_approved_with_prohibited_redistribution() {
    let invalid = ProvenanceRecord {
        id: "bad-asset".to_string(),
        kind: "corpus".to_string(),
        source_url: "https://example.com".to_string(),
        revision: "1.0".to_string(),
        sha256: "db81dabf1c7ad0830bdbe3b5723f491e10297581507967c22bd1ea38fc766734".to_string(),
        code_license: "MIT".to_string(),
        data_license: "MIT".to_string(),
        redistribution: "prohibited".to_string(),
        purpose: "test".to_string(),
        split_role: "test".to_string(),
        status: ProvenanceStatus::Approved,
    };
    assert!(invalid.validate().is_err());
}

#[test]
fn test_project_provenance_file_and_fixture_hash() {
    let data_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("data");

    let manifest_path = data_dir.join("provenance.toml");
    let manifest = ProvenanceManifest::from_file(&manifest_path)
        .expect("Failed to load and validate data/provenance.toml");
    assert!(!manifest.records.is_empty());

    // Find openvikey-fixtures record and verify its real hash
    let fixture_record = manifest
        .records
        .iter()
        .find(|r| r.id == "openvikey-fixtures")
        .expect("openvikey-fixtures record must exist");

    let fixture_file = data_dir
        .join("fixtures")
        .join("engine")
        .join("telex_golden.jsonl");
    fixture_record
        .verify_file(&fixture_file)
        .expect("Fixture file hash must match provenance record");
}
