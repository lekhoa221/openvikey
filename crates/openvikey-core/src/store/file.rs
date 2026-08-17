//! Crash-aware encrypted file store with temp-write, fsync, replace, and backup recovery.

use crate::store::envelope;
use crate::store::{ModelStore, SecretProvider, StoreError};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Deterministic save interruption points used by recovery tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailurePoint {
    AfterTempSync,
    AfterBackup,
    BeforeReplace,
}

/// Encrypted model file. A sibling `.bak` stores the previous valid envelope.
#[derive(Debug, Clone)]
pub struct FileModelStore {
    path: PathBuf,
}

impl FileModelStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn backup_path(&self) -> PathBuf {
        sibling_with_suffix(&self.path, ".bak")
    }

    pub fn save_with_failure(
        &mut self,
        payload: &[u8],
        provider: &dyn SecretProvider,
        failure: Option<FailurePoint>,
    ) -> Result<(), StoreError> {
        let blob = envelope::seal(payload, provider)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp_path = sibling_with_suffix(&self.path, ".tmp");
        let backup_path = self.backup_path();
        let mut temp = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp_path)?;
        temp.write_all(&blob)?;
        temp.flush()?;
        temp.sync_all()?;
        drop(temp);

        if failure == Some(FailurePoint::AfterTempSync) {
            return Err(StoreError::Io(
                "injected failure after temp sync".to_string(),
            ));
        }

        if self.path.exists() {
            fs::copy(&self.path, &backup_path)?;
            sync_file(&backup_path)?;
        }
        if failure == Some(FailurePoint::AfterBackup) {
            return Err(StoreError::Io("injected failure after backup".to_string()));
        }
        if failure == Some(FailurePoint::BeforeReplace) {
            return Err(StoreError::Io(
                "injected failure before replace".to_string(),
            ));
        }

        replace_file(&temp_path, &self.path)?;
        if let Some(parent) = self.path.parent() {
            sync_directory(parent)?;
        }
        Ok(())
    }

    fn load_path(path: &Path, provider: &dyn SecretProvider) -> Result<Vec<u8>, StoreError> {
        let bytes = fs::read(path)?;
        envelope::open(&bytes, provider)
    }
}

impl ModelStore for FileModelStore {
    fn save(&mut self, payload: &[u8], provider: &dyn SecretProvider) -> Result<(), StoreError> {
        self.save_with_failure(payload, provider, None)
    }

    fn load(&self, provider: &dyn SecretProvider) -> Result<Vec<u8>, StoreError> {
        let primary = Self::load_path(&self.path, provider);
        match primary {
            Ok(payload) => Ok(payload),
            Err(StoreError::WrongPassphrase) => Err(StoreError::WrongPassphrase),
            Err(primary_error) => {
                let backup = self.backup_path();
                if !backup.exists() {
                    return Err(primary_error);
                }
                match Self::load_path(&backup, provider) {
                    Ok(payload) => Ok(payload),
                    Err(StoreError::WrongPassphrase) => Err(StoreError::WrongPassphrase),
                    Err(_) => Err(primary_error),
                }
            }
        }
    }
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let name = path
        .file_name()
        .map_or_else(|| "model".into(), std::ffi::OsStr::to_os_string);
    let mut suffixed = name;
    suffixed.push(suffix);
    path.with_file_name(suffixed)
}

fn sync_file(path: &Path) -> Result<(), StoreError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?
        .sync_all()?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn replace_file(temp: &Path, target: &Path) -> Result<(), StoreError> {
    fs::rename(temp, target)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn replace_file(temp: &Path, target: &Path) -> Result<(), StoreError> {
    if target.exists() {
        fs::remove_file(target)?;
    }
    fs::rename(temp, target)?;
    Ok(())
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), StoreError> {
    OpenOptions::new().read(true).open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)]
fn sync_directory(_path: &Path) -> Result<(), StoreError> {
    Ok(())
}
