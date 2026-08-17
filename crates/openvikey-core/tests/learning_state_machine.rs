//! Milestone 6: deterministic adaptive learning state machine.

#![allow(clippy::float_cmp)]

use openvikey_core::decision::{ActionCap, DecisionConfig, DecisionState, decide};
use openvikey_core::model::{AdaptiveModel, ModelConfig, ModelView, RuleContextKey};
use openvikey_core::rank::{RankingContext, ScoreConfig, rank};
use openvikey_core::types::{Candidate, CandidateSource, FeedbackEvent, FeedbackKind, InputMethod};

const DAY_MS: i64 = 24 * 60 * 60 * 1_000;

fn key(original: &str, candidate: &str) -> RuleContextKey {
    RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Abbreviation,
        original_nfc: original.to_string(),
        candidate_nfc: candidate.to_string(),
        left_token_nfc: Some("tôi".to_string()),
        source_rule_id: format!("seed:{original}"),
    }
}

fn feedback(seq: u64, at_ms: i64, kind: FeedbackKind) -> FeedbackEvent {
    FeedbackEvent { seq, at_ms, kind }
}

#[test]
fn beta_mass_is_non_negative_and_keys_are_isolated() {
    let mut model = AdaptiveModel::default();
    let a = key("ko", "không");
    let b = key("ko", "kể");
    model.apply_feedback(
        &a,
        &feedback(1, 100, FeedbackKind::Accept { candidate_id: 1 }),
        true,
    );
    model.apply_feedback(
        &a,
        &feedback(2, 100, FeedbackKind::ExplicitReject { candidate_id: 1 }),
        true,
    );

    assert_eq!(model.positive_mass(&a, 100), 1.0);
    assert_eq!(model.negative_mass(&a, 100), 1.0);
    assert_eq!(model.confidence(&a, 100), 0.5);
    assert_eq!(model.positive_mass(&b, 100), 0.0);
    assert_eq!(model.negative_mass(&b, 100), 0.0);
    assert_eq!(model.confidence(&b, 100), 0.5);
}

#[test]
fn canonical_accept_18_promotes_but_17_does_not() {
    let mut model = AdaptiveModel::default();
    let rule = key("ko", "không");
    let decision = DecisionConfig::default();

    for seq in 1..=17 {
        model.apply_feedback(
            &rule,
            &feedback(seq, 1_000, FeedbackKind::Accept { candidate_id: 1 }),
            true,
        );
    }
    assert_eq!(model.positive_mass(&rule, 1_000), 17.0);
    assert_eq!(model.state(&rule, 1_000), DecisionState::Suggest);
    assert_eq!(
        decide(
            model.state(&rule, 1_000),
            0.95,
            model.confidence(&rule, 1_000),
            model.positive_mass(&rule, 1_000),
            ActionCap::Auto,
            &decision,
        ),
        DecisionState::Suggest
    );

    model.apply_feedback(
        &rule,
        &feedback(18, 1_000, FeedbackKind::Accept { candidate_id: 1 }),
        true,
    );
    assert!((model.confidence(&rule, 1_000) - 0.95).abs() < 1e-12);
    assert_eq!(
        decide(
            model.state(&rule, 1_000),
            0.95,
            model.confidence(&rule, 1_000),
            model.positive_mass(&rule, 1_000),
            ActionCap::Auto,
            &decision,
        ),
        DecisionState::Auto
    );
}

#[test]
fn two_undos_in_last_ten_auto_emissions_demote() {
    let mut model = AdaptiveModel::default();
    let rule = key("ko", "không");
    for edit_id in 1..=11 {
        model.record_auto_emission(&rule, edit_id, i64::try_from(edit_id).unwrap());
    }
    assert_eq!(model.state(&rule, 20), DecisionState::Auto);

    model.apply_feedback(
        &rule,
        &feedback(20, 20, FeedbackKind::Undo { edit_id: 1 }),
        true,
    );
    model.apply_feedback(
        &rule,
        &feedback(21, 21, FeedbackKind::Undo { edit_id: 2 }),
        true,
    );
    assert_eq!(model.state(&rule, 21), DecisionState::Auto);

    model.apply_feedback(
        &rule,
        &feedback(22, 22, FeedbackKind::Undo { edit_id: 3 }),
        true,
    );
    assert_eq!(model.state(&rule, 22), DecisionState::Suggest);
}

#[test]
fn evidence_decay_uses_injected_time_and_clamps_negative_age() {
    let config = ModelConfig {
        half_life_ms: 10 * DAY_MS,
        ..ModelConfig::default()
    };
    let mut model = AdaptiveModel::new(config);
    let rule = key("ko", "không");
    model.apply_feedback(
        &rule,
        &feedback(1, 10 * DAY_MS, FeedbackKind::Accept { candidate_id: 1 }),
        true,
    );

    assert_eq!(model.positive_mass(&rule, 0), 1.0);
    assert!((model.positive_mass(&rule, 20 * DAY_MS) - 0.5).abs() < 1e-12);
}

#[test]
fn settled_signals_are_recorded_exactly_once() {
    let mut model = AdaptiveModel::default();
    let rule = key("ko", "không");
    model.apply_feedback(
        &rule,
        &feedback(1, 0, FeedbackKind::AutoSettled { edit_id: 7 }),
        true,
    );
    model.apply_feedback(
        &rule,
        &feedback(2, 0, FeedbackKind::AutoSettled { edit_id: 7 }),
        true,
    );
    model.apply_feedback(
        &rule,
        &feedback(3, 0, FeedbackKind::SuggestionSettled { candidate_id: 9 }),
        true,
    );
    model.apply_feedback(
        &rule,
        &feedback(4, 0, FeedbackKind::SuggestionSettled { candidate_id: 9 }),
        true,
    );

    assert!((model.positive_mass(&rule, 0) - 0.3).abs() < 1e-12);
    assert!((model.negative_mass(&rule, 0) - 0.2).abs() < 1e-12);
}

#[test]
fn same_events_and_time_serialize_identically() {
    let rule = key("ko", "không");
    let events = [
        feedback(1, 10, FeedbackKind::Accept { candidate_id: 1 }),
        feedback(2, 20, FeedbackKind::AutoSettled { edit_id: 7 }),
        feedback(3, 30, FeedbackKind::ExplicitReject { candidate_id: 1 }),
    ];
    let mut a = AdaptiveModel::default();
    let mut b = AdaptiveModel::default();
    for event in &events {
        a.apply_feedback(&rule, event, true);
        b.apply_feedback(&rule, event, true);
    }

    assert_eq!(a.confidence(&rule, 40), b.confidence(&rule, 40));
    assert_eq!(a.to_json_payload().unwrap(), b.to_json_payload().unwrap());
    assert_eq!(
        AdaptiveModel::from_json_payload(&a.to_json_payload().unwrap()).unwrap(),
        a
    );
}

#[test]
fn personal_rerank_uses_rule_specific_confidence() {
    let mut model = AdaptiveModel::default();
    let preferred = key("ko", "không");
    for seq in 1..=18 {
        model.apply_feedback(
            &preferred,
            &feedback(seq, 0, FeedbackKind::Accept { candidate_id: 1 }),
            true,
        );
    }
    let candidates = vec![
        Candidate {
            id: 2,
            text: "kể".to_string(),
            source: CandidateSource::Abbreviation,
            evidence: "seed:ko-other".to_string(),
            base_score: 0.8,
            final_score: 0.0,
        },
        Candidate {
            id: 1,
            text: "không".to_string(),
            source: CandidateSource::Abbreviation,
            evidence: "seed:ko".to_string(),
            base_score: 0.8,
            final_score: 0.0,
        },
    ];
    let ranked = rank(
        candidates,
        &model,
        0,
        &ScoreConfig::abbrev_v1(),
        Some(&RankingContext {
            input_method: InputMethod::Telex,
            original_nfc: "ko".to_string(),
            left_token_nfc: Some("tôi".to_string()),
        }),
    );
    assert_eq!(ranked[0].text, "không");
    assert!(ranked[0].final_score > ranked[1].final_score);
}

#[test]
fn learning_disabled_does_not_mutate_model() {
    let mut model = AdaptiveModel::default();
    let before = model.to_json_payload().unwrap();
    model.apply_feedback(
        &key("ko", "không"),
        &feedback(1, 0, FeedbackKind::Accept { candidate_id: 1 }),
        false,
    );
    assert_eq!(model.to_json_payload().unwrap(), before);
}
