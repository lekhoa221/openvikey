//! Unified intervention planner — Lát 1.

use openvikey_core::correction::InterventionConfig;
use openvikey_core::intervention::{InterventionAction, InterventionReason, plan_intervention};
use openvikey_core::learning_config::LearningConfigV2;
use openvikey_core::lexicon::Lexicon;
use openvikey_core::model::{AdaptiveModel, EmptyModel, ModelView, RuleContextKey};
use openvikey_core::types::{
    Candidate, CandidateSource, CompositionSnapshot, FeedbackEvent, FeedbackKind, InputContext,
    InputMethod,
};

fn empty_lexicon() -> Lexicon {
    Lexicon::from_entries([], [], Some("empty"))
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
}

#[test]
fn learned_auto_without_valid_edit_degrades_to_suggestion() {
    let model = model_with_accepts(18);
    let plan = plan_with(&[fuzzy_khogn()], &model, InputContext::default(), false);
    assert_eq!(plan.action, InterventionAction::DisplaySuggestion);
    assert_eq!(plan.reason, InterventionReason::LearnedCorrection);
}
