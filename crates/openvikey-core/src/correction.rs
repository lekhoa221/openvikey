//! Correction pipeline orchestration.
//!
//! Generators remain pure; this module owns model-aware ranking and decision.

use crate::decision::{DecisionConfig, DecisionState};
use crate::feedback::LearningSession;
use crate::generate::{Generator, LeftContext, collect_candidates};
use crate::intervention::{InterventionAction, plan_intervention};
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
    let mut learning = LearningConfigV2::compatibility_v1();
    learning.decision = decision_config.clone();
    learning.score = score_config.clone();
    let plan = plan_intervention(
        snapshot,
        &candidates,
        &Lexicon::empty(),
        model,
        &learning,
        InterventionConfig::default(),
        context,
        None,
        None,
        evaluate_at_ms,
        auto_edit_valid,
        input_method,
        left_context.prev_token_nfc.as_deref(),
    );
    let decision = match plan.action {
        InterventionAction::None if candidates.is_empty() => None,
        InterventionAction::None => Some(DecisionState::Ignore),
        InterventionAction::DisplaySuggestion => Some(DecisionState::Suggest),
        InterventionAction::Replace => Some(DecisionState::Auto),
    };
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
    let auto_edit_valid = auto_edit.as_ref().is_some_and(|edit| {
        edit.range.revision == snapshot.revision
            && edit.range.length_grapheme == snapshot.rendered.graphemes(true).count()
    });
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
    );
    let Some(top) = slice.candidates.first() else {
        return slice;
    };
    let rule = candidate_rule_key(snapshot, left_context, input_method, top);

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
    if !policy_auto_allowed(delimiter, config, allow_transform) {
        return None;
    }
    if let Some(fix) = unique_telex_fix_candidate(candidates)
        && lexicon_allows_replacement(lexicon, snapshot, fix)
    {
        return Some(fix);
    }
    unique_top_assist_candidate(snapshot, candidates, lexicon)
}

fn policy_auto_allowed(
    delimiter: Option<char>,
    config: InterventionConfig,
    allow_transform: bool,
) -> bool {
    allow_transform && config.telex_fix_policy_auto && config.policy_delimiters.allows(delimiter)
}

fn lexicon_allows_replacement(
    lexicon: &Lexicon,
    snapshot: &CompositionSnapshot,
    candidate: &Candidate,
) -> bool {
    !lexicon.contains(&snapshot.normalized) && lexicon.contains(&candidate.text)
}

fn unique_top_assist_candidate<'a>(
    snapshot: &CompositionSnapshot,
    candidates: &'a [Candidate],
    lexicon: &Lexicon,
) -> Option<&'a Candidate> {
    let top = candidates.first()?;
    if !lexicon_allows_replacement(lexicon, snapshot, top) {
        return None;
    }
    if top.text == snapshot.normalized || top.text == snapshot.rendered {
        return None;
    }
    match top.source {
        CandidateSource::Abbreviation => unique_abbrev_expansion(top, candidates),
        CandidateSource::Fuzzy => {
            if fuzzy_token_long_enough(snapshot) && candidates.len() == 1 {
                Some(top)
            } else {
                None
            }
        }
        CandidateSource::TelexFix | CandidateSource::Diacritics | CandidateSource::Personal => None,
    }
}

fn fuzzy_token_long_enough(snapshot: &CompositionSnapshot) -> bool {
    snapshot
        .normalized
        .chars()
        .filter(|ch| ch.is_alphabetic())
        .count()
        >= 4
}

fn unique_abbrev_expansion<'a>(
    top: &'a Candidate,
    candidates: &'a [Candidate],
) -> Option<&'a Candidate> {
    if top.text.split_whitespace().nth(1).is_some() {
        return None;
    }
    let mut unique_text: Option<&str> = None;
    for candidate in candidates {
        if candidate.source != CandidateSource::Abbreviation {
            continue;
        }
        match unique_text {
            None => unique_text = Some(candidate.text.as_str()),
            Some(text) if text == candidate.text => {}
            Some(_) => return None,
        }
    }
    (unique_text == Some(top.text.as_str())).then_some(top)
}
