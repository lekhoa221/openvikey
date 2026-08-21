//! Single decision point for None / Suggest / Replace.

use crate::correction::InterventionConfig;
use crate::learning_config::LearningConfigV2;
use crate::lexicon::Lexicon;
use crate::model::ModelView;
use crate::types::{Candidate, CandidateSource, CompositionSnapshot, InputContext, InputMethod};
use serde::{Deserialize, Serialize};

/// Planner output action. Callers must not upgrade Suggest to Replace afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterventionAction {
    None,
    DisplaySuggestion,
    Replace,
}

/// Stable reason codes for tests and advanced UI. Production must not log tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterventionReason {
    NoCandidate,
    UnsafeContext,
    TokenTooShort,
    RevertGuardBypass,
    SourceSuggestOnly,
    LowScore,
    LowMargin,
    ExplicitlySuppressed,
    RecentRevertCooldown,
    SafeStructuralFix,
    UniqueHeuristicAssist,
    LearnedCorrection,
    ContextSupportedSuggestion,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScoreBreakdown {
    pub generator_base: f64,
    pub exact_correction: f64,
    pub unigram: f64,
    pub bigram: f64,
    pub recent_revert_penalty: f64,
    pub top1_top2_margin: f64,
    pub final_score: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoContract {
    pub required: bool,
    pub uses_original_rendered: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InterventionPlan {
    pub action: InterventionAction,
    pub reason: InterventionReason,
    pub candidate_id: Option<u64>,
    pub score_breakdown: ScoreBreakdown,
    pub undo_contract: UndoContract,
}

/// Exact original→candidate identity. Context (left token) is not part of this key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CorrectionIdentity {
    pub input_method: InputMethod,
    pub source: CandidateSource,
    pub original_nfc: String,
    pub candidate_nfc: String,
    pub source_rule_id: String,
}

/// Session-only short-term block after a semantic revert. Not persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevertGuard {
    pub identity: CorrectionIdentity,
    pub raw_token: String,
    pub focus_generation: u64,
    pub composition_revision: u64,
    pub reapply_cooldown_until_ms: i64,
    pub bypass_next_boundary: bool,
}

fn none_plan(reason: InterventionReason) -> InterventionPlan {
    InterventionPlan {
        action: InterventionAction::None,
        reason,
        candidate_id: None,
        score_breakdown: ScoreBreakdown::default(),
        undo_contract: UndoContract {
            required: false,
            uses_original_rendered: true,
        },
    }
}

/// Decide None / Suggest / Replace for one ranked candidate set.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn plan_intervention(
    _snapshot: &CompositionSnapshot,
    ranked: &[Candidate],
    _lexicon: &Lexicon,
    _model: &dyn ModelView,
    _config: &LearningConfigV2,
    _intervention: InterventionConfig,
    context: InputContext,
    _delimiter: Option<char>,
    _revert_guard: Option<&RevertGuard>,
    _evaluate_at_ms: i64,
    _auto_edit_valid: bool,
) -> InterventionPlan {
    if !context.allow_transform {
        return none_plan(InterventionReason::UnsafeContext);
    }
    if ranked.is_empty() {
        return none_plan(InterventionReason::NoCandidate);
    }
    none_plan(InterventionReason::LowScore)
}
