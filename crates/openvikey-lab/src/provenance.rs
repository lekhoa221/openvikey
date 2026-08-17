//! Provenance tracking and license verification for assets and external dependencies.

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

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
        if self.sha256.trim().is_empty() {
            return Err(format!(
                "sha256 hash must be provided for record '{}'",
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
