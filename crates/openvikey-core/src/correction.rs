//! Correction pipeline orchestration.
//!
//! Generators remain pure; this module owns model-aware ranking and decision.

use crate::decision::{DecisionConfig, DecisionState, decide};
use crate::feedback::LearningSession;
use crate::generate::{Generator, LeftContext, collect_candidates};
use crate::lexicon::Lexicon;
use crate::model::{ModelView, RuleContextKey};
use crate::rank::{RankingContext, ScoreConfig, rank};
use crate::types::{
    Candidate, CandidateSource, CompositionSnapshot, EditRange, EngineAction, InputContext,
    InputMethod, ReplaceRangeAction,
};
use unicode_segmentation::UnicodeSegmentation;

/// Which token-ending delimiters may trigger TelexFix policy auto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDelimiters {
    SpaceOnly,
    SpaceAndPunctuation,
}

impl PolicyDelimiters {
    #[must_use]
    pub fn allows(self, delimiter: Option<char>) -> bool {
        match delimiter {
            Some(' ') => true,
            Some(ch)
                if matches!(self, Self::SpaceAndPunctuation)
                    && matches!(ch, '.' | ',' | ';' | ':' | '?' | '!') =>
            {
                true
            }
            _ => false,
        }
    }
}

/// Session-owned intervention policy. Not part of [`InputContext`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterventionConfig {
    pub telex_fix_policy_auto: bool,
    pub policy_delimiters: PolicyDelimiters,
}

impl Default for InterventionConfig {
    fn default() -> Self {
        Self {
            telex_fix_policy_auto: false,
            policy_delimiters: PolicyDelimiters::SpaceAndPunctuation,
        }
    }
}

impl InterventionConfig {
    #[must_use]
    pub fn win32() -> Self {
        Self {
            telex_fix_policy_auto: true,
            policy_delimiters: PolicyDelimiters::SpaceAndPunctuation,
        }
    }

    #[must_use]
    pub fn electron() -> Self {
        Self {
            telex_fix_policy_auto: true,
            policy_delimiters: PolicyDelimiters::SpaceOnly,
        }
    }
}

/// Result of one generate → rank → decision pass.
#[derive(Debug, Clone, PartialEq)]
pub struct CorrectionSlice {
    pub candidates: Vec<Candidate>,
    pub decision: Option<DecisionState>,
    pub action: Option<EngineAction>,
}

/// Caller-owned identity and semantic range for an auto replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoEditContext {
    pub edit_id: u64,
    pub range: EditRange,
    pub delimiter: Option<char>,
}

/// Runs a read-only correction pass. Auto decisions safely degrade to suggestions
/// because this seam has no caller-owned edit identity/range for a self-contained replace.
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
    let mut slice = evaluate_correction_slice(
        snapshot,
        left_context,
        context,
        generators,
        input_method,
        model,
        evaluate_at_ms,
        score_config,
        decision_config,
    );
    if slice.decision == Some(DecisionState::Auto) {
        slice.decision = Some(DecisionState::Suggest);
        slice.action = Some(EngineAction::ShowSuggestions {
            revision: snapshot.revision,
            candidates: slice.candidates.clone(),
        });
    }
    slice
}

#[allow(clippy::too_many_arguments)]
fn evaluate_correction_slice(
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
            if model.auto_allowed(&rule, evaluate_at_ms) {
                top.source.max_action()
            } else {
                crate::decision::ActionCap::Suggest
            },
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

/// Runs correction against an adaptive session and records state/undo atomically in memory.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn run_learning_correction_slice(
    snapshot: &CompositionSnapshot,
    left_context: &LeftContext,
    context: InputContext,
    generators: &[&dyn Generator],
    input_method: InputMethod,
    session: &mut LearningSession,
    evaluate_at_ms: i64,
    score_config: &ScoreConfig,
    decision_config: &DecisionConfig,
    auto_edit: Option<AutoEditContext>,
) -> CorrectionSlice {
    let mut slice = evaluate_correction_slice(
        snapshot,
        left_context,
        context,
        generators,
        input_method,
        session.model(),
        evaluate_at_ms,
        score_config,
        decision_config,
    );
    let Some(top) = slice.candidates.first() else {
        return slice;
    };
    let rule = rule_key(snapshot, left_context, input_method, top);

    if slice.decision == Some(DecisionState::Auto) {
        let valid_auto_edit = auto_edit.filter(|edit| {
            edit.range.revision == snapshot.revision
                && edit.range.length_grapheme == snapshot.rendered.graphemes(true).count()
        });
        if let Some(edit) = valid_auto_edit {
            let action = ReplaceRangeAction {
                edit_id: edit.edit_id,
                range: edit.range,
                original: snapshot.rendered.clone(),
                replacement: top.text.clone(),
                delimiter: edit.delimiter,
            };
            session.record_decision(&rule, DecisionState::Auto, context.allow_learning);
            session.record_auto_edit(rule, action.clone(), evaluate_at_ms, context.allow_learning);
            slice.action = Some(EngineAction::ReplaceRange(action));
        } else {
            slice.decision = Some(DecisionState::Suggest);
            slice.action = Some(EngineAction::ShowSuggestions {
                revision: snapshot.revision,
                candidates: slice.candidates.clone(),
            });
            session.record_decision(&rule, DecisionState::Suggest, context.allow_learning);
        }
    } else if let Some(state) = slice.decision {
        session.record_decision(&rule, state, context.allow_learning);
    }
    slice
}

fn rule_key(
    snapshot: &CompositionSnapshot,
    left_context: &LeftContext,
    input_method: InputMethod,
    candidate: &Candidate,
) -> RuleContextKey {
    RuleContextKey {
        input_method,
        source: candidate.source,
        original_nfc: snapshot.normalized.clone(),
        candidate_nfc: candidate.text.clone(),
        left_token_nfc: left_context.prev_token_nfc.clone(),
        source_rule_id: primary_rule_id(&candidate.evidence).to_string(),
    }
}

fn primary_rule_id(evidence: &str) -> &str {
    evidence.split('+').next().unwrap_or("")
}

fn is_telex_fix_candidate(candidate: &Candidate) -> bool {
    candidate.source == CandidateSource::TelexFix
        || candidate
            .evidence
            .split('+')
            .any(|part| part.contains("telex-fix:") || part.contains("vni-fix:"))
}

/// TelexFix cold-start auto: unique reconstruction, lexicon gates, delimiter policy.
#[must_use]
pub fn telex_fix_policy_applies(
    snapshot: &CompositionSnapshot,
    candidates: &[Candidate],
    delimiter: Option<char>,
    config: InterventionConfig,
    lexicon: &Lexicon,
    allow_transform: bool,
) -> bool {
    if !allow_transform || !config.telex_fix_policy_auto {
        return false;
    }
    if !config.policy_delimiters.allows(delimiter) {
        return false;
    }
    let Some(top) = candidates.first() else {
        return false;
    };
    if !is_telex_fix_candidate(top) {
        return false;
    }
    let telex_count = candidates
        .iter()
        .filter(|candidate| is_telex_fix_candidate(candidate))
        .count();
    if telex_count != 1 {
        return false;
    }
    if candidates
        .get(1)
        .is_some_and(|second| second.text == top.text)
    {
        return false;
    }
    if lexicon.contains(&snapshot.normalized) {
        return false;
    }
    lexicon.contains(&top.text)
}
