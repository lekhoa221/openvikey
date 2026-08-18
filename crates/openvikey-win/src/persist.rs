//! Open JSON persistence for the Windows development host.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use crate::host::TypingHost;
use openvikey_core::model::AdaptiveModel;
use openvikey_core::store::StoreError;
use openvikey_session::capture::{
    CAPTURE_VERSION, CaptureHeader, CaptureLog, SessionStoreError, decode_personal_store_pair,
    ensure_distinct_store_paths, sha256_hex,
};
use openvikey_session::persistence::{DebouncedSaver, MODEL_SAVE_DEBOUNCE};
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
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Session(#[from] SessionStoreError),
    #[error("model serialize: {0}")]
    Model(String),
    #[error("capture serialize: {0}")]
    Capture(String),
}

/// Resolve the development host's open JSON stores under `%LOCALAPPDATA%\OpenViKey`.
///
/// These names intentionally differ from the former encrypted `.ovk` stores so
/// changing development mode never overwrites or misreads an existing envelope.
#[must_use]
pub fn default_store_paths(local_app_data: Option<impl AsRef<Path>>) -> (PathBuf, PathBuf) {
    let root = local_app_data.map_or_else(
        || PathBuf::from("OpenViKey"),
        |base| base.as_ref().join("OpenViKey"),
    );
    (
        root.join("model.ovkdev.json"),
        root.join("capture.ovkdev.json"),
    )
}

/// Reject custom CLI names that could escape the personal-data ignore pattern.
pub fn ensure_open_store_cli_path(path: &Path) -> std::io::Result<()> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if name.to_ascii_lowercase().ends_with(".ovkdev.json") {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "open development store paths must end with .ovkdev.json",
        ))
    }
}

type OpenPayloadPair = (Vec<u8>, Vec<u8>);

/// Load a plaintext model/capture pair, trying sibling backups when primaries
/// were interrupted between their independent atomic writes.
pub fn load_open_personal_store(
    model_path: &Path,
    capture_path: &Path,
) -> Result<(AdaptiveModel, CaptureLog), SessionStoreError> {
    match load_open_payload_pair(model_path, capture_path)? {
        Some((model_payload, capture_payload)) => {
            decode_personal_store_pair(&model_payload, &capture_payload)
        }
        None => Ok((AdaptiveModel::default(), default_capture())),
    }
}

fn load_open_payload_pair(
    model_path: &Path,
    capture_path: &Path,
) -> Result<Option<OpenPayloadPair>, SessionStoreError> {
    ensure_open_sidecars_distinct(model_path, capture_path)?;
    let model_backup_path = sibling_with_suffix(model_path, ".bak");
    let capture_backup_path = sibling_with_suffix(capture_path, ".bak");
    let model_temp_path = sibling_with_suffix(model_path, ".tmp");
    let capture_temp_path = sibling_with_suffix(capture_path, ".tmp");
    let model_primary = read_optional(model_path)?;
    let capture_primary = read_optional(capture_path)?;
    let model_backup = read_optional(&model_backup_path)?;
    let capture_backup = read_optional(&capture_backup_path)?;
    let model_temp = read_optional(&model_temp_path)?;
    let capture_temp = read_optional(&capture_temp_path)?;
    let model_candidates = [
        model_primary.as_ref(),
        model_backup.as_ref(),
        model_temp.as_ref(),
    ];
    let capture_candidates = [
        capture_primary.as_ref(),
        capture_backup.as_ref(),
        capture_temp.as_ref(),
    ];
    let pairs = model_candidates
        .into_iter()
        .flat_map(|model| capture_candidates.map(|capture| (model, capture)));
    let mut saw_edit_cursor = false;
    for (model_bytes, capture_bytes) in pairs {
        let (Some(model_bytes), Some(capture_bytes)) = (model_bytes, capture_bytes) else {
            continue;
        };
        match decode_personal_store_pair(model_bytes, capture_bytes) {
            Ok(_) => return Ok(Some((model_bytes.clone(), capture_bytes.clone()))),
            Err(SessionStoreError::EditCursorBehindModel) => saw_edit_cursor = true,
            Err(_) => {}
        }
    }
    let durable_model = model_primary.is_some() || model_backup.is_some();
    let durable_capture = capture_primary.is_some() || capture_backup.is_some();
    match (durable_model, durable_capture) {
        (true, true) if saw_edit_cursor => Err(SessionStoreError::EditCursorBehindModel),
        (true, true) => Err(SessionStoreError::InconsistentStore),
        (true, false) if capture_temp.is_some() => {
            // Model replacement won an interrupted first pair write; the
            // unusable prepared capture proves this is not a legacy orphan.
            Ok(None)
        }
        (true, false) => Err(SessionStoreError::MissingCaptureLog),
        (false, true) if model_temp.is_some() => Ok(None),
        (false, true) => Err(SessionStoreError::OrphanCaptureLog),
        (false, false) => {
            // Incomplete temp-only state has no committed personal data.
            Ok(None)
        }
    }
}

fn ensure_open_sidecars_distinct(
    model_path: &Path,
    capture_path: &Path,
) -> Result<(), SessionStoreError> {
    let paths = [
        model_path.to_path_buf(),
        sibling_with_suffix(model_path, ".tmp"),
        sibling_with_suffix(model_path, ".bak"),
        sibling_with_suffix(model_path, ".bak.tmp"),
        capture_path.to_path_buf(),
        sibling_with_suffix(capture_path, ".tmp"),
        sibling_with_suffix(capture_path, ".bak"),
        sibling_with_suffix(capture_path, ".bak.tmp"),
    ];
    for (index, left) in paths.iter().enumerate() {
        for right in &paths[index + 1..] {
            ensure_distinct_store_paths(left, right)?;
        }
    }
    Ok(())
}

fn default_capture() -> CaptureLog {
    CaptureLog {
        header: CaptureHeader {
            v: CAPTURE_VERSION,
            next_seq: 1,
            next_edit_id: 1,
            last_at_ms: 0,
            model_sha256: String::new(),
        },
        records: Vec::new(),
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, SessionStoreError> {
    if !path.exists() {
        return Ok(None);
    }
    fs::read(path)
        .map(Some)
        .map_err(StoreError::from)
        .map_err(SessionStoreError::from)
}

/// Clone [`openvikey_session::session::LabSession::save_snapshot`] under the
/// host mutex, then drop the guard.
pub fn snapshot_host(host: &Mutex<TypingHost>) -> SessionSaveSnapshot {
    let guard = host.lock().unwrap_or_else(PoisonError::into_inner);
    guard.session.save_snapshot()
}

fn snapshot_payloads(snap: &SessionSaveSnapshot) -> Result<(Vec<u8>, Vec<u8>), PersistError> {
    let model_payload = snap
        .model
        .to_json_payload()
        .map_err(|error| PersistError::Model(error.to_string()))?;
    let log = CaptureLog {
        header: CaptureHeader {
            v: CAPTURE_VERSION,
            next_seq: snap.cursors.next_seq,
            next_edit_id: snap.cursors.next_edit_id,
            last_at_ms: snap.last_at_ms,
            model_sha256: sha256_hex(&model_payload),
        },
        records: snap.capture_records.clone(),
    };
    let capture_payload = log
        .to_payload()
        .map_err(|error| PersistError::Capture(error.to_string()))?;
    Ok((model_payload, capture_payload))
}

/// Deterministic interruption point for pair-recovery tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenPairFailurePoint {
    AfterModelReplace,
}

/// Save a complete snapshot as inspectable JSON without a passphrase.
pub fn save_open_snapshot(
    snap: &SessionSaveSnapshot,
    model_path: &Path,
    capture_path: &Path,
) -> Result<(), PersistError> {
    save_open_snapshot_with_failure(snap, model_path, capture_path, None)
}

/// Save one coherent pair while optionally injecting an interruption for tests.
pub fn save_open_snapshot_with_failure(
    snap: &SessionSaveSnapshot,
    model_path: &Path,
    capture_path: &Path,
    failure: Option<OpenPairFailurePoint>,
) -> Result<(), PersistError> {
    ensure_distinct_store_paths(model_path, capture_path)?;
    let previous = load_open_payload_pair(model_path, capture_path)?;
    let (model_payload, capture_payload) = snapshot_payloads(snap)?;
    let model_temp = prepare_payload(model_path, &model_payload)?;
    let capture_temp = prepare_payload(capture_path, &capture_payload)?;

    if let Some((previous_model, previous_capture)) = previous {
        atomic_replace_payload(&sibling_with_suffix(model_path, ".bak"), &previous_model)?;
        atomic_replace_payload(
            &sibling_with_suffix(capture_path, ".bak"),
            &previous_capture,
        )?;
    }

    replace_prepared(&model_temp, model_path)?;
    if failure == Some(OpenPairFailurePoint::AfterModelReplace) {
        return Err(std::io::Error::other("injected failure after model replace").into());
    }
    replace_prepared(&capture_temp, capture_path)?;
    Ok(())
}

fn atomic_replace_payload(path: &Path, payload: &[u8]) -> std::io::Result<()> {
    let temp = prepare_payload(path, payload)?;
    replace_prepared(&temp, path)
}

fn prepare_payload(path: &Path, payload: &[u8]) -> std::io::Result<PathBuf> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = sibling_with_suffix(path, ".tmp");
    let mut temp = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp_path)?;
    temp.write_all(payload)?;
    temp.flush()?;
    temp.sync_all()?;
    Ok(temp_path)
}

fn replace_prepared(temp: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(temp, target)?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(target)?
        .sync_all()
}

/// Spawn one debounced worker that snapshots and writes the model/capture pair together.
#[must_use]
pub fn spawn_open_pair_saver(
    model_path: PathBuf,
    capture_path: PathBuf,
    host: std::sync::Arc<Mutex<TypingHost>>,
) -> DebouncedSaver {
    DebouncedSaver::spawn_task(MODEL_SAVE_DEBOUNCE, move || {
        let snapshot = snapshot_host(&host);
        save_open_snapshot(&snapshot, &model_path, &capture_path).map_err(|error| error.to_string())
    })
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let name = path
        .file_name()
        .map_or_else(|| "store".into(), std::ffi::OsStr::to_os_string);
    let mut suffixed = name;
    suffixed.push(suffix);
    path.with_file_name(suffixed)
}
