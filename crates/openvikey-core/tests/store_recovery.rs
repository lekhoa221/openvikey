//! Milestone 8: restart persistence, atomic-save failure points, and backup recovery.

use openvikey_core::model::{AdaptiveModel, RuleContextKey};
use openvikey_core::store::file::{FailurePoint, FileModelStore};
use openvikey_core::store::passphrase::PassphraseProvider;
use openvikey_core::store::{ModelStore, StoreError};
use openvikey_core::types::{CandidateSource, FeedbackEvent, FeedbackKind, InputMethod};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static PATH_SEQ: AtomicU64 = AtomicU64::new(0);

fn model_path(name: &str) -> PathBuf {
    let seq = PATH_SEQ.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("openvikey-m8-{}-{seq}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    root.join(format!("{name}.ovk"))
}

fn provider() -> PassphraseProvider {
    PassphraseProvider::new_for_testing("restart passphrase")
}

#[test]
fn process_restart_reopens_versioned_adaptive_model() {
    let path = model_path("adaptive-restart");
    let rule = RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Abbreviation,
        original_nfc: "ko".to_string(),
        candidate_nfc: "không".to_string(),
        left_token_nfc: None,
        source_rule_id: "seed:ko".to_string(),
    };
    let mut model = AdaptiveModel::default();
    model.apply_feedback(
        &rule,
        &FeedbackEvent {
            seq: 1,
            at_ms: 10,
            kind: FeedbackKind::Accept { candidate_id: 1 },
        },
        true,
    );
    let mut writer = FileModelStore::new(&path);
    writer
        .save(&model.to_json_payload().unwrap(), &provider())
        .unwrap();

    let bytes = FileModelStore::new(&path).load(&provider()).unwrap();
    assert_eq!(AdaptiveModel::from_json_payload(&bytes).unwrap(), model);
}

#[test]
fn process_restart_reopens_model_with_passphrase() {
    let path = model_path("restart");
    let mut writer = FileModelStore::new(&path);
    writer.save(b"persisted model", &provider()).unwrap();

    let reader = FileModelStore::new(&path);
    assert_eq!(reader.load(&provider()).unwrap(), b"persisted model");
}

#[test]
fn corrupt_primary_recovers_last_valid_backup() {
    let path = model_path("backup");
    let mut store = FileModelStore::new(&path);
    store.save(b"version one", &provider()).unwrap();
    store.save(b"version two", &provider()).unwrap();
    assert_eq!(store.load(&provider()).unwrap(), b"version two");
    std::fs::write(&path, b"corrupt primary").unwrap();

    let reopened = FileModelStore::new(&path);
    assert_eq!(reopened.load(&provider()).unwrap(), b"version one");
}

#[test]
fn injected_save_failures_preserve_previous_model() {
    for failure in [
        FailurePoint::AfterTempSync,
        FailurePoint::AfterBackup,
        FailurePoint::BeforeReplace,
    ] {
        let path = model_path("failure");
        let mut store = FileModelStore::new(&path);
        store.save(b"stable", &provider()).unwrap();
        let err = store
            .save_with_failure(b"interrupted", &provider(), Some(failure))
            .unwrap_err();
        assert!(matches!(err, StoreError::Io(_)));

        let reopened = FileModelStore::new(&path);
        assert_eq!(reopened.load(&provider()).unwrap(), b"stable");
    }
}

#[test]
fn wrong_passphrase_does_not_fall_back_as_if_file_were_corrupt() {
    let path = model_path("wrong-passphrase");
    let mut store = FileModelStore::new(&path);
    store.save(b"private", &provider()).unwrap();
    let wrong = PassphraseProvider::new_for_testing("wrong");
    assert_eq!(store.load(&wrong), Err(StoreError::WrongPassphrase));
}
