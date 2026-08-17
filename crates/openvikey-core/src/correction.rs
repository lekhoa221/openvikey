//! Correction pipeline orchestration.
//!
//! Generators remain pure; this module owns model-aware ranking and decision.

use crate::decision::{DecisionConfig, DecisionState, decide};
use crate::generate::{Generator, LeftContext, collect_candidates};
use crate::model::{ModelView, RuleContextKey};
use crate::rank::{RankingContext, ScoreConfig, rank};
use crate::types::{Candidate, CompositionSnapshot, EngineAction, InputContext, InputMethod};

/// Result of one generate → rank → decision pass.
#[derive(Debug, Clone, PartialEq)]
pub struct CorrectionSlice {
    pub candidates: Vec<Candidate>,
    pub decision: Option<DecisionState>,
    pub action: Option<EngineAction>,
}

/// Runs one correction pass and honors the caller's input method and context.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn run_correction_slice(
    snapshot: &CompositionSnapshot,
    left_context: &LeftContext,
    context: InputContext,
    generators: &[&dyn Generator],
    input_method: InputMethod,
    model: &dyn ModelView,
    evaluate_at_ms: i64,
    score_config: &ScoreConfig,
    decision_config: &DecisionConfig,
) -> CorrectionSlice {
    let raw = collect_candidates(snapshot, left_context, context, generators);
    if raw.is_empty() {
        return CorrectionSlice {
            candidates: Vec::new(),
            decision: None,
            action: None,
        };
    }

    let candidates = rank(
        raw,
        model,
        evaluate_at_ms,
        score_config,
        Some(&RankingContext {
            input_method,
            original_nfc: snapshot.normalized.clone(),
            left_token_nfc: left_context.prev_token_nfc.clone(),
        }),
    );
    let decision = candidates.first().map(|top| {
        let rule = RuleContextKey {
            input_method,
            source: top.source,
            original_nfc: snapshot.normalized.clone(),
            candidate_nfc: top.text.clone(),
            left_token_nfc: left_context.prev_token_nfc.clone(),
            source_rule_id: primary_rule_id(&top.evidence).to_string(),
        };
        decide(
            model.state(&rule, evaluate_at_ms),
            top.final_score,
            model.confidence(&rule, evaluate_at_ms),
            model.positive_mass(&rule, evaluate_at_ms),
            top.source.max_action(),
            decision_config,
        )
    });
    let action = match decision {
        Some(DecisionState::Suggest) => Some(EngineAction::ShowSuggestions {
            revision: snapshot.revision,
            candidates: candidates.clone(),
        }),
        Some(DecisionState::Ignore | DecisionState::Auto) | None => None,
    };

    CorrectionSlice {
        candidates,
        decision,
        action,
    }
}

fn primary_rule_id(evidence: &str) -> &str {
    evidence.split('+').next().unwrap_or("")
}
