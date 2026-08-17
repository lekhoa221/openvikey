//! Persist load errors and save_snapshot seal path (no capture_log on save).

use std::sync::atomic::Ordering;

use openvikey_core::engine::EngineConfig;
use openvikey_core::lexicon::Lexicon;
use openvikey_core::model::AdaptiveModel;
use openvikey_core::store::ModelStore;
use openvikey_core::store::file::FileModelStore;
use openvikey_core::store::passphrase::PassphraseProvider;
use openvikey_core::store::StoreError;
use openvikey_session::capture::{
    load_personal_store, sha256_hex, CaptureHeader, CaptureLog, CAPTURE_VERSION, SessionStoreError,
};
use openvikey_session::session::LabSession;
use openvikey_win::persist::{default_store_paths, seal_save_snapshot};
use openvikey_win::HostShutdown;

#[test]
fn missing_model_file_loads_defaults() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gd2a-persist-tests")
        .join(format!("{}-missing-model", std::process::id()));
    let model_path = dir.join("model.ovk");
    let log_path = dir.join("capture.ovk");
    let provider = PassphraseProvider::new_for_testing("p2");
    let (model, log) = load_personal_store(&model_path, &log_path, &provider).unwrap();
    assert_eq!(log.header.next_seq, 1);
    assert_eq!(
        model.to_json_payload().unwrap(),
        AdaptiveModel::default().to_json_payload().unwrap()
    );
}

#[test]
fn wrong_passphrase_is_error() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gd2a-persist-tests")
        .join(format!("{}-wrong-pass", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let model_path = dir.join("model.ovk");
    let log_path = dir.join("capture.ovk");
    let good = PassphraseProvider::new_for_testing("correct");
    let bad = PassphraseProvider::new_for_testing("wrong");
    let payload = AdaptiveModel::default().to_json_payload().unwrap();
    let log = CaptureLog {
        header: CaptureHeader {
            v: CAPTURE_VERSION,
            next_seq: 1,
            next_edit_id: 1,
            last_at_ms: 0,
            model_sha256: sha256_hex(&payload),
        },
        records: Vec::new(),
    };
    FileModelStore::new(&model_path)
        .save(&payload, &good)
        .unwrap();
    FileModelStore::new(&log_path)
        .save(&log.to_payload().unwrap(), &good)
        .unwrap();
    let error = load_personal_store(&model_path, &log_path, &bad).unwrap_err();
    assert!(matches!(
        error,
        SessionStoreError::Store(StoreError::WrongPassphrase)
    ));
}

#[test]
fn save_snapshot_seals_matching_sha() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gd2a-persist-tests")
        .join(format!("{}-seal-snap", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let model_path = dir.join("model.ovk");
    let log_path = dir.join("capture.ovk");
    let provider = PassphraseProvider::new_for_testing("seal");
    let session = LabSession::new(EngineConfig::default(), Lexicon::from_entries([], [], None));
    let snap = session.save_snapshot();
    seal_save_snapshot(&snap, &model_path, &log_path, &provider).unwrap();
    let (model, log) = load_personal_store(&model_path, &log_path, &provider).unwrap();
    let payload = model.to_json_payload().unwrap();
    assert_eq!(log.header.model_sha256, sha256_hex(&payload));
    assert_eq!(
        payload,
        snap.model.to_json_payload().unwrap()
    );
}

#[test]
fn default_store_paths_use_openvikey_dir() {
    let (model, capture) = default_store_paths(Some(r"C:\Users\Test\AppData\Local"));
    assert_eq!(
        model,
        std::path::PathBuf::from(r"C:\Users\Test\AppData\Local\OpenViKey\model.ovk")
    );
    assert_eq!(
        capture,
        std::path::PathBuf::from(r"C:\Users\Test\AppData\Local\OpenViKey\capture.ovk")
    );
}

#[test]
fn host_shutdown_sets_flag_consumed_by_fake_loop() {
    let shutdown = HostShutdown::new();
    assert!(!shutdown.is_requested());
    let mut loops = 0u32;
    // Fake message loop: exits when the flag is set (no GetMessage).
    while !shutdown.is_requested() {
        loops += 1;
        if loops == 1 {
            shutdown.run();
        }
        assert!(loops < 5, "fake loop must observe HostShutdown::run");
    }
    assert_eq!(loops, 1);
    assert!(shutdown.is_requested());
    assert!(shutdown.flag().load(Ordering::SeqCst));
}
