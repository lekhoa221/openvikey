//! Provenance tracking and license verification for assets and external dependencies.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// SHA-256 digest of an empty string (dummy placeholder), forbidden in production provenance.
pub const EMPTY_STRING_SHA256: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Placeholder allowed only for blocked/candidate records that have no local artifact yet.
pub const PENDING_SHA256: &str = "pending";

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
    /// Workspace-relative file, or `cargo-git:<package>` for a fetched git crate.
    #[serde(default)]
    pub artifact: Option<String>,
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
        if hash_trimmed.eq_ignore_ascii_case(PENDING_SHA256) {
            if self.status == ProvenanceStatus::Approved {
                return Err(format!(
                    "approved record '{}' cannot use pending sha256",
                    self.id
                ));
            }
            return Ok(());
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
        let revision_digest = hex::encode(Sha256::digest(self.revision.as_bytes()));
        if hash_trimmed.eq_ignore_ascii_case(&revision_digest) {
            return Err(format!(
                "record '{}' sha256 is the digest of the revision string, not artifact content",
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
        if self.status == ProvenanceStatus::Approved
            && self.artifact.as_ref().is_none_or(|a| a.trim().is_empty())
        {
            return Err(format!(
                "approved record '{}' must declare an artifact to checksum",
                self.id
            ));
        }
        Ok(())
    }

    /// Verifies the SHA-256 of a local file against this record.
    pub fn verify_file<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        self.validate()?;
        let path_ref = path.as_ref();
        let actual_hash = sha256_file(path_ref)
            .map_err(|e| format!("Failed to hash file '{}': {e}", path_ref.display()))?;
        if !actual_hash.eq_ignore_ascii_case(&self.sha256) {
            return Err(format!(
                "Hash mismatch for '{}': expected {}, got {actual_hash}",
                self.id, self.sha256
            ));
        }
        Ok(())
    }

    /// Verifies SHA-256 of a crate/library tree (`Cargo.toml`, `LICENSE*`, `src/`).
    pub fn verify_library_tree<P: AsRef<Path>>(&self, crate_root: P) -> Result<(), String> {
        self.validate()?;
        let actual_hash = sha256_library_tree(crate_root.as_ref()).map_err(|e| {
            format!(
                "Failed to hash crate '{}': {e}",
                crate_root.as_ref().display()
            )
        })?;
        if !actual_hash.eq_ignore_ascii_case(&self.sha256) {
            return Err(format!(
                "Hash mismatch for '{}': expected {}, got {actual_hash}",
                self.id, self.sha256
            ));
        }
        Ok(())
    }

    /// Verifies `artifact` when present: repo-relative file or `cargo-git:<checkout-prefix>`.
    pub fn verify_declared_artifact(&self, workspace_root: &Path) -> Result<(), String> {
        let Some(artifact) = self.artifact.as_deref() else {
            return Ok(());
        };
        if self.sha256.eq_ignore_ascii_case(PENDING_SHA256) {
            return Ok(());
        }
        if let Some(package) = artifact.strip_prefix("cargo-git:") {
            let crate_root = find_cargo_git_checkout(package, &self.revision)?;
            return self.verify_library_tree(crate_root);
        }
        self.verify_file(workspace_root.join(artifact))
    }
}

/// SHA-256 hex of a single file.
pub fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let bytes = fs::read(path)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

/// Deterministic SHA-256 of library sources: `Cargo.toml`, `LICENSE*`, and everything under `src/`.
///
/// Paths are hashed with `/` separators in sorted order so the digest is independent of OS.
pub fn sha256_library_tree(crate_root: &Path) -> Result<String, std::io::Error> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    collect_named(crate_root, "Cargo.toml", &mut files)?;
    collect_license_files(crate_root, &mut files)?;
    collect_dir(crate_root, &crate_root.join("src"), &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (rel, bytes) in files {
        hasher.update(rel.as_bytes());
        hasher.update([0u8]);
        hasher.update(&bytes);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn collect_named(
    root: &Path,
    name: &str,
    files: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), std::io::Error> {
    let path = root.join(name);
    if path.is_file() {
        files.push((name.replace('\\', "/"), fs::read(path)?));
    }
    Ok(())
}

fn collect_license_files(
    root: &Path,
    files: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), std::io::Error> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if entry.file_type()?.is_file() && name.to_ascii_uppercase().starts_with("LICENSE") {
            files.push((name.replace('\\', "/"), fs::read(entry.path())?));
        }
    }
    Ok(())
}

fn collect_dir(
    root: &Path,
    dir: &Path,
    files: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), std::io::Error> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_dir(root, &path, files)?;
        } else if entry.file_type()?.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            files.push((rel, fs::read(path)?));
        }
    }
    Ok(())
}

/// Locates a fetched cargo git checkout for `package` at `revision` prefix.
pub fn find_cargo_git_checkout(package: &str, revision: &str) -> Result<PathBuf, String> {
    let cargo_home = cargo_home_dir()?;
    let checkouts = cargo_home.join("git").join("checkouts");
    let entries = fs::read_dir(&checkouts).map_err(|e| {
        format!(
            "cannot read cargo git checkouts {}: {e}",
            checkouts.display()
        )
    })?;
    let rev_prefix = if revision.len() >= 7 {
        &revision[..7]
    } else {
        revision
    };
    let mut matches = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(package) {
            continue;
        }
        let rev_dir = entry.path().join(rev_prefix);
        if rev_dir.join("Cargo.toml").is_file() {
            matches.push(rev_dir);
        }
    }
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(format!(
            "no cargo git checkout for '{package}' rev {rev_prefix}"
        )),
        _ => Err(format!(
            "multiple cargo git checkouts for '{package}' rev {rev_prefix}"
        )),
    }
}

fn cargo_home_dir() -> Result<PathBuf, String> {
    if let Ok(home) = std::env::var("CARGO_HOME") {
        return Ok(PathBuf::from(home));
    }
    let user_home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| "CARGO_HOME, USERPROFILE, and HOME are unset".to_string())?;
    Ok(PathBuf::from(user_home).join(".cargo"))
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

    pub fn verify_artifacts(&self, workspace_root: &Path) -> Result<(), ProvenanceError> {
        for record in &self.records {
            if let Err(reason) = record.verify_declared_artifact(workspace_root) {
                return Err(ProvenanceError::ValidationFailed {
                    id: record.id.clone(),
                    reason,
                });
            }
        }
        Ok(())
    }
}
