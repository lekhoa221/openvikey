use openvikey_lab::provenance::{ProvenanceManifest, ProvenanceRecord, ProvenanceStatus};
use std::path::Path;

#[test]
fn test_workspace_smoke() {
    assert_eq!(openvikey_core::VERSION, "0.1.0");
}

#[test]
fn test_provenance_rejects_unknown_license() {
    let invalid = ProvenanceRecord {
        id: "bad-asset".to_string(),
        kind: "corpus".to_string(),
        source_url: "https://example.com".to_string(),
        revision: "1.0".to_string(),
        sha256: "abcdef".to_string(),
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
fn test_provenance_rejects_missing_hash() {
    let invalid = ProvenanceRecord {
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
}

#[test]
fn test_provenance_rejects_approved_with_prohibited_redistribution() {
    let invalid = ProvenanceRecord {
        id: "bad-asset".to_string(),
        kind: "corpus".to_string(),
        source_url: "https://example.com".to_string(),
        revision: "1.0".to_string(),
        sha256: "abcdef".to_string(),
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
fn test_project_provenance_file_valid() {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("data")
        .join("provenance.toml");
    let manifest = ProvenanceManifest::from_file(&manifest_path)
        .expect("Failed to load and validate data/provenance.toml");
    assert!(!manifest.records.is_empty());
}
