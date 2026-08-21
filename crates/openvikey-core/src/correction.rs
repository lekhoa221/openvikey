//! Correction pipeline orchestration.
//!
//! Generators remain pure; this module owns model-aware ranking and decision.

use crate::decision::{DecisionConfig, DecisionState};
use crate::feedback::LearningSession;
use crate::generate::{Generator, LeftContext, collect_candidates};
use crate::intervention::{InterventionAction, InterventionPlan, plan_intervention};
use crate::learning_config::LearningConfigV2;
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
    pub plan: Option<InterventionPlan>,
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
        false,
        &Lexicon::empty(),
        InterventionConfig::default(),
        None,
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
    auto_edit_valid: bool,
    lexicon: &Lexicon,
    intervention: InterventionConfig,
    delimiter: Option<char>,
) -> CorrectionSlice {
    let raw = collect_candidates(snapshot, left_context, context, generators);
    if raw.is_empty() {
        return CorrectionSlice {
            candidates: Vec::new(),
            decision: None,
            action: None,
            plan: None,
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
    let mut learning = LearningConfigV2::compatibility_v1();
    learning.decision = decision_config.clone();
    learning.score = score_config.clone();
    let plan = plan_intervention(
        snapshot,
        &candidates,
        lexicon,
        model,
        &learning,
        intervention,
        context,
        delimiter,
        None,
        evaluate_at_ms,
        auto_edit_valid,
        input_method,
        left_context.prev_token_nfc.as_deref(),
    );
    let decision = if plan.action == InterventionAction::Replace {
        Some(DecisionState::Auto)
    } else {
        plan.model_transition
    };
    let action = match plan.action {
        InterventionAction::DisplaySuggestion => Some(EngineAction::ShowSuggestions {
            revision: snapshot.revision,
            candidates: candidates.clone(),
        }),
        InterventionAction::None | InterventionAction::Replace => None,
    };

    CorrectionSlice {
        candidates,
        decision,
        action,
        plan: Some(plan),
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
    lexicon: &Lexicon,
    intervention: InterventionConfig,
) -> CorrectionSlice {
    let auto_edit_valid = auto_edit.as_ref().is_some_and(|edit| {
        edit.range.revision == snapshot.revision
            && edit.range.length_grapheme == snapshot.rendered.graphemes(true).count()
    });
    let delimiter = auto_edit.as_ref().and_then(|edit| edit.delimiter);
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
        auto_edit_valid,
        lexicon,
        intervention,
        delimiter,
    );
    let Some(plan) = slice.plan.clone() else {
        return slice;
    };
    let Some(chosen) = plan
        .candidate_id
        .and_then(|id| slice.candidates.iter().find(|candidate| candidate.id == id))
        .or_else(|| slice.candidates.first())
        .cloned()
    else {
        return slice;
    };
    let rule = candidate_rule_key(snapshot, left_context, input_method, &chosen);

    if plan.action == InterventionAction::Replace {
        let valid_auto_edit = auto_edit.filter(|edit| {
            edit.range.revision == snapshot.revision
                && edit.range.length_grapheme == snapshot.rendered.graphemes(true).count()
        });
        if let Some(edit) = valid_auto_edit {
            let action = ReplaceRangeAction {
                edit_id: edit.edit_id,
                range: edit.range,
                original: snapshot.rendered.clone(),
                replacement: chosen.text.clone(),
                delimiter: edit.delimiter,
            };
            if let Some(state) = plan.model_transition {
                session.record_decision(&rule, state, context.allow_learning);
            }
            session.record_auto_edit(rule, action.clone(), evaluate_at_ms, context.allow_learning);
            slice.action = Some(EngineAction::ReplaceRange(action));
            slice.decision = Some(DecisionState::Auto);
        } else {
            slice.decision = Some(DecisionState::Suggest);
            slice.action = Some(EngineAction::ShowSuggestions {
                revision: snapshot.revision,
                candidates: slice.candidates.clone(),
            });
            if let Some(state) = plan.model_transition {
                session.record_decision(&rule, state, context.allow_learning);
            } else {
                session.record_decision(&rule, DecisionState::Suggest, context.allow_learning);
            }
        }
    } else if let Some(state) = plan.model_transition {
        session.record_decision(&rule, state, context.allow_learning);
    }
    slice
}

/// Stable learning key for a ranked candidate.
///
/// Rank may overwrite [`Candidate::source`] when NFC-identical candidates merge.
/// Telex/VNI reconstructions still identify as [`CandidateSource::TelexFix`] and keep
/// the `telex-fix:` / `vni-fix:` evidence id.
#[must_use]
pub fn candidate_rule_key(
    snapshot: &CompositionSnapshot,
    left_context: &LeftContext,
    input_method: InputMethod,
    candidate: &Candidate,
) -> RuleContextKey {
    RuleContextKey {
        input_method,
        source: if is_telex_fix_candidate(candidate) {
            CandidateSource::TelexFix
        } else {
            candidate.source
        },
        original_nfc: snapshot.normalized.clone(),
        candidate_nfc: candidate.text.clone(),
        left_token_nfc: left_context.prev_token_nfc.clone(),
        source_rule_id: telex_fix_rule_id(candidate)
            .unwrap_or_else(|| primary_rule_id(&candidate.evidence))
            .to_string(),
    }
}

fn telex_fix_rule_id(candidate: &Candidate) -> Option<&str> {
    candidate
        .evidence
        .split('+')
        .find(|part| part.contains("telex-fix:") || part.contains("vni-fix:"))
}

fn primary_rule_id(evidence: &str) -> &str {
    evidence.split('+').next().unwrap_or("")
}

/// True when a ranked candidate is a Telex/VNI misplaced-tone reconstruction.
///
/// Rank may keep a higher-scoring duplicate's [`CandidateSource`] after NFC merge,
/// so callers must not trust `source == TelexFix` alone.
#[must_use]
pub fn is_telex_fix_candidate(candidate: &Candidate) -> bool {
    candidate.source == CandidateSource::TelexFix
        || candidate
            .evidence
            .split('+')
            .any(|part| part.contains("telex-fix:") || part.contains("vni-fix:"))
}

/// The unique misplaced-tone reconstruction, if the ranked list has exactly one.
#[must_use]
pub fn unique_telex_fix_candidate(candidates: &[Candidate]) -> Option<&Candidate> {
    let mut unique = None;
    for candidate in candidates {
        if !is_telex_fix_candidate(candidate) {
            continue;
        }
        if unique.is_some() {
            return None;
        }
        unique = Some(candidate);
    }
    unique
}

/// Unique Space/punct replacement that may be applied without `Ctrl+.`.
///
/// TelexFix keeps its reconstruction rule (may not be ranked first). Abbreviation
/// applies only as a unique single-word top expansion. Fuzzy applies only when it
/// is the sole ranked candidate and the typed token is long enough to be a typo
/// rather than a guess. Diacritics and multi-word expansions never apply.
#[must_use]
pub fn boundary_assist_candidate<'a>(
    snapshot: &CompositionSnapshot,
    candidates: &'a [Candidate],
    delimiter: Option<char>,
    config: InterventionConfig,
    lexicon: &Lexicon,
    allow_transform: bool,
) -> Option<&'a Candidate> {
    crate::intervention::policy_assist_candidate(
        snapshot,
        candidates,
        delimiter,
        config,
        lexicon,
        allow_transform,
        &LearningConfigV2::compatibility_v1(),
    )
    .map(|(candidate, _)| candidate)
}
