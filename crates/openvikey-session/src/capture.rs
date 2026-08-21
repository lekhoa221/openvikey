//! Encrypted-payload capture log and deterministic reducer replay.

use crate::session::{LabSession, SessionCursors};
use openvikey_core::engine::EngineConfig;
use openvikey_core::intervention::CorrectionIdentity;
use openvikey_core::learning_config::LearningConfigV2;
use openvikey_core::lexicon::Lexicon;
use openvikey_core::model::{AdaptiveModel, ModelError};
use openvikey_core::store::file::FileModelStore;
use openvikey_core::store::passphrase::PassphraseProvider;
use openvikey_core::store::{StoreError, envelope};
use openvikey_core::types::{CandidateSource, InputEvent};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

pub const CAPTURE_VERSION: u32 = 2;
const LEGACY_CAPTURE_VERSION: u32 = 1;
/// Newest records kept in RAM and on disk. Older events stay in the model.
pub const MAX_CAPTURE_RECORDS: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureHeader {
    pub v: u32,
    pub next_seq: u64,
    pub next_edit_id: u64,
    #[serde(default)]
    pub last_at_ms: i64,
    #[serde(default)]
    pub model_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CaptureRecord {
    Input {
        event: InputEvent,
    },
    AcceptTop {
        seq: u64,
        at_ms: i64,
    },
    RejectTop {
        seq: u64,
        at_ms: i64,
    },
    UndoLast {
        seq: u64,
        at_ms: i64,
    },
    CandidateSetEvaluated {
        seq: u64,
        at_ms: i64,
        ids: Vec<u64>,
        sources: Vec<CandidateSource>,
        identity_hashes: Vec<String>,
        config_hash: String,
    },
    InterventionApplied {
        seq: u64,
        at_ms: i64,
        edit_id: u64,
        candidate_id: u64,
        reason: String,
    },
    InterventionReverted {
        seq: u64,
        at_ms: i64,
        edit_id: u64,
        revert_kind: String,
    },
    InterventionSettled {
        seq: u64,
        at_ms: i64,
        edit_id: u64,
    },
    CorrectionConfirmed {
        seq: u64,
        at_ms: i64,
        identity: CorrectionIdentity,
        left_token_nfc: Option<String>,
    },
    LanguageCommitSettled {
        seq: u64,
        at_ms: i64,
        token: String,
        left_token: Option<String>,
        transaction_id: u64,
    },
    DataForgotten {
        seq: u64,
        at_ms: i64,
        identity: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureLog {
    pub header: CaptureHeader,
    pub records: Vec<CaptureRecord>,
}

impl CaptureLog {
    pub fn to_payload(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn from_payload(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    pub fn validate(&self) -> Result<(), SessionStoreError> {
        if !matches!(self.header.v, LEGACY_CAPTURE_VERSION | CAPTURE_VERSION) {
            return Err(SessionStoreError::UnsupportedVersion);
        }
        if self.header.v == LEGACY_CAPTURE_VERSION && self.records.iter().any(is_v2_record) {
            return Err(SessionStoreError::UnsupportedVersion);
        }
        if !self.header.model_sha256.is_empty() && !is_sha256_hex(&self.header.model_sha256) {
            return Err(SessionStoreError::InvalidCaptureRecord(
                "model_sha256 must be an empty string or a lowercase SHA-256",
            ));
        }
        for record in &self.records {
            validate_record(record)?;
        }
        let max_seq = self.records.iter().map(record_seq).max();
        if let Some(max_seq) = max_seq
            && self.header.next_seq <= max_seq
        {
            return Err(SessionStoreError::CursorBehindLog);
        }
        let max_edit_id = self.records.iter().filter_map(record_edit_id).max();
        if let Some(max_edit_id) = max_edit_id
            && self.header.next_edit_id <= max_edit_id
        {
            return Err(SessionStoreError::EditCursorBehindLog);
        }
        Ok(())
    }

    fn migrate_legacy_checkpoint(&mut self) {
        if self.header.v != LEGACY_CAPTURE_VERSION {
            return;
        }
        // The paired model is the authoritative snapshot. V1 records lack the
        // semantic identities required for selective compaction, so start a
        // clean bounded v2 journal without changing either cursor.
        self.records.clear();
        self.header.v = CAPTURE_VERSION;
    }
}

fn is_v2_record(record: &CaptureRecord) -> bool {
    !matches!(
        record,
        CaptureRecord::Input { .. }
            | CaptureRecord::AcceptTop { .. }
            | CaptureRecord::RejectTop { .. }
            | CaptureRecord::UndoLast { .. }
    )
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn validate_record(record: &CaptureRecord) -> Result<(), SessionStoreError> {
    match record {
        CaptureRecord::CandidateSetEvaluated {
            ids,
            sources,
            identity_hashes,
            config_hash,
            ..
        } => {
            if ids.len() != sources.len() || ids.len() != identity_hashes.len() {
                return Err(SessionStoreError::InvalidCaptureRecord(
                    "candidate ids, sources, and identity hashes must be parallel",
                ));
            }
            if !is_sha256_hex(config_hash)
                || identity_hashes.iter().any(|hash| !is_sha256_hex(hash))
            {
                return Err(SessionStoreError::InvalidCaptureRecord(
                    "candidate and config identities must be lowercase SHA-256 values",
                ));
            }
        }
        CaptureRecord::DataForgotten { identity, .. } if !is_sha256_hex(identity) => {
            return Err(SessionStoreError::InvalidCaptureRecord(
                "forgotten identity must be a lowercase SHA-256",
            ));
        }
        _ => {}
    }
    Ok(())
}

fn record_seq(record: &CaptureRecord) -> u64 {
    match record {
        CaptureRecord::Input { event } => event.seq,
        CaptureRecord::AcceptTop { seq, .. }
        | CaptureRecord::RejectTop { seq, .. }
        | CaptureRecord::UndoLast { seq, .. }
        | CaptureRecord::CandidateSetEvaluated { seq, .. }
        | CaptureRecord::InterventionApplied { seq, .. }
        | CaptureRecord::InterventionReverted { seq, .. }
        | CaptureRecord::InterventionSettled { seq, .. }
        | CaptureRecord::CorrectionConfirmed { seq, .. }
        | CaptureRecord::LanguageCommitSettled { seq, .. }
        | CaptureRecord::DataForgotten { seq, .. } => *seq,
    }
}

fn record_edit_id(record: &CaptureRecord) -> Option<u64> {
    match record {
        CaptureRecord::InterventionApplied { edit_id, .. }
        | CaptureRecord::InterventionReverted { edit_id, .. }
        | CaptureRecord::InterventionSettled { edit_id, .. } => Some(*edit_id),
        _ => None,
    }
}

pub fn trim_capture_to(records: &mut Vec<CaptureRecord>, max: usize) {
    if max == 0 {
        records.clear();
        return;
    }
    if records.len() > max {
        let excess = records.len() - max;
        records.drain(..excess);
    }
}

/// Stable text-free identity used to selectively compact capture-v2 records.
#[must_use]
pub fn correction_identity_hash(identity: &CorrectionIdentity) -> String {
    let mut hash = Sha256::new();
    hash.update([match identity.input_method {
        openvikey_core::types::InputMethod::Telex => 0,
        openvikey_core::types::InputMethod::Vni => 1,
    }]);
    hash.update([match identity.source {
        CandidateSource::TelexFix => 0,
        CandidateSource::Fuzzy => 1,
        CandidateSource::Abbreviation => 2,
        CandidateSource::Diacritics => 3,
        CandidateSource::Personal => 4,
    }]);
    for field in [
        identity.original_nfc.as_bytes(),
        identity.candidate_nfc.as_bytes(),
        identity.source_rule_id.as_bytes(),
    ] {
        hash.update(u64::try_from(field.len()).unwrap_or(u64::MAX).to_le_bytes());
        hash.update(field);
    }
    hex::encode(hash.finalize())
}

/// Removes only records that can reconstruct one forgotten identity.
///
/// A v1 journal has no identity metadata, so it still falls back to clearing the
/// whole bounded journal. This is the intentional fail-closed v1 deviation.
pub fn compact_capture_after_forget(
    records: &mut Vec<CaptureRecord>,
    forgotten: &CorrectionIdentity,
) {
    let has_v2_identity_metadata = records.iter().any(|record| {
        matches!(
            record,
            CaptureRecord::CandidateSetEvaluated { .. } | CaptureRecord::CorrectionConfirmed { .. }
        )
    });
    if !has_v2_identity_metadata {
        records.clear();
        return;
    }

    let forgotten_hash = correction_identity_hash(forgotten);
    let mut related_seqs = records
        .iter()
        .filter(|record| record_matches_identity(record, forgotten, &forgotten_hash))
        .map(record_seq)
        .collect::<BTreeSet<_>>();
    let related_edit_ids = records
        .iter()
        .filter_map(|record| match record {
            CaptureRecord::InterventionApplied { seq, edit_id, .. }
                if related_seqs.contains(seq) =>
            {
                Some(*edit_id)
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    related_seqs.extend(records.iter().filter_map(|record| match record {
        CaptureRecord::InterventionReverted { seq, edit_id, .. }
        | CaptureRecord::InterventionSettled { seq, edit_id, .. }
            if related_edit_ids.contains(edit_id) =>
        {
            Some(*seq)
        }
        _ => None,
    }));
    records.retain(|record| {
        let replay_command = matches!(
            record,
            CaptureRecord::Input { .. }
                | CaptureRecord::AcceptTop { .. }
                | CaptureRecord::RejectTop { .. }
                | CaptureRecord::UndoLast { .. }
        );
        (replay_command || !related_seqs.contains(&record_seq(record)))
            && !record_matches_identity(record, forgotten, &forgotten_hash)
            && !matches!(
                record,
                CaptureRecord::DataForgotten { identity, .. } if identity == &forgotten_hash
            )
    });
}

fn record_matches_identity(
    record: &CaptureRecord,
    identity: &CorrectionIdentity,
    identity_hash: &str,
) -> bool {
    match record {
        CaptureRecord::CandidateSetEvaluated {
            identity_hashes, ..
        } => identity_hashes.iter().any(|hash| hash == identity_hash),
        CaptureRecord::CorrectionConfirmed {
            identity: recorded, ..
        } => recorded == identity,
        _ => false,
    }
}

pub fn ensure_distinct_store_paths(
    model_path: &Path,
    capture_path: &Path,
) -> Result<(), SessionStoreError> {
    if store_paths_equal(model_path, capture_path) {
        return Err(SessionStoreError::SameStorePath);
    }
    Ok(())
}

fn store_paths_equal(left: &Path, right: &Path) -> bool {
    match (normalize_store_path(left), normalize_store_path(right)) {
        (Some(left), Some(right)) => platform_path_eq(&left, &right),
        _ => left == right,
    }
}

fn normalize_store_path(path: &Path) -> Option<PathBuf> {
    let absolute = std::path::absolute(path).ok()?;
    Some(lexical_normalize(&absolute))
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(components.last(), Some(Component::Normal(_))) {
                    components.pop();
                } else {
                    components.push(component);
                }
            }
            other => components.push(other),
        }
    }
    components.iter().collect()
}

fn platform_path_eq(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    #[cfg(windows)]
    {
        left.as_os_str().eq_ignore_ascii_case(right.as_os_str())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Replays command records through a fresh reducer. Capture is disabled so the
/// log is not duplicated. V2 replay fails closed when its recorded learning
/// configuration is unavailable rather than re-deciding history under a new
/// policy. AutoSettled is regenerated by the matching live path.
pub fn replay(
    engine_config: EngineConfig,
    lexicon: Lexicon,
    log: &CaptureLog,
) -> Result<LabSession, SessionStoreError> {
    replay_with_model(
        engine_config,
        lexicon,
        AdaptiveModel::default(),
        SessionCursors::default(),
        log,
    )
}

pub fn replay_with_model(
    engine_config: EngineConfig,
    lexicon: Lexicon,
    model: AdaptiveModel,
    cursors: SessionCursors,
    log: &CaptureLog,
) -> Result<LabSession, SessionStoreError> {
    log.validate()?;
    validate_replay_config(log)?;
    let mut session = LabSession::new_with_model(engine_config, lexicon, model, cursors);
    session.set_capturing(false);
    for record in &log.records {
        match record {
            CaptureRecord::Input { event } => {
                session.process_event(event);
            }
            CaptureRecord::AcceptTop { at_ms, .. } => {
                let _ = session.accept_top(*at_ms);
            }
            CaptureRecord::RejectTop { at_ms, .. } => {
                session.reject_top(*at_ms);
            }
            CaptureRecord::UndoLast { at_ms, .. } => {
                let _ = session.undo_last(*at_ms);
            }
            CaptureRecord::CandidateSetEvaluated { .. }
            | CaptureRecord::InterventionApplied { .. }
            | CaptureRecord::InterventionReverted { .. }
            | CaptureRecord::InterventionSettled { .. }
            | CaptureRecord::CorrectionConfirmed { .. }
            | CaptureRecord::LanguageCommitSettled { .. } => {}
            CaptureRecord::DataForgotten { identity, .. } => {
                session.forget_identity_hash_for_replay(identity);
            }
        }
    }
    Ok(session)
}

fn validate_replay_config(log: &CaptureLog) -> Result<(), SessionStoreError> {
    if log.header.v == LEGACY_CAPTURE_VERSION {
        return Ok(());
    }
    let available = LearningConfigV2::compatibility_v1().hash();
    if let Some(captured) = log.records.iter().find_map(|record| match record {
        CaptureRecord::CandidateSetEvaluated { config_hash, .. } if config_hash != &available => {
            Some(config_hash.clone())
        }
        _ => None,
    }) {
        return Err(SessionStoreError::ReplayConfigUnavailable {
            captured,
            available,
        });
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum SessionStoreError {
    #[error("personal model is missing its capture log")]
    MissingCaptureLog,
    #[error("capture log exists but the personal model is missing")]
    OrphanCaptureLog,
    #[error("unsupported capture log version")]
    UnsupportedVersion,
    #[error("capture cursor is behind recorded sequence numbers")]
    CursorBehindLog,
    #[error("capture edit cursor is behind model auto-emission ids")]
    EditCursorBehindModel,
    #[error("capture edit cursor is behind recorded intervention ids")]
    EditCursorBehindLog,
    #[error("invalid capture record: {0}")]
    InvalidCaptureRecord(&'static str),
    #[error(
        "capture requires unavailable learning config {captured}; current replay config is {available}"
    )]
    ReplayConfigUnavailable { captured: String, available: String },
    #[error("model and capture paths must be distinct")]
    SameStorePath,
    #[error("personal model does not match its capture log")]
    InconsistentStore,
    #[error("invalid capture log payload: {0}")]
    CapturePayload(String),
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Load encrypted model + capture log.
///
/// Both files missing → empty defaults. Capture without a model (including `.bak`)
/// is an error so the next save cannot overwrite an orphan log. If primaries
/// disagree, try `.bak` pairings for a hash-consistent pair.
pub fn load_personal_store(
    model_path: &Path,
    log_path: &Path,
    provider: &PassphraseProvider,
) -> Result<(AdaptiveModel, CaptureLog), SessionStoreError> {
    let model_store = FileModelStore::new(model_path);
    let log_store = FileModelStore::new(log_path);
    let model_backup_path = model_store.backup_path();
    let log_backup_path = log_store.backup_path();
    let model_present = model_path.exists() || model_backup_path.exists();
    let log_present = log_path.exists() || log_backup_path.exists();
    if !model_present && !log_present {
        return Ok((
            AdaptiveModel::default(),
            CaptureLog {
                header: CaptureHeader {
                    v: CAPTURE_VERSION,
                    next_seq: 1,
                    next_edit_id: 1,
                    last_at_ms: 0,
                    model_sha256: String::new(),
                },
                records: Vec::new(),
            },
        ));
    }
    if !model_present {
        return Err(SessionStoreError::OrphanCaptureLog);
    }
    if !log_present {
        return Err(SessionStoreError::MissingCaptureLog);
    }
    let model_primary = decrypt_store_file(model_path, provider)?;
    let log_primary = decrypt_store_file(log_path, provider)?;
    let model_backup = decrypt_store_file(&model_backup_path, provider)?;
    let log_backup = decrypt_store_file(&log_backup_path, provider)?;
    let pairs = [
        (model_primary.as_ref(), log_primary.as_ref()),
        (model_primary.as_ref(), log_backup.as_ref()),
        (model_backup.as_ref(), log_primary.as_ref()),
        (model_backup.as_ref(), log_backup.as_ref()),
    ];
    let mut saw_edit_cursor = false;
    for (model_bytes, log_bytes) in pairs {
        let (Some(model_bytes), Some(log_bytes)) = (model_bytes, log_bytes) else {
            continue;
        };
        match decode_personal_store_pair(model_bytes, log_bytes) {
            Ok(loaded) => return Ok(loaded),
            Err(SessionStoreError::EditCursorBehindModel) => saw_edit_cursor = true,
            Err(
                SessionStoreError::InconsistentStore
                | SessionStoreError::UnsupportedVersion
                | SessionStoreError::CursorBehindLog
                | SessionStoreError::EditCursorBehindLog
                | SessionStoreError::InvalidCaptureRecord(_)
                | SessionStoreError::ReplayConfigUnavailable { .. }
                | SessionStoreError::CapturePayload(_)
                | SessionStoreError::Model(_),
            ) => {}
            Err(error) => return Err(error),
        }
    }
    if saw_edit_cursor {
        Err(SessionStoreError::EditCursorBehindModel)
    } else {
        Err(SessionStoreError::InconsistentStore)
    }
}

fn decrypt_store_file(
    path: &Path,
    provider: &PassphraseProvider,
) -> Result<Option<Vec<u8>>, SessionStoreError> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(StoreError::from)?;
    match envelope::open(&bytes, provider) {
        Ok(payload) => Ok(Some(payload)),
        Err(StoreError::WrongPassphrase) => Err(StoreError::WrongPassphrase.into()),
        Err(_) => Ok(None),
    }
}

/// Decode and validate one model/capture payload pair, independent of its storage envelope.
pub fn decode_personal_store_pair(
    model_bytes: &[u8],
    log_bytes: &[u8],
) -> Result<(AdaptiveModel, CaptureLog), SessionStoreError> {
    let model = AdaptiveModel::from_json_payload(model_bytes)?;
    let mut log = CaptureLog::from_payload(log_bytes)
        .map_err(|error| SessionStoreError::CapturePayload(error.to_string()))?;
    log.validate()?;
    if !log.header.model_sha256.is_empty() && log.header.model_sha256 != sha256_hex(model_bytes) {
        return Err(SessionStoreError::InconsistentStore);
    }
    let max_edit_id = model.max_recorded_edit_id();
    if max_edit_id > 0 && log.header.next_edit_id <= max_edit_id {
        return Err(SessionStoreError::EditCursorBehindModel);
    }
    log.migrate_legacy_checkpoint();
    Ok((model, log))
}
