//! Provenance tracking and license verification for assets and external dependencies.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use thiserror::Error;

/// SHA-256 digest of an empty string (dummy placeholder), forbidden in production provenance.
pub const EMPTY_STRING_SHA256: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

#[derive(Debug, Error)]
pub enum ProvenanceError {
    #[error("I/O error reading provenance: {0}")]
    Io(#[from] std::io::Error),
    #[error("Deserialization error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("Validation failed for record '{id}': {reason}")]
    ValidationFailed { id: String, reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceStatus {
    Approved,
    Candidate,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenanceRecord {
    pub id: String,
    pub kind: String,
    pub source_url: String,
    pub revision: String,
    pub sha256: String,
    pub code_license: String,
    pub data_license: String,
    pub redistribution: String,
    pub purpose: String,
    pub split_role: String,
    pub status: ProvenanceStatus,
}

impl ProvenanceRecord {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("id must not be empty".to_string());
        }
        if self.code_license.eq_ignore_ascii_case("unknown")
            || self.data_license.eq_ignore_ascii_case("unknown")
        {
            return Err(format!(
                "license cannot be 'unknown' for record '{}'",
                self.id
            ));
        }
        let hash_trimmed = self.sha256.trim();
        if hash_trimmed.is_empty() {
            return Err(format!(
                "sha256 hash must be provided for record '{}'",
                self.id
            ));
        }
        if hash_trimmed.len() != 64 || hex::decode(hash_trimmed).is_err() {
            return Err(format!(
                "sha256 must be a valid 64-character hex string for record '{}'",
                self.id
            ));
        }
        if hash_trimmed.eq_ignore_ascii_case(EMPTY_STRING_SHA256) {
            return Err(format!(
                "record '{}' contains dummy empty-string sha256 hash",
                self.id
            ));
        }
        if self.status == ProvenanceStatus::Approved
            && self.redistribution.eq_ignore_ascii_case("prohibited")
        {
            return Err(format!(
                "record '{}' marked approved but redistribution is prohibited",
                self.id
            ));
        }
        Ok(())
    }

    /// Verifies the SHA-256 of a local file against this record.
    pub fn verify_file<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        self.validate()?;
        let path_ref = path.as_ref();
        let bytes = std::fs::read(path_ref)
            .map_err(|e| format!("Failed to read file '{}': {e}", path_ref.display()))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual_hash = hex::encode(hasher.finalize());
        if !actual_hash.eq_ignore_ascii_case(&self.sha256) {
            return Err(format!(
                "Hash mismatch for '{}': expected {}, got {actual_hash}",
                self.id, self.sha256
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProvenanceManifest {
    #[serde(default)]
    pub records: Vec<ProvenanceRecord>,
}

impl ProvenanceManifest {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ProvenanceError> {
        let content = std::fs::read_to_string(path)?;
        let manifest: ProvenanceManifest = toml::from_str(&content)?;
        manifest.validate_all()?;
        Ok(manifest)
    }

    pub fn from_toml_str(content: &str) -> Result<Self, ProvenanceError> {
        let manifest: ProvenanceManifest = toml::from_str(content)?;
        manifest.validate_all()?;
        Ok(manifest)
    }

    pub fn validate_all(&self) -> Result<(), ProvenanceError> {
        for record in &self.records {
            if let Err(reason) = record.validate() {
                return Err(ProvenanceError::ValidationFailed {
                    id: record.id.clone(),
                    reason,
                });
            }
        }
        Ok(())
    }
}
