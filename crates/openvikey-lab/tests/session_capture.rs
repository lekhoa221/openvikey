//! Part 2 Wave 5–6: document buffer, implicit capture, persist, auto/undo.

#![allow(clippy::float_cmp)]

use openvikey_core::correction::InterventionConfig;
use openvikey_core::engine::EngineConfig;
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use openvikey_core::model::{AdaptiveModel, ModelView, RuleContextKey};
use openvikey_core::types::{
    CandidateSource, EngineAction, FeedbackEvent, FeedbackKind, InputContext, InputKind,
    InputMethod,
};
use openvikey_lab::capture::{
    CAPTURE_VERSION, CaptureHeader, CaptureLog, SessionStoreError, load_personal_store, replay,
    replay_with_model, sha256_hex,
};
use openvikey_lab::document::{CommittedUnit, DocumentBuffer};
use openvikey_lab::session::{AcceptVisual, LabSession, SessionCursors, SessionSaveSnapshot};
use sha2::{Digest, Sha256};

fn empty_lexicon() -> Lexicon {
    Lexicon::from_entries([], [], Some("session-capture"))
}

fn telex_session() -> LabSession {
    LabSession::new(EngineConfig::default(), empty_lexicon())
}

fn ko_rule() -> RuleContextKey {
    RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Abbreviation,
        original_nfc: "ko".to_string(),
        candidate_nfc: "không".to_string(),
        left_token_nfc: None,
        source_rule_id: "seed:ko".to_string(),
    }
}

fn payload_sha(session: &LabSession) -> String {
    hex::encode(Sha256::digest(session.model_payload().unwrap()))
}

fn commit_ko(session: &mut LabSession) {
    session.inject(
        InputKind::Key {
            logical: 'k',
            physical: None,
        },
        InputContext::default(),
        0,
    );
    session.inject(
        InputKind::Key {
            logical: 'o',
            physical: None,
        },
        InputContext::default(),
        1,
    );
    session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        2,
    );
}

fn pop_committed_ko(session: &mut LabSession) {
    for at_ms in 3..6 {
        session.inject(InputKind::Backspace, InputContext::default(), at_ms);
    }
}

fn commit_khong_via_insert(session: &mut LabSession, at_ms: i64) {
    session.inject(
        InputKind::InsertText {
            text: "không".to_string(),
        },
        InputContext::default(),
        at_ms,
    );
}

#[test]
fn document_pop_removes_delimiter_then_token_and_reports_start_once() {
    let mut document = DocumentBuffer::new();
    document.push_commit(CommittedUnit::new(
        "ko",
        Some(' '),
        "ko",
        None,
        InputMethod::Telex,
        Vec::new(),
    ));
    assert_eq!(document.rendered(), "ko ");

    let first = document.pop_grapheme().unwrap();
    assert!(first.started_deleting.is_none());
    assert_eq!(document.rendered(), "ko");

    let second = document.pop_grapheme().unwrap();
    assert_eq!(
        second
            .started_deleting
            .as_ref()
            .map(|unit| unit.full_token_nfc.as_str()),
        Some("ko")
    );
    assert_eq!(document.rendered(), "k");

    let third = document.pop_grapheme().unwrap();
    assert!(third.started_deleting.is_none());
    assert_eq!(document.rendered(), "");
    assert!(document.pop_grapheme().is_none());
}

#[test]
fn live_commit_then_empty_backspace_clears_document() {
    let mut session = telex_session();
    commit_ko(&mut session);
    assert_eq!(session.document_text(), "ko ");

    session.inject(
        InputKind::Key {
            logical: 'x',
            physical: None,
        },
        InputContext::default(),
        10,
    );
    assert_eq!(session.document_text(), "ko ");
    session.inject(InputKind::Backspace, InputContext::default(), 11);
    assert_eq!(session.document_text(), "ko ");

    pop_committed_ko(&mut session);
    assert_eq!(session.document_text(), "");
}

#[test]
fn implicit_retype_ko_to_khong_adds_mass_on_abbrev_rule() {
    let mut session = telex_session();
    commit_ko(&mut session);
    pop_committed_ko(&mut session);
    commit_khong_via_insert(&mut session, 20);
    assert!(session.model().positive_mass(&ko_rule(), 20) >= 1.0);
}

#[test]
fn implicit_retype_skipped_when_y_is_not_a_snapshot_candidate() {
    let mut session = telex_session();
    commit_ko(&mut session);
    pop_committed_ko(&mut session);
    session.inject(
        InputKind::InsertText {
            text: "xyz".to_string(),
        },
        InputContext::default(),
        20,
    );
    assert_eq!(session.model().positive_mass(&ko_rule(), 20), 0.0);
}

#[test]
fn caret_break_cancels_implicit_mining() {
    let mut session = telex_session();
    commit_ko(&mut session);
    session.inject(InputKind::Backspace, InputContext::default(), 3);
    session.inject(InputKind::Backspace, InputContext::default(), 4);
    session.inject(InputKind::CursorMoved, InputContext::default(), 5);
    session.inject(InputKind::Backspace, InputContext::default(), 6);
    commit_khong_via_insert(&mut session, 20);
    assert_eq!(session.model().positive_mass(&ko_rule(), 20), 0.0);
}

#[test]
fn sensitive_context_skips_candidates_model_and_capture() {
    let mut session = telex_session();
    let before = session.model_payload().unwrap();
    let context = InputContext {
        allow_transform: false,
        allow_learning: false,
    };
    session.inject(
        InputKind::Key {
            logical: 'k',
            physical: None,
        },
        context,
        0,
    );
    session.inject(
        InputKind::Key {
            logical: 'o',
            physical: None,
        },
        context,
        1,
    );
    assert!(session.drain_capture().is_empty());
    assert_eq!(session.model_payload().unwrap(), before);
}

#[test]
fn replay_of_captured_inputs_matches_live_model_hash() {
    let mut live = telex_session();
    commit_ko(&mut live);
    pop_committed_ko(&mut live);
    commit_khong_via_insert(&mut live, 20);
    let log = live.capture_log();
    let replayed = replay(EngineConfig::default(), empty_lexicon(), &log);
    assert_eq!(payload_sha(&live), payload_sha(&replayed));
    assert!(replayed.model().positive_mass(&ko_rule(), 20) >= 1.0);
}

fn vni_config() -> EngineConfig {
    EngineConfig {
        method: InputMethod::Vni,
        tone_placement: openvikey_core::types::TonePlacement::Modern,
    }
}

fn phat_lexicon() -> Lexicon {
    Lexicon::from_entries(
        [LexiconEntry {
            token_nfc: "phát".to_string(),
            frequency: 10,
        }],
        [],
        Some("session-capture-phat"),
    )
}

fn seed_accepts(key: &RuleContextKey, count: u64) -> AdaptiveModel {
    let mut model = AdaptiveModel::default();
    for seq in 1..=count {
        model.apply_feedback(
            key,
            &FeedbackEvent {
                seq,
                at_ms: 0,
                kind: FeedbackKind::Accept { candidate_id: 1 },
            },
            true,
        );
    }
    model
}

fn paht1_rule() -> (RuleContextKey, String) {
    let mut probe = LabSession::new(vni_config(), phat_lexicon());
    let mut last = None;
    for (index, logical) in "paht1".chars().enumerate() {
        last = Some(probe.inject(
            InputKind::Key {
                logical,
                physical: None,
            },
            InputContext::default(),
            i64::try_from(index).unwrap(),
        ));
    }
    let observation = last.unwrap();
    let top = observation.candidates.first().unwrap();
    (
        RuleContextKey {
            input_method: InputMethod::Vni,
            source: top.source,
            original_nfc: observation.snapshot.normalized.clone(),
            candidate_nfc: top.text.clone(),
            left_token_nfc: None,
            source_rule_id: top.evidence.split('+').next().unwrap_or("").to_string(),
        },
        top.text.clone(),
    )
}

fn vni_auto_session() -> (LabSession, RuleContextKey, String) {
    let (rule, replacement) = paht1_rule();
    let session = LabSession::new_with_model(
        vni_config(),
        phat_lexicon(),
        seed_accepts(&rule, 19),
        SessionCursors {
            next_seq: 19,
            next_edit_id: 1,
        },
    );
    (session, rule, replacement)
}

fn type_paht1_commit(
    session: &mut LabSession,
    start_ms: i64,
) -> openvikey_lab::session::SessionObservation {
    for (index, logical) in "paht1".chars().enumerate() {
        session.inject(
            InputKind::Key {
                logical,
                physical: None,
            },
            InputContext::default(),
            start_ms + i64::try_from(index).unwrap(),
        );
    }
    session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        start_ms + 10,
    )
}

#[test]
fn accept_top_writes_expansion_into_document_and_adds_mass() {
    let mut session = telex_session();
    session.inject(
        InputKind::Key {
            logical: 'k',
            physical: None,
        },
        InputContext::default(),
        0,
    );
    session.inject(
        InputKind::Key {
            logical: 'o',
            physical: None,
        },
        InputContext::default(),
        1,
    );
    let visual = session.accept_top(2);
    assert_eq!(
        visual,
        Some(AcceptVisual {
            candidate_nfc: "không".into(),
            was_composing: true,
        })
    );
    assert!(session.document_text().starts_with("không"));
    assert!(session.model().positive_mass(&ko_rule(), 2) >= 1.0);
}

#[test]
fn save_snapshot_does_not_call_to_json() {
    let session = telex_session();
    let snap = session.save_snapshot();
    assert!(matches!(
        snap,
        SessionSaveSnapshot {
            capture_records: _,
            cursors: _,
            last_at_ms: _,
            ..
        }
    ));
    let _ = snap.model.to_json_payload().unwrap();
}

#[test]
fn reject_top_adds_negative_mass_without_changing_document() {
    let mut session = telex_session();
    session.inject(
        InputKind::Key {
            logical: 'k',
            physical: None,
        },
        InputContext::default(),
        0,
    );
    session.inject(
        InputKind::Key {
            logical: 'o',
            physical: None,
        },
        InputContext::default(),
        1,
    );
    let before = session.document_text();
    session.reject_top(2);
    assert_eq!(session.document_text(), before);
    assert!(session.model().negative_mass(&ko_rule(), 2) >= 1.0);
}

#[test]
fn auto_commit_is_undoable_with_stored_snapshot_revision() {
    let (mut session, rule, replacement) = vni_auto_session();
    let last = type_paht1_commit(&mut session, 20);
    assert_eq!(
        last.decision,
        Some(openvikey_core::decision::DecisionState::Auto)
    );
    assert!(
        session.document_text().contains(&replacement),
        "document {}",
        session.document_text()
    );
    let mass_after_auto = session.model().positive_mass(&rule, 30);
    let _ = session.undo_last(31);
    assert!(session.model().negative_mass(&rule, 31) >= 1.5);
    assert!(session.model().positive_mass(&rule, 31) <= mass_after_auto);
}

#[test]
fn auto_settles_after_ten_following_events() {
    let (mut session, rule, _) = vni_auto_session();
    type_paht1_commit(&mut session, 20);
    let before = session.model().positive_mass(&rule, 40);
    for index in 0..9 {
        session.inject(
            InputKind::Key {
                logical: 'a',
                physical: None,
            },
            InputContext::default(),
            40 + index,
        );
    }
    assert!(
        (session.model().positive_mass(&rule, 49) - before).abs() < 0.001,
        "AutoSettled must not fire on the Auto commit itself"
    );
    session.inject(
        InputKind::Key {
            logical: 'a',
            physical: None,
        },
        InputContext::default(),
        4000,
    );
    assert!(session.model().positive_mass(&rule, 4000) >= before + 0.29);
}

#[test]
fn caret_break_cancels_pending_auto_settlement() {
    let (mut session, rule, _) = vni_auto_session();
    type_paht1_commit(&mut session, 20);
    let before = session.model().positive_mass(&rule, 40);
    session.inject(InputKind::CursorMoved, InputContext::default(), 41);
    for index in 0..10 {
        session.inject(
            InputKind::Key {
                logical: 'a',
                physical: None,
            },
            InputContext::default(),
            50 + index,
        );
    }
    assert!((session.model().positive_mass(&rule, 60) - before).abs() < 0.001);
}

#[test]
fn restart_restores_cursor_so_new_feedback_is_not_deduped() {
    let mut first = telex_session();
    commit_ko(&mut first);
    pop_committed_ko(&mut first);
    commit_khong_via_insert(&mut first, 20);
    let mass = first.model().positive_mass(&ko_rule(), 20);
    assert!(mass >= 1.0);

    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/p2-store-tests")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    let model_path = dir.join("model.ovk");
    let log_path = dir.join("capture.ovk");
    let provider = openvikey_core::store::passphrase::PassphraseProvider::new_for_testing("p2");
    let mut model_store = openvikey_core::store::file::FileModelStore::new(&model_path);
    let mut log_store = openvikey_core::store::file::FileModelStore::new(&log_path);
    openvikey_core::store::ModelStore::save(
        &mut model_store,
        &first.model_payload().unwrap(),
        &provider,
    )
    .unwrap();
    openvikey_core::store::ModelStore::save(
        &mut log_store,
        &first.capture_log().to_payload().unwrap(),
        &provider,
    )
    .unwrap();

    let (model, log) = load_personal_store(&model_path, &log_path, &provider).unwrap();
    let mut second = LabSession::new_with_model(
        EngineConfig::default(),
        empty_lexicon(),
        model,
        SessionCursors {
            next_seq: log.header.next_seq,
            next_edit_id: log.header.next_edit_id,
        },
    );
    assert!(second.model().positive_mass(&ko_rule(), 20) >= mass);
    second.inject(
        InputKind::Key {
            logical: 'k',
            physical: None,
        },
        InputContext::default(),
        100,
    );
    second.inject(
        InputKind::Key {
            logical: 'o',
            physical: None,
        },
        InputContext::default(),
        101,
    );
    let before_accept = second.model().positive_mass(&ko_rule(), 102);
    let _ = second.accept_top(102);
    assert!(second.model().positive_mass(&ko_rule(), 102) >= before_accept + 0.9);
}

#[test]
fn model_file_without_capture_log_is_an_error() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/p2-store-tests")
        .join(format!("{}-missing-log", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let model_path = dir.join("model.ovk");
    let log_path = dir.join("capture.ovk");
    let provider = openvikey_core::store::passphrase::PassphraseProvider::new_for_testing("p2");
    let mut model_store = openvikey_core::store::file::FileModelStore::new(&model_path);
    openvikey_core::store::ModelStore::save(
        &mut model_store,
        &AdaptiveModel::default().to_json_payload().unwrap(),
        &provider,
    )
    .unwrap();
    let error = load_personal_store(&model_path, &log_path, &provider).unwrap_err();
    assert!(matches!(error, SessionStoreError::MissingCaptureLog));
}

#[test]
fn replay_includes_accept_top_commands() {
    let mut live = telex_session();
    live.inject(
        InputKind::Key {
            logical: 'k',
            physical: None,
        },
        InputContext::default(),
        0,
    );
    live.inject(
        InputKind::Key {
            logical: 'o',
            physical: None,
        },
        InputContext::default(),
        1,
    );
    let _ = live.accept_top(2);
    let replayed = replay(
        EngineConfig::default(),
        empty_lexicon(),
        &live.capture_log(),
    );
    assert_eq!(payload_sha(&live), payload_sha(&replayed));
}

#[test]
fn undo_last_does_not_replace_a_later_committed_token() {
    let (mut session, rule, replacement) = vni_auto_session();
    type_paht1_commit(&mut session, 20);
    assert!(session.document_text().contains(&replacement));
    session.inject(
        InputKind::Key {
            logical: 'a',
            physical: None,
        },
        InputContext::default(),
        40,
    );
    session.inject(
        InputKind::Key {
            logical: 'b',
            physical: None,
        },
        InputContext::default(),
        41,
    );
    session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        42,
    );
    let before_undo = session.document_text();
    assert!(before_undo.contains("ab"));
    let mass_before = session.model().negative_mass(&rule, 42);
    let _ = session.undo_last(43);
    assert_eq!(session.document_text(), before_undo);
    assert!((session.model().negative_mass(&rule, 43) - mass_before).abs() < 0.001);
}

#[test]
fn insert_text_during_composition_keeps_every_engine_commit() {
    let mut session = telex_session();
    session.inject(
        InputKind::Key {
            logical: 'k',
            physical: None,
        },
        InputContext::default(),
        0,
    );
    session.inject(
        InputKind::InsertText {
            text: "x".to_string(),
        },
        InputContext::default(),
        1,
    );
    assert_eq!(session.document_text(), "kx");
}

#[test]
fn consecutive_delimiter_commits_are_kept() {
    let mut session = telex_session();
    commit_ko(&mut session);
    session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        3,
    );
    assert_eq!(session.document_text(), "ko  ");
}

#[test]
fn extra_delimiter_backspaces_one_space_each() {
    let mut session = telex_session();
    commit_ko(&mut session);
    session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        3,
    );
    assert_eq!(session.document_text(), "ko  ");
    session.inject(InputKind::Backspace, InputContext::default(), 4);
    assert_eq!(session.document_text(), "ko ");
    session.inject(InputKind::Backspace, InputContext::default(), 5);
    assert_eq!(session.document_text(), "ko");
}

#[test]
fn unicode_ellipsis_commits_like_punctuation() {
    let mut session = telex_session();
    session.inject(
        InputKind::Key {
            logical: 'x',
            physical: None,
        },
        InputContext::default(),
        0,
    );
    session.inject(
        InputKind::Key {
            logical: 'i',
            physical: None,
        },
        InputContext::default(),
        1,
    );
    session.inject(
        InputKind::Key {
            logical: 'n',
            physical: None,
        },
        InputContext::default(),
        2,
    );
    session.inject(
        InputKind::Key {
            logical: '…',
            physical: None,
        },
        InputContext::default(),
        3,
    );
    assert_eq!(session.document_text(), "xin…");
    assert!(session.composition_text().is_empty());
}

#[test]
fn replay_includes_undo_last_commands() {
    let (rule, _) = paht1_rule();
    let seed = seed_accepts(&rule, 19);
    let cursors = SessionCursors {
        next_seq: 19,
        next_edit_id: 1,
    };
    let mut live = LabSession::new_with_model(vni_config(), phat_lexicon(), seed.clone(), cursors);
    type_paht1_commit(&mut live, 20);
    let _ = live.undo_last(31);
    let replayed = replay_with_model(
        vni_config(),
        phat_lexicon(),
        seed,
        cursors,
        &live.capture_log(),
    );
    assert_eq!(payload_sha(&live), payload_sha(&replayed));
}

#[test]
fn unsupported_capture_version_is_rejected() {
    let mut log = telex_session().capture_log();
    log.header.v = 999;
    let error = log.validate().unwrap_err();
    assert!(matches!(error, SessionStoreError::UnsupportedVersion));
}

#[test]
fn capture_cursor_behind_records_is_rejected() {
    let mut live = telex_session();
    commit_ko(&mut live);
    let mut log = live.capture_log();
    log.header.next_seq = 1;
    let error = log.validate().unwrap_err();
    assert!(matches!(error, SessionStoreError::CursorBehindLog));
}

#[test]
fn model_and_capture_paths_must_differ() {
    let path = std::path::Path::new("openvikey-model.ovk");
    let error = openvikey_lab::capture::ensure_distinct_store_paths(path, path).unwrap_err();
    assert!(matches!(error, SessionStoreError::SameStorePath));
}

#[test]
fn model_and_capture_path_aliases_are_rejected_before_files_exist() {
    let left = std::path::Path::new("openvikey-model.ovk");
    let right = std::path::Path::new("./openvikey-model.ovk");
    let error = openvikey_lab::capture::ensure_distinct_store_paths(left, right).unwrap_err();
    assert!(matches!(error, SessionStoreError::SameStorePath));
}

#[cfg(windows)]
#[test]
fn model_and_capture_paths_are_case_insensitive_on_windows() {
    let left = std::path::Path::new("OpenViKey-Model.ovk");
    let right = std::path::Path::new("openvikey-model.ovk");
    let error = openvikey_lab::capture::ensure_distinct_store_paths(left, right).unwrap_err();
    assert!(matches!(error, SessionStoreError::SameStorePath));
}

#[test]
fn capture_without_model_is_an_error() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/p2-store-tests")
        .join(format!("{}-orphan-capture", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let model_path = dir.join("model.ovk");
    let log_path = dir.join("capture.ovk");
    let provider = openvikey_core::store::passphrase::PassphraseProvider::new_for_testing("p2");
    let mut first = telex_session();
    commit_ko(&mut first);
    let mut log_store = openvikey_core::store::file::FileModelStore::new(&log_path);
    openvikey_core::store::ModelStore::save(
        &mut log_store,
        &first.capture_log().to_payload().unwrap(),
        &provider,
    )
    .unwrap();
    assert!(!model_path.exists());
    let error = load_personal_store(&model_path, &log_path, &provider).unwrap_err();
    assert!(matches!(error, SessionStoreError::OrphanCaptureLog));
}

#[test]
fn mismatched_model_and_capture_generation_is_rejected() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/p2-store-tests")
        .join(format!("{}-mismatch", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let model_path = dir.join("model.ovk");
    let log_path = dir.join("capture.ovk");
    let provider = openvikey_core::store::passphrase::PassphraseProvider::new_for_testing("p2");
    let mut first = telex_session();
    commit_ko(&mut first);
    let mut model_store = openvikey_core::store::file::FileModelStore::new(&model_path);
    let mut log_store = openvikey_core::store::file::FileModelStore::new(&log_path);
    openvikey_core::store::ModelStore::save(
        &mut model_store,
        &first.model_payload().unwrap(),
        &provider,
    )
    .unwrap();
    openvikey_core::store::ModelStore::save(
        &mut log_store,
        &first.capture_log().to_payload().unwrap(),
        &provider,
    )
    .unwrap();
    let other = AdaptiveModel::default().to_json_payload().unwrap();
    openvikey_core::store::ModelStore::save(&mut model_store, &other, &provider).unwrap();
    let _ = std::fs::remove_file(model_store.backup_path());
    let _ = std::fs::remove_file(log_store.backup_path());
    let error = load_personal_store(&model_path, &log_path, &provider).unwrap_err();
    assert!(matches!(error, SessionStoreError::InconsistentStore));
}

#[test]
fn edit_cursor_behind_model_emissions_is_rejected() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/p2-store-tests")
        .join(format!("{}-edit-cursor", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let model_path = dir.join("model.ovk");
    let log_path = dir.join("capture.ovk");
    let provider = openvikey_core::store::passphrase::PassphraseProvider::new_for_testing("p2");
    let (rule, _) = paht1_rule();
    let mut model = AdaptiveModel::default();
    model.record_auto_emission(&rule, 7, 0, true);
    let payload = model.to_json_payload().unwrap();
    let log = CaptureLog {
        header: CaptureHeader {
            v: CAPTURE_VERSION,
            next_seq: 10,
            next_edit_id: 1,
            last_at_ms: 0,
            model_sha256: sha256_hex(&payload),
        },
        records: Vec::new(),
    };
    let mut model_store = openvikey_core::store::file::FileModelStore::new(&model_path);
    let mut log_store = openvikey_core::store::file::FileModelStore::new(&log_path);
    openvikey_core::store::ModelStore::save(&mut model_store, &payload, &provider).unwrap();
    openvikey_core::store::ModelStore::save(&mut log_store, &log.to_payload().unwrap(), &provider)
        .unwrap();
    let error = load_personal_store(&model_path, &log_path, &provider).unwrap_err();
    assert!(matches!(error, SessionStoreError::EditCursorBehindModel));
}

#[test]
fn mismatched_primaries_recover_matching_backup_pair() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/p2-store-tests")
        .join(format!("{}-bak-pair", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let model_path = dir.join("model.ovk");
    let log_path = dir.join("capture.ovk");
    let provider = openvikey_core::store::passphrase::PassphraseProvider::new_for_testing("p2");
    let mut first = telex_session();
    commit_ko(&mut first);
    let first_payload = first.model_payload().unwrap();
    let first_log = first.capture_log().to_payload().unwrap();
    let mut model_store = openvikey_core::store::file::FileModelStore::new(&model_path);
    let mut log_store = openvikey_core::store::file::FileModelStore::new(&log_path);
    openvikey_core::store::ModelStore::save(&mut model_store, &first_payload, &provider).unwrap();
    openvikey_core::store::ModelStore::save(&mut log_store, &first_log, &provider).unwrap();
    let mut second = telex_session();
    commit_ko(&mut second);
    let _ = second.accept_top(10);
    openvikey_core::store::ModelStore::save(
        &mut log_store,
        &second.capture_log().to_payload().unwrap(),
        &provider,
    )
    .unwrap();
    let (loaded, log) = load_personal_store(&model_path, &log_path, &provider).unwrap();
    assert_eq!(loaded.to_json_payload().unwrap(), first_payload);
    assert_eq!(log.header.model_sha256, sha256_hex(&first_payload));
}

#[test]
fn missing_model_file_loads_defaults() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/p2-store-tests")
        .join(format!("{}-missing-model", std::process::id()));
    let model_path = dir.join("model.ovk");
    let log_path = dir.join("capture.ovk");
    let provider = openvikey_core::store::passphrase::PassphraseProvider::new_for_testing("p2");
    let (model, log) = load_personal_store(&model_path, &log_path, &provider).unwrap();
    assert_eq!(log.header.next_seq, 1);
    assert_eq!(
        model.to_json_payload().unwrap(),
        AdaptiveModel::default().to_json_payload().unwrap()
    );
}

#[test]
fn capture_trim_keeps_the_newest_records() {
    let mut records: Vec<openvikey_lab::capture::CaptureRecord> = (1..=5)
        .map(|seq| openvikey_lab::capture::CaptureRecord::AcceptTop {
            seq,
            at_ms: i64::try_from(seq).unwrap_or(0),
        })
        .collect();
    openvikey_lab::capture::trim_capture_to(&mut records, 3);
    assert_eq!(records.len(), 3);
    let seqs: Vec<u64> = records
        .iter()
        .map(|record| match record {
            openvikey_lab::capture::CaptureRecord::AcceptTop { seq, .. } => *seq,
            _ => 0,
        })
        .collect();
    assert_eq!(seqs, vec![3, 4, 5]);
}

fn type_keys(session: &mut LabSession, text: &str, start_ms: i64) {
    session.type_text(text, InputContext::default(), start_ms);
}

fn space_at(session: &mut LabSession, at_ms: i64) {
    session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        at_ms,
    );
}

fn backspace_n(session: &mut LabSession, count: usize, start_ms: i64) {
    for index in 0..count {
        session.inject(
            InputKind::Backspace,
            InputContext::default(),
            start_ms.saturating_add(i64::try_from(index).unwrap_or(0)),
        );
    }
}

fn chao_lexicon() -> Lexicon {
    Lexicon::from_entries(
        [LexiconEntry {
            token_nfc: "chào".to_string(),
            frequency: 10,
        }],
        [],
        Some("session-chao"),
    )
}

#[test]
fn composition_rewind_first_word_learns_abbrev_from_keys() {
    let mut session = telex_session();
    type_keys(&mut session, "ko", 0);
    backspace_n(&mut session, 2, 2);
    type_keys(&mut session, "khoong", 4);
    assert_eq!(session.composition_text(), "không");
    space_at(&mut session, 20);
    assert!(
        (session.model().positive_mass(&ko_rule(), 20) - 1.5).abs() < 1e-12,
        "mass {}",
        session.model().positive_mass(&ko_rule(), 20)
    );
}

#[test]
fn composition_rewind_two_rewinds_keeps_last_peak_only() {
    let mut session = telex_session();
    type_keys(&mut session, "ko", 0);
    backspace_n(&mut session, 1, 2);
    type_keys(&mut session, "x", 3);
    backspace_n(&mut session, 2, 4);
    type_keys(&mut session, "khoong", 6);
    space_at(&mut session, 20);
    assert_eq!(session.model().positive_mass(&ko_rule(), 20), 0.0);
}

#[test]
fn composition_rewind_caret_break_cancels_learning() {
    let mut session = telex_session();
    type_keys(&mut session, "ko", 0);
    backspace_n(&mut session, 1, 2);
    session.inject(InputKind::CursorMoved, InputContext::default(), 3);
    type_keys(&mut session, "khoong", 4);
    space_at(&mut session, 20);
    assert_eq!(session.model().positive_mass(&ko_rule(), 20), 0.0);
}

#[test]
fn empty_commit_after_rewind_to_empty_does_not_attach_to_next_word() {
    let mut session = telex_session();
    type_keys(&mut session, "ko", 0);
    backspace_n(&mut session, 2, 2);
    space_at(&mut session, 4);
    type_keys(&mut session, "xin", 5);
    space_at(&mut session, 20);
    assert_eq!(session.model().positive_mass(&ko_rule(), 20), 0.0);
}

#[test]
fn personal_pair_promotes_on_second_composition_session() {
    let mut session = telex_session();
    for round in 0..2 {
        let base = i64::from(round) * 40;
        type_keys(&mut session, "aaa", base);
        backspace_n(&mut session, 3, base + 3);
        type_keys(&mut session, "bbb", base + 6);
        space_at(&mut session, base + 20);
    }
    type_keys(&mut session, "aaa", 100);
    assert!(
        session.candidate_texts().iter().any(|text| text == "bbb"),
        "personal overlay missing; got {:?}",
        session.candidate_texts()
    );
}

#[test]
fn telex_fix_policy_auto_on_space_and_undo_restores_raw_keys() {
    let mut session = LabSession::new(vni_config(), chao_lexicon());
    type_keys(&mut session, "ch2ao", 0);
    let last = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    match &last.action {
        Some(EngineAction::ReplaceRange(action)) => {
            assert_eq!(action.replacement, "chào");
        }
        other => panic!("expected TelexFix replace, got {other:?}"),
    }
    session.inject(InputKind::Backspace, InputContext::default(), 11);
    assert_eq!(session.composition_text(), "ch2ao");
    assert!(!session.document_text().contains("chào"));
    let after_mass = session.model().negative_mass(
        &RuleContextKey {
            input_method: InputMethod::Vni,
            source: CandidateSource::TelexFix,
            original_nfc: "ch2ao".into(),
            candidate_nfc: "chào".into(),
            left_token_nfc: None,
            source_rule_id: "vni-fix:move-tone-2".into(),
        },
        12,
    );
    assert!(
        after_mass < 0.1,
        "immediate restore must not apply Undo mass, got {after_mass}"
    );
}

#[test]
fn electron_telex_fix_auto_is_space_only() {
    let mut session = LabSession::new(vni_config(), chao_lexicon());
    session.set_intervention_config(InterventionConfig::electron());
    type_keys(&mut session, "ch2ao", 0);
    let punct = session.inject(
        InputKind::Boundary { delimiter: '.' },
        InputContext::default(),
        10,
    );
    assert!(
        !matches!(punct.action, Some(EngineAction::ReplaceRange(_))),
        "electron punct must not auto, got {:?}",
        punct.action
    );

    let mut spaced = LabSession::new(vni_config(), chao_lexicon());
    spaced.set_intervention_config(InterventionConfig::electron());
    type_keys(&mut spaced, "ch2ao", 0);
    let space = spaced.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    match &space.action {
        Some(EngineAction::ReplaceRange(action)) => {
            assert_eq!(action.replacement, "chào");
        }
        other => panic!("expected electron space auto, got {other:?}"),
    }
}

#[test]
fn forget_last_rule_clears_rewind_mass() {
    let mut session = telex_session();
    type_keys(&mut session, "ko", 0);
    backspace_n(&mut session, 2, 2);
    type_keys(&mut session, "khoong", 4);
    space_at(&mut session, 20);
    assert!(session.model().positive_mass(&ko_rule(), 20) >= 1.5);
    assert!(session.forget_last_rule());
    assert_eq!(session.model().positive_mass(&ko_rule(), 20), 0.0);
}
