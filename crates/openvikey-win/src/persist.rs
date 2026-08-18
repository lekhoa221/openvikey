//! Default store paths and save_snapshot seal helpers (no capture_log on save).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use crate::host::TypingHost;

use openvikey_core::store::ModelStore;
use openvikey_core::store::StoreError;
use openvikey_core::store::file::FileModelStore;
use openvikey_core::store::passphrase::PassphraseProvider;
use openvikey_session::capture::{
    CAPTURE_VERSION, CaptureHeader, CaptureLog, SessionStoreError, sha256_hex,
};
use openvikey_session::session::SessionSaveSnapshot;
use thiserror::Error;

/// Cooperative shutdown flag for the host message loop (unit-tested without GetMessage).
#[derive(Debug, Default)]
pub struct HostShutdown {
    stop: AtomicBool,
}

impl HostShutdown {
    #[must_use]
    pub fn new() -> Self {
        Self {
            stop: AtomicBool::new(false),
        }
    }

    /// Request exit; a fake or real message loop observes [`Self::is_requested`].
    pub fn run(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    #[must_use]
    pub fn is_requested(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    #[must_use]
    pub fn flag(&self) -> &AtomicBool {
        &self.stop
    }
}

#[derive(Debug, Error)]
pub enum PersistError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Session(#[from] SessionStoreError),
    #[error("model serialize: {0}")]
    Model(String),
    #[error("capture serialize: {0}")]
    Capture(String),
}

/// Resolve `%LOCALAPPDATA%\OpenViKey\model.ovk` and `capture.ovk`.
///
/// `local_app_data` is typically `std::env::var_os("LOCALAPPDATA")`; tests pass a fixed root.
#[must_use]
pub fn default_store_paths(local_app_data: Option<impl AsRef<Path>>) -> (PathBuf, PathBuf) {
    let root = local_app_data.map_or_else(
        || PathBuf::from("OpenViKey"),
        |base| base.as_ref().join("OpenViKey"),
    );
    (root.join("model.ovk"), root.join("capture.ovk"))
}

/// Clone [`LabSession::save_snapshot`] under the host mutex, then drop the guard.
pub fn snapshot_host(host: &Mutex<TypingHost>) -> SessionSaveSnapshot {
    let guard = host.lock().unwrap_or_else(PoisonError::into_inner);
    guard.session.save_snapshot()
}

/// Serialize the model payload after unlocking the typing host.
pub fn model_payload_from_host(host: &Mutex<TypingHost>) -> Result<Vec<u8>, String> {
    let snap = snapshot_host(host);
    snap.model
        .to_json_payload()
        .map_err(|error| error.to_string())
}

/// Serialize the capture payload after unlocking the typing host (no `capture_log`).
pub fn capture_payload_from_host(host: &Mutex<TypingHost>) -> Result<Vec<u8>, String> {
    let snap = snapshot_host(host);
    let model_payload = snap
        .model
        .to_json_payload()
        .map_err(|error| error.to_string())?;
    let log = CaptureLog {
        header: CaptureHeader {
            v: CAPTURE_VERSION,
            next_seq: snap.cursors.next_seq,
            next_edit_id: snap.cursors.next_edit_id,
            last_at_ms: snap.last_at_ms,
            model_sha256: sha256_hex(&model_payload),
        },
        records: snap.capture_records,
    };
    log.to_payload().map_err(|error| error.to_string())
}

/// Seal model + capture from a [`SessionSaveSnapshot`] (clone-then-serialize; no `capture_log`).
pub fn seal_save_snapshot(
    snap: &SessionSaveSnapshot,
    model_path: &Path,
    capture_path: &Path,
    provider: &PassphraseProvider,
) -> Result<(), PersistError> {
    let model_payload = snap
        .model
        .to_json_payload()
        .map_err(|error| PersistError::Model(error.to_string()))?;
    let model_sha = sha256_hex(&model_payload);
    let log = CaptureLog {
        header: CaptureHeader {
            v: CAPTURE_VERSION,
            next_seq: snap.cursors.next_seq,
            next_edit_id: snap.cursors.next_edit_id,
            last_at_ms: snap.last_at_ms,
            model_sha256: model_sha,
        },
        records: snap.capture_records.clone(),
    };
    let capture_payload = log
        .to_payload()
        .map_err(|error| PersistError::Capture(error.to_string()))?;
    FileModelStore::new(model_path).save(&model_payload, provider)?;
    FileModelStore::new(capture_path).save(&capture_payload, provider)?;
    Ok(())
}
