//! Milestone 6: deterministic adaptive learning state machine.

#![allow(clippy::float_cmp)]

use openvikey_core::correction::{
    AutoEditContext, InterventionConfig, run_correction_slice, run_learning_correction_slice,
};
use openvikey_core::decision::{ActionCap, DecisionConfig, DecisionState, decide};
use openvikey_core::feedback::LearningSession;
use openvikey_core::generate::personal::PersonalGenerator;
use openvikey_core::generate::{Generator, LeftContext};
use openvikey_core::lexicon::Lexicon;
use openvikey_core::model::{AdaptiveModel, ModelConfig, ModelView, RuleContextKey};
use openvikey_core::rank::{RankingContext, ScoreConfig, rank};
use openvikey_core::types::{
    Candidate, CandidateSource, CompositionSnapshot, EditRange, EngineAction, FeedbackEvent,
    FeedbackKind, InputContext, InputMethod, RangeBasis,
};

const DAY_MS: i64 = 24 * 60 * 60 * 1_000;

struct FixedGenerator {
    source: CandidateSource,
    score: f64,
}

impl Generator for FixedGenerator {
    fn source(&self) -> CandidateSource {
        self.source
    }

    fn generate(
        &self,
        _snapshot: &CompositionSnapshot,
        _left_context: &LeftContext,
    ) -> Vec<Candidate> {
        vec![Candidate {
            id: 77,
            text: "không".to_string(),
            source: self.source,
            evidence: "fixed:ko".to_string(),
            base_score: self.score,
            final_score: 0.0,
        }]
    }
}

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

fn fixed_rule(source: CandidateSource) -> RuleContextKey {
    RuleContextKey {
        source,
        source_rule_id: "fixed:ko".to_string(),
        left_token_nfc: None,
        ..key("ko", "không")
    }
}

fn model_with_accepts(rule: &RuleContextKey, count: u64, at_ms: i64) -> AdaptiveModel {
    let mut model = AdaptiveModel::default();
    for seq in 1..=count {
        model.apply_feedback(
            rule,
            &feedback(seq, at_ms, FeedbackKind::Accept { candidate_id: 77 }),
            true,
        );
    }
    model
}

fn fixed_generator(source: CandidateSource, score: f64) -> FixedGenerator {
    FixedGenerator { source, score }
}

fn ko_snapshot() -> CompositionSnapshot {
    CompositionSnapshot::new(10, "ko".to_string(), "ko".to_string())
}

fn auto_edit(edit_id: u64, revision: u64, delimiter: Option<char>) -> AutoEditContext {
    AutoEditContext {
        edit_id,
        range: EditRange {
            basis: RangeBasis::CommittedBeforeCaret,
            start_grapheme: 0,
            length_grapheme: 2,
            revision,
        },
        delimiter,
    }
}

#[test]
fn promoted_correction_emits_replace_range_and_records_auto_edit() {
    let rule = fixed_rule(CandidateSource::Abbreviation);
    let mut session = LearningSession::new(model_with_accepts(&rule, 18, 1_000), 8);
    let generator = fixed_generator(CandidateSource::Abbreviation, 0.95);
    let snapshot = ko_snapshot();
    let edit = auto_edit(900, 10, Some(' '));
    let range = edit.range;

    let slice = run_learning_correction_slice(
        &snapshot,
        &LeftContext::default(),
        InputContext::default(),
        &[&generator],
        InputMethod::Telex,
        &mut session,
        1_000,
        &ScoreConfig::default(),
        &DecisionConfig::default(),
        Some(edit),
        &Lexicon::empty(),
        InterventionConfig::default(),
    );

    assert_eq!(slice.decision, Some(DecisionState::Auto));
    assert_eq!(
        slice.action,
        Some(EngineAction::ReplaceRange(
            openvikey_core::types::ReplaceRangeAction {
                edit_id: 900,
                range,
                original: "ko".to_string(),
                replacement: "không".to_string(),
                delimiter: Some(' '),
            }
        ))
    );
    assert_eq!(session.model().state(&rule, 1_000), DecisionState::Auto);
    let undo = session
        .undo(10, 19, 1_001, true)
        .expect("emitted auto replacement is connected to semantic undo");
    assert_eq!(undo.inverse.original, "không");
    assert_eq!(undo.inverse.replacement, "ko");
    assert_eq!(undo.inverse.delimiter, Some(' '));
    assert_eq!(undo.inverse.range.revision, 11);
}

#[test]
fn read_only_correction_without_edit_payload_degrades_auto_to_suggestion() {
    let rule = fixed_rule(CandidateSource::Abbreviation);
    let model = model_with_accepts(&rule, 18, 1_000);
    let generator = fixed_generator(CandidateSource::Abbreviation, 0.95);
    let slice = run_correction_slice(
        &ko_snapshot(),
        &LeftContext::default(),
        InputContext::default(),
        &[&generator],
        InputMethod::Telex,
        &model,
        1_000,
        &ScoreConfig::default(),
        &DecisionConfig::default(),
    );

    assert_eq!(slice.decision, Some(DecisionState::Suggest));
    assert!(matches!(
        slice.action,
        Some(EngineAction::ShowSuggestions { .. })
    ));
}

#[test]
fn stale_auto_edit_context_degrades_to_suggestion() {
    let rule = fixed_rule(CandidateSource::Abbreviation);
    let mut session = LearningSession::new(model_with_accepts(&rule, 18, 1_000), 8);
    let generator = fixed_generator(CandidateSource::Abbreviation, 0.95);
    let slice = run_learning_correction_slice(
        &CompositionSnapshot::new(10, "ko".to_string(), "ko".to_string()),
        &LeftContext::default(),
        InputContext::default(),
        &[&generator],
        InputMethod::Telex,
        &mut session,
        1_000,
        &ScoreConfig::default(),
        &DecisionConfig::default(),
        Some(auto_edit(902, 9, None)),
        &Lexicon::empty(),
        InterventionConfig::default(),
    );

    assert_eq!(slice.decision, Some(DecisionState::Suggest));
    assert!(matches!(
        slice.action,
        Some(EngineAction::ShowSuggestions { .. })
    ));
}

#[test]
fn learning_context_false_allows_auto_undo_but_does_not_mutate_model() {
    let rule = fixed_rule(CandidateSource::Abbreviation);
    let mut session = LearningSession::new(model_with_accepts(&rule, 18, 1_000), 8);
    let before = session.model().to_json_payload().unwrap();
    let generator = fixed_generator(CandidateSource::Abbreviation, 0.95);
    let snapshot = ko_snapshot();
    let slice = run_learning_correction_slice(
        &snapshot,
        &LeftContext::default(),
        InputContext {
            allow_transform: true,
            allow_learning: false,
        },
        &[&generator],
        InputMethod::Telex,
        &mut session,
        1_000,
        &ScoreConfig::default(),
        &DecisionConfig::default(),
        Some(auto_edit(901, 10, None)),
        &Lexicon::empty(),
        InterventionConfig::default(),
    );

    assert!(matches!(slice.action, Some(EngineAction::ReplaceRange(_))));
    assert_eq!(session.model().to_json_payload().unwrap(), before);
    assert!(session.undo(10, 99, 1_001, false).is_some());
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
    model.record_decision(&rule, DecisionState::Auto, true);
    for edit_id in 1..=11 {
        model.record_auto_emission(&rule, edit_id, i64::try_from(edit_id).unwrap(), true);
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
fn two_recent_undos_block_immediate_repromotion_even_at_high_confidence() {
    let rule = fixed_rule(CandidateSource::Abbreviation);
    let mut model = model_with_accepts(&rule, 100, 0);
    for edit_id in 1..=10 {
        model.record_auto_emission(&rule, edit_id, 0, true);
    }
    model.apply_feedback(
        &rule,
        &feedback(101, 0, FeedbackKind::Undo { edit_id: 9 }),
        true,
    );
    model.apply_feedback(
        &rule,
        &feedback(102, 0, FeedbackKind::Undo { edit_id: 10 }),
        true,
    );
    assert!(model.confidence(&rule, 0) > 0.95);

    let mut session = LearningSession::new(model, 8);
    let generator = fixed_generator(CandidateSource::Abbreviation, 0.95);
    let slice = run_learning_correction_slice(
        &CompositionSnapshot::new(10, "ko".to_string(), "ko".to_string()),
        &LeftContext::default(),
        InputContext::default(),
        &[&generator],
        InputMethod::Telex,
        &mut session,
        0,
        &ScoreConfig::default(),
        &DecisionConfig::default(),
        Some(auto_edit(999, 10, None)),
        &Lexicon::empty(),
        InterventionConfig::default(),
    );

    assert_eq!(slice.decision, Some(DecisionState::Suggest));
    assert!(matches!(
        slice.action,
        Some(EngineAction::ShowSuggestions { .. })
    ));

    for seq in 103..=120 {
        session.model_mut().apply_feedback(
            &rule,
            &feedback(seq, 0, FeedbackKind::Accept { candidate_id: 77 }),
            true,
        );
    }
    let retrained = run_learning_correction_slice(
        &CompositionSnapshot::new(10, "ko".to_string(), "ko".to_string()),
        &LeftContext::default(),
        InputContext::default(),
        &[&generator],
        InputMethod::Telex,
        &mut session,
        0,
        &ScoreConfig::default(),
        &DecisionConfig::default(),
        Some(auto_edit(1_001, 10, None)),
        &Lexicon::empty(),
        InterventionConfig::default(),
    );
    assert_eq!(retrained.decision, Some(DecisionState::Auto));
}

#[test]
fn diacritics_stays_suggestion_only_after_promotion_mass() {
    let rule = fixed_rule(CandidateSource::Diacritics);
    let mut session = LearningSession::new(model_with_accepts(&rule, 18, 0), 8);
    let generator = fixed_generator(CandidateSource::Diacritics, 0.95);
    let slice = run_learning_correction_slice(
        &CompositionSnapshot::new(10, "ko".to_string(), "ko".to_string()),
        &LeftContext::default(),
        InputContext::default(),
        &[&generator],
        InputMethod::Telex,
        &mut session,
        0,
        &ScoreConfig::default(),
        &DecisionConfig::default(),
        Some(auto_edit(1_000, 10, None)),
        &Lexicon::empty(),
        InterventionConfig::default(),
    );

    assert_eq!(slice.decision, Some(DecisionState::Suggest));
    assert!(matches!(
        slice.action,
        Some(EngineAction::ShowSuggestions { .. })
    ));
    assert_eq!(session.model().state(&rule, 0), DecisionState::Suggest);
}

#[test]
fn decision_hysteresis_persists_across_correction_calls() {
    let snapshot = ko_snapshot();
    let mut session = LearningSession::new(AdaptiveModel::default(), 8);
    let on = fixed_generator(CandidateSource::Abbreviation, 0.70);
    let first = run_learning_correction_slice(
        &snapshot,
        &LeftContext::default(),
        InputContext::default(),
        &[&on],
        InputMethod::Telex,
        &mut session,
        0,
        &ScoreConfig::default(),
        &DecisionConfig::default(),
        None,
        &Lexicon::empty(),
        InterventionConfig::default(),
    );
    assert_eq!(first.decision, Some(DecisionState::Suggest));

    let inside_hysteresis_band = fixed_generator(CandidateSource::Abbreviation, 0.65);
    let second = run_learning_correction_slice(
        &snapshot,
        &LeftContext::default(),
        InputContext::default(),
        &[&inside_hysteresis_band],
        InputMethod::Telex,
        &mut session,
        1,
        &ScoreConfig::default(),
        &DecisionConfig::default(),
        None,
        &Lexicon::empty(),
        InterventionConfig::default(),
    );
    assert_eq!(second.decision, Some(DecisionState::Suggest));
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
fn independent_rule_event_streams_converge_across_interleavings() {
    let first = key("ko", "không");
    let second = key("ntn", "như thế nào");
    let first_events = [
        feedback(1, 10, FeedbackKind::Accept { candidate_id: 1 }),
        feedback(3, 30, FeedbackKind::AutoSettled { edit_id: 7 }),
    ];
    let second_events = [
        feedback(2, 20, FeedbackKind::ExplicitReject { candidate_id: 2 }),
        feedback(4, 40, FeedbackKind::Accept { candidate_id: 2 }),
    ];
    let mut forward = AdaptiveModel::default();
    for event in &first_events {
        forward.apply_feedback(&first, event, true);
    }
    for event in &second_events {
        forward.apply_feedback(&second, event, true);
    }
    let mut reverse = AdaptiveModel::default();
    for event in &second_events {
        reverse.apply_feedback(&second, event, true);
    }
    for event in &first_events {
        reverse.apply_feedback(&first, event, true);
    }

    for evaluate_at_ms in [-1, 40, DAY_MS] {
        assert_eq!(
            forward.confidence(&first, evaluate_at_ms),
            reverse.confidence(&first, evaluate_at_ms)
        );
        assert_eq!(
            forward.confidence(&second, evaluate_at_ms),
            reverse.confidence(&second, evaluate_at_ms)
        );
    }
    assert_eq!(
        forward.to_json_payload().unwrap(),
        reverse.to_json_payload().unwrap()
    );
    assert_eq!(
        AdaptiveModel::from_json_payload(&forward.to_json_payload().unwrap()).unwrap(),
        forward
    );
}

#[test]
fn personal_rerank_uses_rule_specific_confidence() {
    let preferred = key("ko", "không");
    let model = model_with_accepts(&preferred, 18, 0);
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

#[test]
fn left_token_backoff_sums_mass_and_keeps_other_candidates_isolated() {
    let toi = key("khogn", "không");
    let mut rat = toi.clone();
    rat.left_token_nfc = Some("rất".to_string());
    let mut other = toi.clone();
    other.candidate_nfc = "khổng".to_string();
    other.source_rule_id = "seed:khogn-other".to_string();

    let mut model = AdaptiveModel::default();
    model.apply_feedback(
        &toi,
        &feedback(1, 100, FeedbackKind::Accept { candidate_id: 1 }),
        true,
    );
    model.apply_feedback(
        &rat,
        &feedback(2, 100, FeedbackKind::Accept { candidate_id: 1 }),
        true,
    );
    model.apply_feedback(
        &other,
        &feedback(3, 100, FeedbackKind::Accept { candidate_id: 2 }),
        true,
    );

    assert_eq!(model.positive_mass(&toi, 100), 2.0);
    assert_eq!(model.positive_mass(&rat, 100), 2.0);
    assert_eq!(model.positive_mass(&other, 100), 1.0);
}

#[test]
fn implicit_correction_adds_one_and_a_half_mass() {
    let mut model = AdaptiveModel::default();
    let rule = key("ko", "không");
    model.apply_feedback(
        &rule,
        &feedback(
            1,
            0,
            FeedbackKind::ImplicitCorrection {
                original: "ko".into(),
                replacement: "không".into(),
            },
        ),
        true,
    );
    assert_eq!(model.positive_mass(&rule, 0), 1.5);
}

#[test]
fn personal_store_promotes_on_second_repeat_and_old_payload_loads() {
    let mut model = AdaptiveModel::default();
    assert!(!model.record_personal_correction(InputMethod::Vni, "x3uong", "xưởng", true));
    assert!(model.personal_promoted().is_empty());
    assert!(model.record_personal_correction(InputMethod::Vni, "x3uong", "xưởng", true));
    assert_eq!(
        model.personal_promoted(),
        vec![(InputMethod::Vni, "x3uong".into(), "xưởng".into())]
    );

    let legacy = br#"{"version":1,"config":{"half_life_ms":2592000000,"max_events_per_rule":512,"auto_undo_window":10},"entries":[]}"#;
    let loaded = AdaptiveModel::from_json_payload(legacy).expect("legacy payload");
    assert!(loaded.personal_promoted().is_empty());
}

#[test]
fn left_token_backoff_uses_max_decision_state() {
    let toi = key("khogn", "không");
    let mut rat = toi.clone();
    rat.left_token_nfc = Some("rất".to_string());
    let mut model = AdaptiveModel::default();
    model.record_decision(&toi, DecisionState::Auto, true);
    assert_eq!(model.state(&rat, 0), DecisionState::Auto);
}

#[test]
fn auto_settled_mass_caps_at_twenty_four_settlements() {
    let mut model = AdaptiveModel::default();
    let rule = key("ko", "không");
    for edit_id in 1..=25 {
        model.apply_feedback(
            &rule,
            &feedback(edit_id, 0, FeedbackKind::AutoSettled { edit_id }),
            true,
        );
    }
    assert!((model.positive_mass(&rule, 0) - 7.2).abs() < 1e-12);
}

#[test]
fn personal_generator_looks_up_promoted_normalized() {
    let generator = PersonalGenerator::for_method(
        InputMethod::Vni,
        &[(InputMethod::Vni, "x3uong".into(), "xưởng".into())],
    );
    let candidates = generator.generate(
        &CompositionSnapshot::new(1, "x3uong".into(), "x3uong".into()),
        &LeftContext::default(),
    );
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].text, "xưởng");
    assert_eq!(candidates[0].source, CandidateSource::Personal);
    assert_eq!(candidates[0].id, 5_000_000);
}
