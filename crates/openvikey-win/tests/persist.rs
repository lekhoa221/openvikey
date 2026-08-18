//! Open development persistence for the Windows host.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use openvikey_core::engine::EngineConfig;
use openvikey_core::lexicon::Lexicon;
use openvikey_core::model::AdaptiveModel;
use openvikey_session::capture::{CAPTURE_VERSION, CaptureHeader, CaptureLog, SessionStoreError};
use openvikey_session::session::LabSession;
use openvikey_win::HostShutdown;
use openvikey_win::host::TypingHost;
use openvikey_win::persist::{
    OpenPairFailurePoint, default_store_paths, ensure_open_store_cli_path,
    load_open_personal_store, save_open_snapshot, save_open_snapshot_with_failure,
    spawn_open_pair_saver,
};
use openvikey_win::policy::RawKey;

fn test_dir(case: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gd2a-open-persist-tests")
        .join(format!("{}-{case}", std::process::id()))
}

#[test]
fn missing_open_store_loads_defaults_without_a_key() {
    let dir = test_dir("missing");
    let (model, log) =
        load_open_personal_store(&dir.join("model.json"), &dir.join("capture.json")).unwrap();
    assert_eq!(log.header.next_seq, 1);
    assert_eq!(
        model.to_json_payload().unwrap(),
        AdaptiveModel::default().to_json_payload().unwrap()
    );
}

#[test]
fn one_sided_durable_open_store_is_not_silently_reset() {
    let model_only = test_dir("model-only");
    std::fs::create_dir_all(&model_only).unwrap();
    std::fs::write(
        model_only.join("model.json"),
        AdaptiveModel::default().to_json_payload().unwrap(),
    )
    .unwrap();
    let error = load_open_personal_store(
        &model_only.join("model.json"),
        &model_only.join("capture.json"),
    )
    .unwrap_err();
    assert!(matches!(error, SessionStoreError::MissingCaptureLog));

    let capture_only = test_dir("capture-only");
    std::fs::create_dir_all(&capture_only).unwrap();
    let capture = CaptureLog {
        header: CaptureHeader {
            v: CAPTURE_VERSION,
            next_seq: 1,
            next_edit_id: 1,
            last_at_ms: 0,
            model_sha256: String::new(),
        },
        records: Vec::new(),
    };
    std::fs::write(
        capture_only.join("capture.json"),
        capture.to_payload().unwrap(),
    )
    .unwrap();
    let error = load_open_personal_store(
        &capture_only.join("model.json"),
        &capture_only.join("capture.json"),
    )
    .unwrap_err();
    assert!(matches!(error, SessionStoreError::OrphanCaptureLog));
}

#[test]
fn invalid_orphan_temp_does_not_block_first_save_retry() {
    let dir = test_dir("invalid-first-temp");
    std::fs::create_dir_all(&dir).unwrap();
    let model_path = dir.join("model.json");
    let capture_path = dir.join("capture.json");
    std::fs::write(dir.join("model.json.tmp"), b"partial-json").unwrap();

    let (model, log) = load_open_personal_store(&model_path, &capture_path).unwrap();
    assert_eq!(model, AdaptiveModel::default());
    assert_eq!(log.header.next_seq, 1);

    let session = LabSession::new(EngineConfig::default(), Lexicon::from_entries([], [], None));
    save_open_snapshot(&session.save_snapshot(), &model_path, &capture_path).unwrap();
    load_open_personal_store(&model_path, &capture_path).unwrap();
}

#[test]
fn interrupted_first_open_save_recovers_prepared_pair() {
    let dir = test_dir("first-save-interruption");
    std::fs::create_dir_all(&dir).unwrap();
    let model_path = dir.join("model.json");
    let capture_path = dir.join("capture.json");
    let session = LabSession::new(EngineConfig::default(), Lexicon::from_entries([], [], None));
    let snapshot = session.save_snapshot();

    save_open_snapshot_with_failure(
        &snapshot,
        &model_path,
        &capture_path,
        Some(OpenPairFailurePoint::AfterModelReplace),
    )
    .unwrap_err();

    let (model, log) = load_open_personal_store(&model_path, &capture_path).unwrap();
    assert_eq!(model, snapshot.model);
    assert_eq!(log.header.next_seq, snapshot.cursors.next_seq);
    save_open_snapshot(&snapshot, &model_path, &capture_path).unwrap();
    assert!(capture_path.exists());
}

#[test]
fn open_snapshot_is_plaintext_json_and_round_trips_without_a_key() {
    let dir = test_dir("roundtrip");
    std::fs::create_dir_all(&dir).unwrap();
    let model_path = dir.join("model.json");
    let capture_path = dir.join("capture.json");
    let session = LabSession::new(EngineConfig::default(), Lexicon::from_entries([], [], None));
    let snap = session.save_snapshot();

    save_open_snapshot(&snap, &model_path, &capture_path).unwrap();
    save_open_snapshot(&snap, &model_path, &capture_path).unwrap();
    assert!(dir.join("model.json.bak").exists());
    assert!(dir.join("capture.json.bak").exists());

    let model_bytes = std::fs::read(&model_path).unwrap();
    let capture_bytes = std::fs::read(&capture_path).unwrap();
    assert_eq!(model_bytes.first(), Some(&b'{'));
    assert_eq!(capture_bytes.first(), Some(&b'{'));
    serde_json::from_slice::<serde_json::Value>(&model_bytes).unwrap();
    serde_json::from_slice::<serde_json::Value>(&capture_bytes).unwrap();

    let (model, log) = load_open_personal_store(&model_path, &capture_path).unwrap();
    assert_eq!(model.to_json_payload().unwrap(), model_bytes);
    assert_eq!(log.header.next_seq, snap.cursors.next_seq);
}

#[test]
fn default_store_paths_use_open_json_files() {
    let (model, capture) = default_store_paths(Some(r"C:\Users\Test\AppData\Local"));
    assert_eq!(
        model,
        std::path::PathBuf::from(r"C:\Users\Test\AppData\Local\OpenViKey\model.ovkdev.json")
    );
    assert_eq!(
        capture,
        std::path::PathBuf::from(r"C:\Users\Test\AppData\Local\OpenViKey\capture.ovkdev.json")
    );
}

#[test]
fn cli_store_names_are_git_ignorable_and_sidecars_cannot_collide() {
    assert!(ensure_open_store_cli_path(std::path::Path::new("personal.ovkdev.json")).is_ok());
    assert!(ensure_open_store_cli_path(std::path::Path::new("personal-model.json")).is_err());

    let error = load_open_personal_store(
        std::path::Path::new("state.json"),
        std::path::Path::new("state.json.bak"),
    )
    .unwrap_err();
    assert!(matches!(error, SessionStoreError::SameStorePath));
}

#[test]
fn host_shutdown_sets_flag_consumed_by_fake_loop() {
    let shutdown = HostShutdown::new();
    assert!(!shutdown.is_requested());
    let mut loops = 0u32;
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

fn persist_key(vk: u16) -> RawKey {
    RawKey {
        vk,
        down: true,
        control: false,
        shift: false,
        extra_info: 0,
        left_ctrl: false,
        left_shift: false,
    }
}

#[test]
fn notify_and_shutdown_writes_open_stores() {
    let dir = test_dir("notify-shutdown");
    std::fs::create_dir_all(&dir).unwrap();
    let model_path = dir.join("model.json");
    let capture_path = dir.join("capture.json");
    let host = Arc::new(Mutex::new(TypingHost::new_telex_fixture()));
    host.lock().unwrap().handle_key(persist_key(0x58), 1);

    let pair_saver =
        spawn_open_pair_saver(model_path.clone(), capture_path.clone(), Arc::clone(&host));
    pair_saver.notify().unwrap();
    pair_saver.flush().unwrap();

    let (_model, log) = load_open_personal_store(&model_path, &capture_path).unwrap();
    assert!(
        !log.records.is_empty(),
        "dirty session capture must persist"
    );
}

#[test]
fn interrupted_open_pair_recovers_and_retries_without_touching_encrypted_checkpoint() {
    let dir = test_dir("recover-pair");
    std::fs::create_dir_all(&dir).unwrap();
    let model_path = dir.join("model.json");
    let capture_path = dir.join("capture.json");
    let encrypted_checkpoint = dir.join("model.ovk");
    std::fs::write(&encrypted_checkpoint, b"encrypted-checkpoint-sentinel").unwrap();

    let mut host = TypingHost::new_telex_fixture();
    let before = host.session.save_snapshot();
    save_open_snapshot(&before, &model_path, &capture_path).unwrap();
    host.handle_key(persist_key(0x4B), 1);
    host.handle_key(persist_key(0x4F), 2);
    let mut accept = persist_key(0xBE);
    accept.control = true;
    host.handle_key(accept, 3);
    let after = host.session.save_snapshot();
    assert_ne!(after.model, before.model);

    let error = save_open_snapshot_with_failure(
        &after,
        &model_path,
        &capture_path,
        Some(OpenPairFailurePoint::AfterModelReplace),
    )
    .unwrap_err();
    assert!(error.to_string().contains("injected failure"));

    let (recovered_model, recovered_log) =
        load_open_personal_store(&model_path, &capture_path).unwrap();
    assert_eq!(recovered_model, after.model);
    assert_eq!(recovered_log.header.next_seq, after.cursors.next_seq);

    save_open_snapshot(&after, &model_path, &capture_path).unwrap();
    let (retried_model, retried_log) =
        load_open_personal_store(&model_path, &capture_path).unwrap();
    assert_eq!(retried_model, after.model);
    assert_eq!(retried_log.header.next_seq, after.cursors.next_seq);
    assert_eq!(
        std::fs::read(&encrypted_checkpoint).unwrap(),
        b"encrypted-checkpoint-sentinel"
    );
}

#[test]
fn snapshot_helpers_do_not_call_capture_log() {
    let persist = std::fs::read_to_string("src/persist.rs").unwrap();
    let main = std::fs::read_to_string("src/main.rs").unwrap();
    let host = std::fs::read_to_string("src/host.rs").unwrap();
    for (path, src) in [
        ("src/persist.rs", persist.as_str()),
        ("src/main.rs", main.as_str()),
        ("src/host.rs", host.as_str()),
    ] {
        assert!(
            !src.contains("capture_log()"),
            "{path} snapshot/save path must not call capture_log()"
        );
    }
}
