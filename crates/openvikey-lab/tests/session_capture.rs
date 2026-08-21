//! Part 2 Wave 5–6: document buffer, implicit capture, persist, auto/undo.

#![allow(clippy::float_cmp)]

use openvikey_core::correction::InterventionConfig;
use openvikey_core::engine::EngineConfig;
use openvikey_core::intervention::{CorrectionIdentity, InterventionReason};
use openvikey_core::learning_config::LearningConfigV2;
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use openvikey_core::model::{AdaptiveModel, ModelView, RuleContextKey};
use openvikey_core::types::{
    CandidateSource, EngineAction, FeedbackEvent, FeedbackKind, InputContext, InputKind,
    InputMethod,
};
use openvikey_lab::capture::{
    CAPTURE_VERSION, CaptureHeader, CaptureLog, CaptureRecord, SessionStoreError,
    compact_capture_after_forget, correction_identity_hash, decode_personal_store_pair,
    load_personal_store, replay, replay_with_model, sha256_hex,
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

fn compatibility_session(engine_config: EngineConfig, lexicon: Lexicon) -> LabSession {
    LabSession::new_with_learning_config(
        engine_config,
        lexicon,
        LearningConfigV2::compatibility_v1(),
    )
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
        Some("ko".into()),
        None,
        InputMethod::Telex,
        Vec::new(),
    ));
    assert_eq!(document.rendered(), "ko ");

    let first = document.pop_grapheme().unwrap();
    assert!(first.started_deleting.is_none());
    assert_eq!(first.removed_delimiter, Some(' '));
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
    assert_eq!(log.header.v, 2);
    let payload = log.to_payload().unwrap();
    let restored_log = CaptureLog::from_payload(&payload).unwrap();
    let replayed = replay(EngineConfig::default(), empty_lexicon(), &restored_log).unwrap();
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
fn displayed_suggestion_records_impression_and_accept_records_selection() {
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

    let shown = session
        .model()
        .inspection_rows()
        .into_iter()
        .find(|row| row.original_nfc == "ko")
        .expect("visible suggestion records an inspection row");
    assert_eq!(shown.shown_count, 1);
    assert_eq!(shown.selected_count, 0);
    assert!(shown.negative_evidence.abs() < f64::EPSILON);

    session.accept_top(2);
    let selected = session
        .model()
        .inspection_rows()
        .into_iter()
        .find(|row| row.original_nfc == "ko")
        .unwrap();
    assert_eq!(selected.shown_count, 1);
    assert_eq!(selected.selected_count, 1);
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
    assert_eq!(session.model().unigram_count("không"), 1);
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
    assert!(session.undo_last(31).is_some());
    assert!(session.undo_last(32).is_none());
    assert_eq!(session.model().evidence_totals(&rule).1, 1.5);
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
fn committed_user_token_updates_unigram_once() {
    let mut session = telex_session();

    session.type_text("nam ", InputContext::default(), 0);

    assert_eq!(session.model().unigram_count("nam"), 1);
    assert!(session.capture_log().records.iter().any(|record| matches!(
        record,
        CaptureRecord::LanguageCommitSettled { token, .. } if token == "nam"
    )));
}

#[test]
fn consecutive_user_commits_update_one_left_bigram() {
    let mut session = telex_session();

    session.type_text("viet nam ", InputContext::default(), 0);

    assert_eq!(session.model().bigram_count("viet", "nam"), 1);
}

#[test]
fn private_mode_commit_does_not_update_unigram() {
    let mut session = telex_session();
    let private = InputContext {
        allow_transform: true,
        allow_learning: false,
    };

    session.type_text("nam ", private, 0);

    assert_eq!(session.model().unigram_count("nam"), 0);
}

#[test]
fn auto_replacement_updates_unigram_only_after_settlement() {
    let (mut session, _, replacement) = vni_auto_session();
    type_paht1_commit(&mut session, 20);
    assert_eq!(session.model().unigram_count(&replacement), 0);

    for index in 0..10 {
        session.inject(
            InputKind::Key {
                logical: 'a',
                physical: None,
            },
            InputContext::default(),
            if index == 9 { 4_000 } else { 40 + index },
        );
    }

    assert_eq!(session.model().unigram_count(&replacement), 1);
    assert!(session.capture_log().records.iter().any(|record| matches!(
        record,
        CaptureRecord::LanguageCommitSettled { token, .. } if token == &replacement
    )));
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
    )
    .unwrap();
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
    )
    .unwrap();
    assert_eq!(payload_sha(&live), payload_sha(&replayed));
}

#[test]
fn capture_v2_records_evaluated_and_applied_interventions() {
    let mut session = compatibility_session(EngineConfig::default(), khong_lexicon());
    commit_ko(&mut session);
    let log = session.capture_log();

    assert_eq!(log.header.v, 2);
    assert!(log.records.iter().any(|record| matches!(
        record,
        CaptureRecord::CandidateSetEvaluated { config_hash, .. } if config_hash.len() == 64
    )));
    assert!(
        log.records
            .iter()
            .any(|record| matches!(record, CaptureRecord::InterventionApplied { .. }))
    );
}

#[test]
fn v1_capture_still_loads_and_unknown_future_kind_is_rejected() {
    let payload = br#"{
        "header":{"v":1,"next_seq":2,"next_edit_id":1},
        "records":[{"kind":"accept_top","seq":1,"at_ms":0}]
    }"#;
    let log = CaptureLog::from_payload(payload).unwrap();
    assert!(log.validate().is_ok());
    assert!(CaptureLog::from_payload(
        br#"{"header":{"v":2,"next_seq":2,"next_edit_id":1},"records":[{"kind":"future_kind","seq":1,"at_ms":0}]}"#
    )
    .is_err());
}

#[test]
fn v1_store_pair_migrates_to_a_clean_v2_checkpoint() {
    let model_payload = AdaptiveModel::default().to_json_payload().unwrap();
    let log = CaptureLog::from_payload(
        format!(
            r#"{{"header":{{"v":1,"next_seq":2,"next_edit_id":1,"model_sha256":"{}"}},"records":[{{"kind":"accept_top","seq":1,"at_ms":0}}]}}"#,
            sha256_hex(&model_payload)
        )
        .as_bytes(),
    )
    .unwrap();

    let (_, migrated) =
        decode_personal_store_pair(&model_payload, &log.to_payload().unwrap()).unwrap();

    assert_eq!(migrated.header.v, CAPTURE_VERSION);
    assert_eq!(migrated.header.next_seq, 2);
    assert!(migrated.records.is_empty());
}

#[test]
fn v1_header_rejects_v2_records() {
    let mut session = LabSession::new(EngineConfig::default(), khong_lexicon());
    commit_ko(&mut session);
    let mut log = session.capture_log();
    log.header.v = 1;

    let error = log.validate().unwrap_err();

    assert!(matches!(error, SessionStoreError::UnsupportedVersion));
}

#[test]
fn malformed_v2_candidate_record_is_rejected() {
    let mut log = telex_session().capture_log();
    log.records.push(CaptureRecord::CandidateSetEvaluated {
        seq: log.header.next_seq,
        at_ms: 0,
        ids: vec![1],
        sources: Vec::new(),
        identity_hashes: vec!["0".repeat(64)],
        config_hash: "0".repeat(64),
    });
    log.header.next_seq = log.header.next_seq.saturating_add(1);

    let error = log.validate().unwrap_err();

    assert!(matches!(error, SessionStoreError::InvalidCaptureRecord(_)));
}

#[test]
fn v2_replay_fails_closed_when_recorded_config_is_unavailable() {
    let mut session = LabSession::new(EngineConfig::default(), khong_lexicon());
    commit_ko(&mut session);
    let mut log = session.capture_log();
    for record in &mut log.records {
        if let CaptureRecord::CandidateSetEvaluated { config_hash, .. } = record {
            *config_hash = "0".repeat(64);
        }
    }

    assert!(matches!(
        replay(EngineConfig::default(), empty_lexicon(), &log),
        Err(SessionStoreError::ReplayConfigUnavailable { .. })
    ));
}

#[test]
fn v2_replay_fails_closed_when_capture_mixes_config_hashes() {
    let mut session = LabSession::new(EngineConfig::default(), khong_lexicon());
    commit_ko(&mut session);
    let mut log = session.capture_log();
    let mut candidate_records = log.records.iter_mut().filter_map(|record| match record {
        CaptureRecord::CandidateSetEvaluated { config_hash, .. } => Some(config_hash),
        _ => None,
    });
    *candidate_records.next().expect("first candidate record") =
        LearningConfigV2::compatibility_v1().hash();
    assert!(
        candidate_records.next().is_some(),
        "mixed-hash fixture needs two records"
    );

    assert!(matches!(
        replay(EngineConfig::default(), empty_lexicon(), &log),
        Err(SessionStoreError::ReplayConfigUnavailable { .. })
    ));
}

#[test]
fn identity_free_v1_records_use_fail_closed_full_compaction() {
    let mut records = vec![
        CaptureRecord::AcceptTop { seq: 1, at_ms: 0 },
        CaptureRecord::UndoLast { seq: 2, at_ms: 1 },
    ];
    let forgotten = CorrectionIdentity {
        input_method: InputMethod::Telex,
        source: CandidateSource::Abbreviation,
        original_nfc: "ko".into(),
        candidate_nfc: "không".into(),
        source_rule_id: "seed:ko".into(),
    };

    compact_capture_after_forget(&mut records, &forgotten);

    assert!(records.is_empty());
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

#[test]
fn capture_trim_drops_a_whole_oldest_seq_transaction() {
    let identity = CorrectionIdentity {
        input_method: InputMethod::Telex,
        source: CandidateSource::Abbreviation,
        original_nfc: "ko".into(),
        candidate_nfc: "không".into(),
        source_rule_id: "seed:ko".into(),
    };
    let mut records = vec![
        CaptureRecord::AcceptTop { seq: 1, at_ms: 1 },
        CaptureRecord::CorrectionConfirmed {
            seq: 1,
            at_ms: 1,
            identity,
            left_token_nfc: None,
        },
        CaptureRecord::AcceptTop { seq: 2, at_ms: 2 },
    ];

    openvikey_lab::capture::trim_capture_to(&mut records, 2);

    assert_eq!(records, vec![CaptureRecord::AcceptTop { seq: 2, at_ms: 2 }]);
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

fn khong_lexicon() -> Lexicon {
    Lexicon::from_entries(
        [LexiconEntry {
            token_nfc: "không".to_string(),
            frequency: 100,
        }],
        [],
        Some("session-khong"),
    )
}

fn khogn_rule() -> RuleContextKey {
    RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Fuzzy,
        original_nfc: "khogn".into(),
        candidate_nfc: "không".into(),
        left_token_nfc: None,
        source_rule_id: "fuzzy:weighted:khogn->không".into(),
    }
}

fn nen_lexicon() -> Lexicon {
    Lexicon::from_entries(
        [
            LexiconEntry {
                token_nfc: "nên".to_string(),
                frequency: 50,
            },
            LexiconEntry {
                token_nfc: "không".to_string(),
                frequency: 100,
            },
        ],
        [],
        Some("session-nen"),
    )
}

#[test]
fn product_v2_abbrev_ko_space_does_not_replace_without_evidence() {
    let mut session = LabSession::new_with_learning_config(
        EngineConfig::default(),
        khong_lexicon(),
        LearningConfigV2::product_v2(),
    );
    type_keys(&mut session, "ko", 0);
    let last = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    assert!(
        !matches!(last.action, Some(EngineAction::ReplaceRange(_))),
        "product policy must keep cold abbreviation as Suggest, got {:?}",
        last.action
    );
    assert!(matches!(
        last.action,
        Some(EngineAction::ShowSuggestions { .. })
    ));
    assert!(session.take_learning_notice().is_none());
    assert_eq!(session.model().positive_mass(&ko_rule(), 10), 0.0);
    assert!(session.document_text().contains("ko"));
    assert!(!session.document_text().contains("không"));
}

#[test]
fn two_abbrev_assist_undos_stop_further_space_auto() {
    let mut session = compatibility_session(EngineConfig::default(), khong_lexicon());
    for (typed_at, space_ms, undo_ms, bypass_ms) in [(0, 10, 11, 12), (20, 30, 31, 32)] {
        type_keys(&mut session, "ko", typed_at);
        let replaced = session.inject(
            InputKind::Boundary { delimiter: ' ' },
            InputContext::default(),
            space_ms,
        );
        assert!(
            matches!(replaced.action, Some(EngineAction::ReplaceRange(_))),
            "expected assist replace before two undos, got {:?}",
            replaced.action
        );
        session.inject(InputKind::Backspace, InputContext::default(), undo_ms);
        assert_eq!(session.composition_text(), "ko");
        let bypassed = session.inject(
            InputKind::Boundary { delimiter: ' ' },
            InputContext::default(),
            bypass_ms,
        );
        assert!(
            !matches!(bypassed.action, Some(EngineAction::ReplaceRange(_))),
            "first boundary after each revert must commit original"
        );
        // Keep the v1 long-demotion assertion on the same context bucket.
        session.clear_document_context();
    }

    type_keys(&mut session, "ko", 40);
    let third = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        50,
    );
    assert!(
        !matches!(third.action, Some(EngineAction::ReplaceRange(_))),
        "abbrev/fuzzy assist must yield after two Backspace undos, got {:?}",
        third.action
    );
}

#[test]
fn space_replace_backspace_space_commits_original_once() {
    let mut session = compatibility_session(EngineConfig::default(), khong_lexicon());
    type_keys(&mut session, "khogn", 0);
    let first = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    match &first.action {
        Some(EngineAction::ReplaceRange(action)) => {
            assert_eq!(action.replacement, "không");
        }
        other => panic!("expected first fuzzy replace, got {other:?}"),
    }
    session.inject(InputKind::Backspace, InputContext::default(), 11);
    assert_eq!(session.composition_text(), "khogn");
    let second = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        12,
    );
    assert!(
        !matches!(second.action, Some(EngineAction::ReplaceRange(_))),
        "first boundary after revert must commit original, got {:?}",
        second.action
    );
    assert!(second.candidates.is_empty());
    assert!(session.candidate_texts().is_empty());
    assert!(session.accept_top(13).is_none());
    assert!(session.document_text().contains("khogn"));
    assert!(!session.document_text().contains("không"));
}

#[test]
fn immediate_backspace_rolls_back_without_strong_negative() {
    let mut session = compatibility_session(EngineConfig::default(), khong_lexicon());
    type_keys(&mut session, "khogn", 0);
    let replaced = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    assert!(matches!(
        replaced.action,
        Some(EngineAction::ReplaceRange(_))
    ));

    session.inject(InputKind::Backspace, InputContext::default(), 11);

    assert_eq!(session.composition_text(), "khogn");
    assert_eq!(session.model().evidence_totals(&khogn_rule()).1, 0.0);
}

#[test]
fn recommit_original_after_rollback_adds_strong_negative_once() {
    let mut session = compatibility_session(EngineConfig::default(), khong_lexicon());
    type_keys(&mut session, "khogn", 0);
    session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    session.inject(InputKind::Backspace, InputContext::default(), 11);

    session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        12,
    );
    assert_eq!(session.model().evidence_totals(&khogn_rule()).1, 1.5);

    session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        13,
    );
    assert_eq!(session.model().evidence_totals(&khogn_rule()).1, 1.5);
}

#[test]
fn immediate_revert_window_expires_after_three_seconds() {
    let mut session = compatibility_session(EngineConfig::default(), khong_lexicon());
    type_keys(&mut session, "khogn", 0);
    let first = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    assert!(matches!(first.action, Some(EngineAction::ReplaceRange(_))));

    session.inject(InputKind::Backspace, InputContext::default(), 3_011);
    assert!(
        session.composition_text().is_empty(),
        "late Backspace must not reopen the raw composition"
    );
    assert!(session.document_text().contains("không"));
}

#[test]
fn bypass_still_applies_after_cooldown_if_raw_token_unchanged() {
    let mut session = compatibility_session(EngineConfig::default(), khong_lexicon());
    type_keys(&mut session, "khogn", 0);
    let first = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    assert!(matches!(first.action, Some(EngineAction::ReplaceRange(_))));
    session.inject(InputKind::Backspace, InputContext::default(), 1_000);
    assert_eq!(session.composition_text(), "khogn");

    let after_cooldown = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10_000,
    );
    assert!(
        !matches!(after_cooldown.action, Some(EngineAction::ReplaceRange(_))),
        "one-shot boundary bypass must outlive cooldown, got {:?}",
        after_cooldown.action
    );
    assert!(session.document_text().contains("khogn"));
    assert!(!session.document_text().contains("không"));
}

#[test]
fn changing_raw_token_clears_guard_and_allows_new_correction() {
    let mut session = compatibility_session(EngineConfig::default(), khong_lexicon());
    type_keys(&mut session, "khogn", 0);
    let first = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    assert!(matches!(first.action, Some(EngineAction::ReplaceRange(_))));
    session.inject(InputKind::Backspace, InputContext::default(), 11);
    assert_eq!(session.composition_text(), "khogn");

    session.inject(
        InputKind::Key {
            logical: 'x',
            physical: None,
        },
        InputContext::default(),
        12,
    );
    session.inject(InputKind::Backspace, InputContext::default(), 13);
    assert_eq!(session.composition_text(), "khogn");
    let changed = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        14,
    );
    assert!(
        matches!(changed.action, Some(EngineAction::ReplaceRange(_))),
        "editing raw token must clear one-shot guard, got {:?}",
        changed.action
    );
}

#[test]
fn focus_context_change_clears_revert_guard() {
    let mut session = compatibility_session(EngineConfig::default(), khong_lexicon());
    type_keys(&mut session, "khogn", 0);
    let first = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    assert!(matches!(first.action, Some(EngineAction::ReplaceRange(_))));
    session.inject(InputKind::Backspace, InputContext::default(), 11);
    assert_eq!(session.composition_text(), "khogn");

    session.clear_document_context();
    let after_focus_change = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        12,
    );
    assert!(
        matches!(
            after_focus_change.action,
            Some(EngineAction::ReplaceRange(_))
        ),
        "a focus boundary must clear the old revert guard"
    );
}

#[test]
fn abbrev_revert_bypasses_the_next_boundary_once() {
    let mut session = compatibility_session(EngineConfig::default(), khong_lexicon());
    type_keys(&mut session, "ko", 0);
    let first = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    match &first.action {
        Some(EngineAction::ReplaceRange(action)) => {
            assert_eq!(action.replacement, "không");
        }
        other => panic!("expected first abbrev replace, got {other:?}"),
    }
    session.inject(InputKind::Backspace, InputContext::default(), 11);
    assert_eq!(session.composition_text(), "ko");
    let second = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        12,
    );
    assert!(
        !matches!(second.action, Some(EngineAction::ReplaceRange(_))),
        "abbrev revert must bypass the next boundary, got {:?}",
        second.action
    );
    assert!(session.document_text().contains("ko"));
    assert!(!session.document_text().contains("không"));
}

fn a_acute_lexicon() -> Lexicon {
    Lexicon::from_entries(
        [LexiconEntry {
            token_nfc: "á".to_string(),
            frequency: 10,
        }],
        [],
        Some("session-a-acute"),
    )
}

#[test]
fn one_letter_a_has_no_visible_candidates_or_accept() {
    let mut session = LabSession::new(EngineConfig::default(), a_acute_lexicon());
    type_keys(&mut session, "a", 0);
    assert!(
        session.candidate_texts().is_empty(),
        "TokenTooShort must hide overlay candidates, got {:?}",
        session.candidate_texts()
    );
    assert!(session.accept_top(1).is_none());
}

#[test]
fn session_does_not_replace_when_planner_returned_suggest() {
    let mut session = LabSession::new(EngineConfig::default(), khong_lexicon());
    session.set_intervention_config(InterventionConfig::default());
    type_keys(&mut session, "ko", 0);
    let last = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    assert!(
        !matches!(last.action, Some(EngineAction::ReplaceRange(_))),
        "policy auto off must not upgrade Suggest, got {:?}",
        last.action
    );
}

#[test]
fn ntn_space_does_not_boundary_assist_a_guess() {
    let mut session = LabSession::new(vni_config(), nen_lexicon());
    type_keys(&mut session, "ntn", 0);
    let last = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    assert!(
        !matches!(&last.action, Some(EngineAction::ReplaceRange(_))),
        "ntn must stay typed; got {:?}",
        last.action
    );
    assert!(session.take_learning_notice().is_none());
}

#[test]
fn product_v2_fuzzy_khogn_is_suggest_without_heuristic_flag() {
    let mut session = LabSession::new_with_learning_config(
        EngineConfig::default(),
        khong_lexicon(),
        LearningConfigV2::product_v2(),
    );
    type_keys(&mut session, "khogn", 0);
    let last = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    assert!(matches!(
        last.action,
        Some(EngineAction::ShowSuggestions { .. })
    ));
    assert_eq!(
        session.last_intervention_reason(),
        Some(InterventionReason::LowScore)
    );
    assert!(session.document_text().contains("khogn"));
    assert!(!session.document_text().contains("không"));
}

#[test]
fn product_v2_telex_fix_chfao_still_replaces() {
    let mut session = LabSession::new_with_learning_config(
        EngineConfig::default(),
        chao_lexicon(),
        LearningConfigV2::product_v2(),
    );
    type_keys(&mut session, "chfao", 0);
    let last = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    match &last.action {
        Some(EngineAction::ReplaceRange(action)) => {
            assert_eq!(action.original, "chfao");
            assert_eq!(action.replacement, "chào");
        }
        other => panic!("expected structural chfao → chào replacement, got {other:?}"),
    }
    assert_eq!(
        session.last_intervention_reason(),
        Some(InterventionReason::SafeStructuralFix)
    );
}

#[test]
fn product_v2_abbrev_replaces_after_personal_evidence() {
    let mut session = LabSession::new_with_learning_config(
        EngineConfig::default(),
        khong_lexicon(),
        LearningConfigV2::product_v2(),
    );
    for round in 0..19 {
        let at_ms = i64::from(round) * 10;
        type_keys(&mut session, "ko", at_ms);
        assert!(session.accept_top(at_ms + 2).is_some());
        session.clear_document_context();
    }
    type_keys(&mut session, "ko", 200);
    let last = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        210,
    );
    assert!(
        matches!(last.action, Some(EngineAction::ReplaceRange(_))),
        "expected learned abbreviation replace, got action={:?} reason={:?} decision={:?} candidates={:?} mass={} confidence={}",
        last.action,
        session.last_intervention_reason(),
        last.decision,
        last.candidates,
        session.model().positive_mass(&ko_rule(), 210),
        session.model().confidence(&ko_rule(), 210),
    );
    assert_eq!(
        session.last_intervention_reason(),
        Some(InterventionReason::LearnedCorrection)
    );
}

#[test]
fn diacritics_does_not_boundary_assist_on_space() {
    let mut session = LabSession::new(EngineConfig::default(), khong_lexicon());
    type_keys(&mut session, "khong", 0);
    let last = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    assert!(
        !matches!(last.action, Some(EngineAction::ReplaceRange(_))),
        "khong must stay suggestion-only, got {:?}",
        last.action
    );
    assert!(
        session.document_text().contains("khong"),
        "document was {}",
        session.document_text()
    );
}

#[test]
fn telex_fix_policy_auto_stays_silent() {
    let mut session = LabSession::new(vni_config(), chao_lexicon());
    type_keys(&mut session, "ch2ao", 0);
    session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        10,
    );
    assert!(
        session.take_learning_notice().is_none(),
        "TelexFix auto must not emit a learning notice"
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
fn telex_fix_revert_bypasses_once_without_long_demotion() {
    let mut session = LabSession::new(vni_config(), chao_lexicon());
    for (typed_at, space_ms, undo_ms, bypass_ms) in [(0, 10, 11, 12), (20, 30, 31, 32)] {
        type_keys(&mut session, "ch2ao", typed_at);
        let replaced = session.inject(
            InputKind::Boundary { delimiter: ' ' },
            InputContext::default(),
            space_ms,
        );
        assert!(
            matches!(replaced.action, Some(EngineAction::ReplaceRange(_))),
            "expected TelexFix replace, got {:?}",
            replaced.action
        );
        session.inject(InputKind::Backspace, InputContext::default(), undo_ms);
        assert_eq!(session.composition_text(), "ch2ao");
        let bypassed = session.inject(
            InputKind::Boundary { delimiter: ' ' },
            InputContext::default(),
            bypass_ms,
        );
        assert!(
            !matches!(bypassed.action, Some(EngineAction::ReplaceRange(_))),
            "TelexFix revert must bypass one boundary"
        );
        session.clear_document_context();
    }

    type_keys(&mut session, "ch2ao", 40);
    let third = session.inject(
        InputKind::Boundary { delimiter: ' ' },
        InputContext::default(),
        50,
    );
    match &third.action {
        Some(EngineAction::ReplaceRange(action)) => assert_eq!(action.replacement, "chào"),
        other => panic!("TelexFix must stay deterministic after guarded undos, got {other:?}"),
    }
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

#[test]
fn forgetting_one_identity_preserves_unrelated_v2_journal_history() {
    let mut session = telex_session();
    type_keys(&mut session, "ko", 0);
    assert!(session.accept_top(2).is_some());
    type_keys(&mut session, "ntn", 10);
    assert!(session.accept_top(13).is_some());

    let ko_row = session
        .model()
        .inspection_rows()
        .into_iter()
        .find(|row| row.original_nfc == "ko")
        .unwrap();
    assert!(session.forget_inspection_row(&ko_row));

    let ntn_identity = CorrectionIdentity {
        input_method: InputMethod::Telex,
        source: CandidateSource::Abbreviation,
        original_nfc: "ntn".into(),
        candidate_nfc: "như thế nào".into(),
        source_rule_id: "seed:ntn".into(),
    };
    let ntn_hash = correction_identity_hash(&ntn_identity);
    let log = session.capture_log();
    assert!(log.records.iter().any(|record| matches!(
        record,
        CaptureRecord::CandidateSetEvaluated { identity_hashes, .. }
            if identity_hashes.contains(&ntn_hash)
    )));
    assert!(log.records.iter().any(|record| matches!(
        record,
        CaptureRecord::CorrectionConfirmed { identity, .. } if identity == &ntn_identity
    )));

    let replayed = replay(EngineConfig::default(), empty_lexicon(), &log).unwrap();
    assert!(replayed.model().positive_mass(&ko_rule(), 20).abs() < f64::EPSILON);
    let ntn_rule = RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Abbreviation,
        original_nfc: "ntn".into(),
        candidate_nfc: "như thế nào".into(),
        left_token_nfc: Some("không".into()),
        source_rule_id: "seed:ntn".into(),
    };
    assert_eq!(replayed.model().evidence_totals(&ntn_rule).0, 1.0);
}

#[test]
fn forgotten_rule_cannot_be_resurrected_by_capture_replay() {
    let mut session = telex_session();
    type_keys(&mut session, "ko", 0);
    assert!(session.accept_top(2).is_some());
    assert!(session.model().positive_mass(&ko_rule(), 2) >= 1.0);
    assert!(!session.capture_log().records.is_empty());

    assert!(session.forget_last_rule());
    let log = session.capture_log();
    assert_eq!(log.header.v, 2);
    assert!(matches!(
        log.records.last(),
        Some(CaptureRecord::DataForgotten { .. })
    ));
    assert!(
        !log.records
            .iter()
            .any(|record| matches!(record, CaptureRecord::CorrectionConfirmed { .. })),
        "v2 forget must selectively remove reconstructable identity records"
    );
    let capture_payload = String::from_utf8(log.to_payload().unwrap()).unwrap();
    assert!(!capture_payload.contains("\"original_nfc\":\"ko\""));
    assert!(!capture_payload.contains("\"candidate_nfc\":\"không\""));
    let replayed = replay(EngineConfig::default(), empty_lexicon(), &log).unwrap();
    assert_eq!(replayed.model().positive_mass(&ko_rule(), 2), 0.0);
    let model_payload = String::from_utf8(session.model_payload().unwrap()).unwrap();
    assert!(!model_payload.contains("\"original_nfc\":\"ko\""));
    assert!(!model_payload.contains("\"candidate_nfc\":\"không\""));
}
