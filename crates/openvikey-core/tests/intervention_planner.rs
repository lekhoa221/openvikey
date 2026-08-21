//! Unified intervention planner — Lát 1.

use openvikey_core::correction::InterventionConfig;
use openvikey_core::decision::DecisionState;
use openvikey_core::intervention::{
    InterventionAction, InterventionReason, alphabetic_grapheme_count, plan_intervention,
};
use openvikey_core::learning_config::LearningConfigV2;
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use openvikey_core::model::{AdaptiveModel, EmptyModel, ModelView, RuleContextKey};
use openvikey_core::types::{
    Candidate, CandidateSource, CompositionSnapshot, FeedbackEvent, FeedbackKind, InputContext,
    InputMethod,
};

fn empty_lexicon() -> Lexicon {
    Lexicon::from_entries([], [], Some("empty"))
}

fn lex(tokens: &[&str]) -> Lexicon {
    Lexicon::from_entries(
        tokens.iter().map(|token| LexiconEntry {
            token_nfc: (*token).to_string(),
            frequency: 10,
        }),
        [],
        Some("planner-lex"),
    )
}

fn fuzzy_khogn() -> Candidate {
    Candidate {
        id: 42,
        text: "không".into(),
        source: CandidateSource::Fuzzy,
        evidence: "fuzzy:khogn".into(),
        base_score: 0.95,
        final_score: 0.95,
    }
}

fn khogn_snapshot() -> CompositionSnapshot {
    CompositionSnapshot::new(1, "khogn".into(), "khogn".into())
}

fn khogn_rule() -> RuleContextKey {
    RuleContextKey {
        input_method: InputMethod::Telex,
        source: CandidateSource::Fuzzy,
        original_nfc: "khogn".into(),
        candidate_nfc: "không".into(),
        left_token_nfc: None,
        source_rule_id: "fuzzy:khogn".into(),
    }
}

fn plan_with(
    ranked: &[Candidate],
    model: &dyn ModelView,
    context: InputContext,
    auto_edit_valid: bool,
) -> openvikey_core::intervention::InterventionPlan {
    plan_intervention(
        &khogn_snapshot(),
        ranked,
        &empty_lexicon(),
        model,
        &LearningConfigV2::compatibility_v1(),
        InterventionConfig::win32(),
        context,
        Some(' '),
        None,
        0,
        auto_edit_valid,
        InputMethod::Telex,
        None,
    )
}

fn model_with_accepts(count: u64) -> AdaptiveModel {
    let mut model = AdaptiveModel::default();
    let key = khogn_rule();
    for seq in 1..=count {
        model.apply_feedback(
            &key,
            &FeedbackEvent {
                seq,
                at_ms: 0,
                kind: FeedbackKind::Accept { candidate_id: 42 },
            },
            true,
        );
    }
    model
}

#[test]
fn empty_candidates_are_none_no_candidate() {
    let plan = plan_with(&[], &EmptyModel, InputContext::default(), true);
    assert_eq!(plan.action, InterventionAction::None);
    assert_eq!(plan.reason, InterventionReason::NoCandidate);
    assert_eq!(plan.candidate_id, None);
}

#[test]
fn allow_transform_false_is_none_unsafe_even_with_candidates() {
    let ctx = InputContext {
        allow_transform: false,
        allow_learning: true,
    };
    let plan = plan_with(&[fuzzy_khogn()], &EmptyModel, ctx, true);
    assert_eq!(plan.action, InterventionAction::None);
    assert_eq!(plan.reason, InterventionReason::UnsafeContext);
    assert_eq!(plan.model_transition, None);
}

#[test]
fn learned_auto_with_valid_edit_replaces_and_reasons_learned_correction() {
    let model = model_with_accepts(18);
    let ranked = [fuzzy_khogn()];
    let plan = plan_with(&ranked, &model, InputContext::default(), true);
    assert_eq!(plan.action, InterventionAction::Replace);
    assert_eq!(plan.reason, InterventionReason::LearnedCorrection);
    assert_eq!(plan.candidate_id, Some(ranked[0].id));
    assert!(plan.undo_contract.required);
    assert_eq!(plan.model_transition, Some(DecisionState::Auto));
}

#[test]
fn learned_auto_without_valid_edit_degrades_to_suggestion() {
    let model = model_with_accepts(18);
    let plan = plan_with(&[fuzzy_khogn()], &model, InputContext::default(), false);
    assert_eq!(plan.action, InterventionAction::DisplaySuggestion);
    assert_eq!(plan.reason, InterventionReason::LearnedCorrection);
    assert_eq!(plan.model_transition, Some(DecisionState::Suggest));
}

#[test]
fn product_v2_bumps_config_version_and_hash() {
    let compat = LearningConfigV2::compatibility_v1();
    let product = LearningConfigV2::product_v2();
    assert_eq!(compat.version, 1);
    assert_eq!(product.version, 2);
    assert_ne!(compat.hash(), product.hash());
}

#[test]
fn low_score_keeps_top_candidate_and_breakdown_without_persisting_as_required_undo() {
    let mut weak = fuzzy_khogn();
    weak.base_score = 0.4;
    weak.final_score = 0.4;
    let plan = plan_with(&[weak.clone()], &EmptyModel, InputContext::default(), true);
    assert_eq!(plan.action, InterventionAction::None);
    assert_eq!(plan.reason, InterventionReason::LowScore);
    assert_eq!(plan.candidate_id, Some(weak.id));
    assert!((plan.score_breakdown.generator_base - 0.4).abs() < 1e-12);
    assert!((plan.score_breakdown.final_score - 0.4).abs() < 1e-12);
    assert_eq!(plan.model_transition, Some(DecisionState::Ignore));
    assert!(!plan.undo_contract.required);
}

fn telex_fix_candidate(text: &str) -> Candidate {
    Candidate {
        id: 2_000_001,
        text: text.to_string(),
        source: CandidateSource::TelexFix,
        evidence: "telex-fix:move-tone-f".to_string(),
        base_score: 0.92,
        final_score: 0.92,
    }
}

fn abbrev_candidate(text: &str) -> Candidate {
    Candidate {
        id: 3_000_001,
        text: text.to_string(),
        source: CandidateSource::Abbreviation,
        evidence: "seed:ko".to_string(),
        base_score: 0.8,
        final_score: 0.8,
    }
}

fn diacritics_candidate(text: &str) -> Candidate {
    Candidate {
        id: 1,
        text: text.to_string(),
        source: CandidateSource::Diacritics,
        evidence: "diacritics:unigram".to_string(),
        base_score: 0.9,
        final_score: 0.9,
    }
}

fn plan_on(
    snapshot: &CompositionSnapshot,
    ranked: &[Candidate],
    lexicon: &Lexicon,
) -> openvikey_core::intervention::InterventionPlan {
    plan_intervention(
        snapshot,
        ranked,
        lexicon,
        &EmptyModel,
        &LearningConfigV2::compatibility_v1(),
        InterventionConfig::win32(),
        InputContext::default(),
        Some(' '),
        None,
        0,
        true,
        InputMethod::Telex,
        None,
    )
}

#[test]
fn unique_telex_fix_is_replace_safe_structural_fix() {
    let snapshot = CompositionSnapshot::new(1, "chfao".into(), "chfao".into());
    let ranked = [telex_fix_candidate("chào")];
    let plan = plan_on(&snapshot, &ranked, &lex(&["chào"]));
    assert_eq!(plan.action, InterventionAction::Replace);
    assert_eq!(plan.reason, InterventionReason::SafeStructuralFix);
    assert_eq!(plan.candidate_id, Some(ranked[0].id));
    assert_eq!(plan.model_transition, None);
}

#[test]
fn unique_abbrev_ko_is_replace_unique_heuristic_when_compat_flag_on() {
    let snapshot = CompositionSnapshot::new(1, "ko".into(), "ko".into());
    let ranked = [abbrev_candidate("không")];
    let plan = plan_on(&snapshot, &ranked, &lex(&["không"]));
    assert_eq!(plan.action, InterventionAction::Replace);
    assert_eq!(plan.reason, InterventionReason::UniqueHeuristicAssist);
    assert_eq!(plan.model_transition, None);
}

#[test]
fn unique_fuzzy_khogn_is_replace_when_compat_flag_on() {
    let snapshot = khogn_snapshot();
    let ranked = [fuzzy_khogn()];
    let plan = plan_on(&snapshot, &ranked, &lex(&["không"]));
    assert_eq!(plan.action, InterventionAction::Replace);
    assert_eq!(plan.reason, InterventionReason::UniqueHeuristicAssist);
}

#[test]
fn diacritics_unique_is_display_suggestion_source_suggest_only() {
    let snapshot = CompositionSnapshot::new(1, "khong".into(), "khong".into());
    let ranked = [diacritics_candidate("không")];
    let plan = plan_on(&snapshot, &ranked, &lex(&["không"]));
    assert_eq!(plan.action, InterventionAction::DisplaySuggestion);
    assert_eq!(plan.reason, InterventionReason::SourceSuggestOnly);
}

#[test]
fn one_alphabetic_grapheme_is_none_token_too_short() {
    let snapshot = CompositionSnapshot::new(1, "dd".into(), "đ".into());
    let ranked = [fuzzy_khogn()];
    let plan = plan_on(&snapshot, &ranked, &empty_lexicon());
    assert_eq!(plan.action, InterventionAction::None);
    assert_eq!(plan.reason, InterventionReason::TokenTooShort);
    assert_eq!(plan.model_transition, None);
}

#[test]
fn a1_rendered_as_a_acute_is_token_too_short() {
    let snapshot = CompositionSnapshot::new(1, "a1".into(), "á".into());
    assert_eq!(alphabetic_grapheme_count(&snapshot.normalized), 1);
    let plan = plan_on(&snapshot, &[fuzzy_khogn()], &empty_lexicon());
    assert_eq!(plan.reason, InterventionReason::TokenTooShort);
}

#[test]
fn two_graphemes_are_eligible() {
    let snapshot = CompositionSnapshot::new(1, "ko".into(), "ko".into());
    assert_eq!(alphabetic_grapheme_count(&snapshot.normalized), 2);
    let plan = plan_on(&snapshot, &[abbrev_candidate("không")], &lex(&["không"]));
    assert_ne!(plan.reason, InterventionReason::TokenTooShort);
}
