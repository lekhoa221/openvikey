//! Pure candidate generators.
//!
//! Wave 0 locks the seam: a generator sees a composition snapshot and
//! left context only. It must not take a model. Concrete generators
//! (abbrev / telex_fix / fuzzy / diacritics) arrive in M5 and M7.

pub mod abbrev;

use crate::decision::{DecisionConfig, DecisionState, decide};
use crate::model::{ModelView, RuleContextKey};
use crate::rank::{ScoreConfig, rank};
use crate::types::{Candidate, CandidateSource, CompositionSnapshot, InputContext, InputMethod};

/// Tokens already committed to the left of the active composition.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LeftContext {
    pub prev_token_nfc: Option<String>,
}

/// Pure generator. Implementations must not read personal model state.
pub trait Generator {
    fn source(&self) -> CandidateSource;

    fn generate(
        &self,
        snapshot: &CompositionSnapshot,
        left_context: &LeftContext,
    ) -> Vec<Candidate>;
}

/// Entry point that honors `allow_transform`. Rank/decision must not run on the empty result.
#[must_use]
pub fn collect_candidates(
    snapshot: &CompositionSnapshot,
    left_context: &LeftContext,
    context: InputContext,
    generators: &[&dyn Generator],
) -> Vec<Candidate> {
    if !context.allow_transform {
        return Vec::new();
    }
    generators
        .iter()
        .flat_map(|generator| generator.generate(snapshot, left_context))
        .collect()
}

/// One entry: skip generate/rank/decide when transforms are disabled.
#[derive(Debug, Clone, PartialEq)]
pub struct CorrectionSlice {
    pub candidates: Vec<Candidate>,
    pub decision: Option<DecisionState>,
}

#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn run_correction_slice(
    snapshot: &CompositionSnapshot,
    left_context: &LeftContext,
    context: InputContext,
    generators: &[&dyn Generator],
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
        };
    }
    let candidates = rank(raw, model, evaluate_at_ms, score_config);
    let decision = candidates.first().map(|top| {
        let rule = RuleContextKey {
            input_method: InputMethod::Telex,
            source: top.source,
            original_nfc: snapshot.normalized.clone(),
            candidate_nfc: top.text.clone(),
            left_token_nfc: left_context.prev_token_nfc.clone(),
            source_rule_id: top.evidence.split('+').next().unwrap_or("").to_string(),
        };
        decide(
            model.state(&rule, evaluate_at_ms),
            top.final_score,
            model.confidence(&rule, evaluate_at_ms),
            0.0,
            top.source.max_action(),
            decision_config,
        )
    });
    CorrectionSlice {
        candidates,
        decision,
    }
}
