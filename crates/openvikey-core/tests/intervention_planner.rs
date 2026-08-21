//! Unified intervention planner — Lát 1 skeleton.

use openvikey_core::correction::InterventionConfig;
use openvikey_core::intervention::{InterventionAction, InterventionReason, plan_intervention};
use openvikey_core::learning_config::LearningConfigV2;
use openvikey_core::lexicon::Lexicon;
use openvikey_core::model::EmptyModel;
use openvikey_core::types::{Candidate, CandidateSource, CompositionSnapshot, InputContext};

fn empty_lexicon() -> Lexicon {
    Lexicon::from_entries([], [], Some("empty"))
}

fn fuzzy_khogn() -> Candidate {
    Candidate {
        id: 42,
        text: "không".into(),
        source: CandidateSource::Fuzzy,
        evidence: "fuzzy:khogn".into(),
        base_score: 0.9,
        final_score: 0.9,
    }
}

#[test]
fn empty_candidates_are_none_no_candidate() {
    let plan = plan_intervention(
        &CompositionSnapshot::new(1, "khogn".into(), "khogn".into()),
        &[],
        &empty_lexicon(),
        &EmptyModel,
        &LearningConfigV2::compatibility_v1(),
        InterventionConfig::win32(),
        InputContext::default(),
        Some(' '),
        None,
        0,
        true,
    );
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
    let plan = plan_intervention(
        &CompositionSnapshot::new(1, "khogn".into(), "khogn".into()),
        &[fuzzy_khogn()],
        &empty_lexicon(),
        &EmptyModel,
        &LearningConfigV2::compatibility_v1(),
        InterventionConfig::win32(),
        ctx,
        Some(' '),
        None,
        0,
        true,
    );
    assert_eq!(plan.action, InterventionAction::None);
    assert_eq!(plan.reason, InterventionReason::UnsafeContext);
}
