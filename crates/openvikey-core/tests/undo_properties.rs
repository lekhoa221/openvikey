//! Milestone 6: auto edit logging, inverse undo, and implicit-correction boundaries.

#![allow(clippy::float_cmp)]

use openvikey_core::feedback::{
    CompositionPeak, CompositionRewindMiner, ImplicitCorrectionMiner, LearningSession,
    RewindEvaluate,
};
use openvikey_core::model::{AdaptiveModel, ModelView, RuleContextKey};
use openvikey_core::types::{
    Candidate, CandidateSource, EditRange, FeedbackKind, InputMethod, RangeBasis,
    ReplaceRangeAction,
};
use unicode_segmentation::UnicodeSegmentation;

fn rule() -> RuleContextKey {
    RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Abbreviation,
        original_nfc: "ntn".to_string(),
        candidate_nfc: "như thế nào".to_string(),
        left_token_nfc: None,
        source_rule_id: "seed:ntn".to_string(),
    }
}

fn edit() -> ReplaceRangeAction {
    ReplaceRangeAction {
        edit_id: 42,
        range: EditRange {
            basis: RangeBasis::CommittedBeforeCaret,
            start_grapheme: 0,
            length_grapheme: "ntn".graphemes(true).count(),
            revision: 10,
        },
        original: "ntn".to_string(),
        replacement: "như thế nào".to_string(),
        delimiter: Some(','),
    }
}

#[test]
fn auto_edit_undo_returns_exact_inverse_and_negative_evidence() {
    let mut session = LearningSession::new(AdaptiveModel::default(), 8);
    session.record_auto_edit(rule(), edit(), 100, true);

    let outcome = session
        .undo(10, 1, 110, true)
        .expect("matching revision is undoable");
    assert_eq!(outcome.inverse.edit_id, 42);
    assert_eq!(outcome.inverse.original, "như thế nào");
    assert_eq!(outcome.inverse.replacement, "ntn");
    assert_eq!(outcome.inverse.delimiter, Some(','));
    assert_eq!(outcome.inverse.range.revision, 11);
    assert_eq!(outcome.feedback.kind, FeedbackKind::Undo { edit_id: 42 });
    assert_eq!(session.model().negative_mass(&rule(), 110), 1.5);
    for seq in 2..=11 {
        assert!(session.observe_input_or_edit(seq, 110, true).is_empty());
    }
}

#[test]
fn auto_settles_once_after_ten_subsequent_input_or_edit_events() {
    let mut session = LearningSession::new(AdaptiveModel::default(), 8);
    session.record_auto_edit(rule(), edit(), 100, true);

    for seq in 1..=9 {
        assert!(
            session
                .observe_input_or_edit(seq, 100 + i64::try_from(seq).unwrap(), true)
                .is_empty()
        );
        assert_eq!(session.model().positive_mass(&rule(), 200), 0.0);
    }
    let settled = session.observe_input_or_edit(10, 4100, true);
    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0].kind, FeedbackKind::AutoSettled { edit_id: 42 });
    assert_eq!(session.model().positive_mass(&rule(), 4100), 0.3);
    let mass_at_4111 = session.model().positive_mass(&rule(), 4111);
    assert!(session.observe_input_or_edit(11, 4111, true).is_empty());
    assert_eq!(session.model().positive_mass(&rule(), 4111), mass_at_4111);
}

#[test]
fn caret_break_invalidates_undo_and_implicit_correction() {
    let mut session = LearningSession::new(AdaptiveModel::default(), 8);
    session.record_auto_edit(rule(), edit(), 100, true);
    session.invalidate_due_to_caret_break();
    assert!(session.undo(10, 1, 110, true).is_none());

    let mut miner = ImplicitCorrectionMiner::default();
    miner.record_deleted_token("teh");
    miner.invalidate_due_to_caret_break();
    assert!(miner.finish_replacement("the", 2, 120).is_none());
}

#[test]
fn contiguous_delete_and_retype_mines_one_feedback_event() {
    let mut miner = ImplicitCorrectionMiner::default();
    miner.record_deleted_token("teh");
    let event = miner
        .finish_replacement("the", 7, 500)
        .expect("contiguous correction should be learned");
    assert_eq!(
        event.kind,
        FeedbackKind::ImplicitCorrection {
            original: "teh".to_string(),
            replacement: "the".to_string(),
        }
    );
    assert!(miner.finish_replacement("then", 8, 510).is_none());
}

#[test]
fn learning_disabled_still_undoes_but_does_not_update_model() {
    let mut session = LearningSession::new(AdaptiveModel::default(), 8);
    let before = session.model().to_json_payload().unwrap();
    session.record_auto_edit(rule(), edit(), 100, false);
    assert!(session.undo(10, 1, 110, false).is_some());
    assert_eq!(session.model().to_json_payload().unwrap(), before);
}

fn peak(original: &str, candidate: &str, started_at_ms: i64) -> CompositionPeak {
    CompositionPeak {
        original_nfc: original.to_string(),
        raw_keys: original.to_string(),
        candidates: vec![Candidate {
            id: 1,
            text: candidate.to_string(),
            source: CandidateSource::Abbreviation,
            evidence: "seed:ko".to_string(),
            base_score: 0.8,
            final_score: 0.8,
        }],
        left_token_nfc: None,
        input_method: InputMethod::Telex,
        started_at_ms,
    }
}

#[test]
fn rewind_miner_keeps_last_peak_only() {
    let mut miner = CompositionRewindMiner::default();
    miner.on_backspace(peak("ko", "không", 0));
    miner.on_key();
    miner.on_backspace(peak("kx", "kẻ", 5));
    miner.on_key();
    match miner.evaluate("không", 1, 20) {
        RewindEvaluate::Unmatched {
            original_nfc,
            replacement_nfc,
            ..
        } => {
            assert_eq!(original_nfc, "kx");
            assert_eq!(replacement_nfc, "không");
        }
        other => panic!("expected unmatched last peak, got {other:?}"),
    }
}

#[test]
fn rewind_miner_times_out_after_ten_seconds() {
    let mut miner = CompositionRewindMiner::default();
    miner.on_backspace(peak("ko", "không", 0));
    miner.on_key();
    assert!(matches!(
        miner.evaluate("không", 1, 10_001),
        RewindEvaluate::Ignored
    ));
}

#[test]
fn rewind_miner_matches_peak_candidate() {
    let mut miner = CompositionRewindMiner::default();
    miner.on_backspace(peak("ko", "không", 0));
    miner.on_key();
    match miner.evaluate("không", 3, 50) {
        RewindEvaluate::Matched { key, .. } => {
            assert_eq!(key.original_nfc, "ko");
            assert_eq!(key.candidate_nfc, "không");
            assert!(key.left_token_nfc.is_none());
        }
        other => panic!("expected match, got {other:?}"),
    }
}

#[test]
fn auto_does_not_settle_before_three_seconds() {
    let mut session = LearningSession::new(AdaptiveModel::default(), 8);
    session.record_auto_edit(rule(), edit(), 100, true);
    for seq in 1..=10 {
        assert!(session.observe_input_or_edit(seq, 200, true).is_empty());
    }
    assert_eq!(session.model().positive_mass(&rule(), 200), 0.0);
    let settled = session.observe_input_or_edit(11, 4100, true);
    assert_eq!(settled.len(), 1);
    assert_eq!(session.model().positive_mass(&rule(), 4100), 0.3);
}
