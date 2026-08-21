//! Single decision point for None / Suggest / Replace.

use crate::correction::{InterventionConfig, unique_telex_fix_candidate};
use crate::decision::{ActionCap, DecisionState, decide};
use crate::learning_config::LearningConfigV2;
use crate::lexicon::Lexicon;
use crate::model::{ModelView, RuleContextKey};
use crate::rank::{RankingContext, score_contributions};
use crate::types::{Candidate, CandidateSource, CompositionSnapshot, InputContext, InputMethod};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use unicode_segmentation::UnicodeSegmentation;

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

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
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
    /// Ranked candidates the product may expose when `action` is Suggest.
    pub display_candidate_ids: Vec<u64>,
    pub score_breakdown: ScoreBreakdown,
    pub undo_contract: UndoContract,
    pub model_transition: Option<DecisionState>,
}

/// Exact original→candidate identity. Context (left token) is not part of this key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

/// Count NFC grapheme clusters that contain at least one alphabetic character.
#[must_use]
pub fn alphabetic_grapheme_count(normalized: &str) -> usize {
    normalized
        .graphemes(true)
        .filter(|grapheme| grapheme.chars().any(char::is_alphabetic))
        .count()
}

fn none_plan(reason: InterventionReason) -> InterventionPlan {
    InterventionPlan {
        action: InterventionAction::None,
        reason,
        candidate_id: None,
        display_candidate_ids: Vec::new(),
        score_breakdown: ScoreBreakdown::default(),
        undo_contract: UndoContract {
            required: false,
            uses_original_rendered: true,
        },
        model_transition: None,
    }
}

fn breakdown_for(candidate: &Candidate) -> ScoreBreakdown {
    ScoreBreakdown {
        generator_base: candidate.base_score,
        final_score: candidate.final_score,
        ..ScoreBreakdown::default()
    }
}

fn ranked_breakdown_for(
    candidate: &Candidate,
    model: &dyn ModelView,
    config: &LearningConfigV2,
    snapshot: &CompositionSnapshot,
    input_method: InputMethod,
    left_token_nfc: Option<&str>,
    evaluate_at_ms: i64,
) -> ScoreBreakdown {
    let contributions = score_contributions(
        candidate,
        model,
        evaluate_at_ms,
        &config.score,
        &RankingContext {
            input_method,
            original_nfc: snapshot.normalized.clone(),
            left_token_nfc: left_token_nfc.map(ToOwned::to_owned),
        },
    );
    ScoreBreakdown {
        generator_base: candidate.base_score,
        exact_correction: contributions.exact_correction,
        unigram: contributions.unigram,
        bigram: contributions.bigram,
        final_score: contributions.final_score,
        ..ScoreBreakdown::default()
    }
}

fn primary_rule_id(evidence: &str) -> &str {
    evidence.split('+').next().unwrap_or("")
}

/// Builds the context-free identity used by short-lived intervention guards.
#[must_use]
pub fn correction_identity(
    snapshot: &CompositionSnapshot,
    candidate: &Candidate,
    input_method: InputMethod,
) -> CorrectionIdentity {
    CorrectionIdentity {
        input_method,
        source: candidate.source,
        original_nfc: snapshot.normalized.clone(),
        candidate_nfc: candidate.text.clone(),
        source_rule_id: primary_rule_id(&candidate.evidence).to_string(),
    }
}

fn guard_matches_snapshot(guard: &RevertGuard, snapshot: &CompositionSnapshot) -> bool {
    guard.raw_token == snapshot.raw_keys && guard.composition_revision == snapshot.revision
}

fn guard_matches_candidate(
    guard: &RevertGuard,
    snapshot: &CompositionSnapshot,
    candidate: &Candidate,
    input_method: InputMethod,
) -> bool {
    guard_matches_snapshot(guard, snapshot)
        && guard.identity == correction_identity(snapshot, candidate, input_method)
}

fn policy_auto_allowed(
    delimiter: Option<char>,
    intervention: InterventionConfig,
    allow_transform: bool,
) -> bool {
    allow_transform
        && intervention.telex_fix_policy_auto
        && intervention.policy_delimiters.allows(delimiter)
}

fn lexicon_allows_replacement(
    lexicon: &Lexicon,
    snapshot: &CompositionSnapshot,
    candidate: &Candidate,
) -> bool {
    !lexicon.contains(&snapshot.normalized) && lexicon.contains(&candidate.text)
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

/// Unique-candidate heuristic assist.
///
/// Fuzzy assist requires the sole ranked candidate, so its top1–top2 margin is
/// vacuously 1.0. Abbrev cold-start uniqueness is scoped to Abbreviation-source
/// candidates only: a near-tie candidate from another source does not block the
/// replace, and the `auto_margin` guard is not consulted on this path (v1
/// compat; `product_v2` turns the flag off).
fn unique_heuristic_candidate<'a>(
    snapshot: &CompositionSnapshot,
    ranked: &'a [Candidate],
    lexicon: &Lexicon,
    config: &LearningConfigV2,
) -> Option<&'a Candidate> {
    let top = ranked.first()?;
    if !lexicon_allows_replacement(lexicon, snapshot, top) {
        return None;
    }
    if top.text == snapshot.normalized || top.text == snapshot.rendered {
        return None;
    }
    match top.source {
        CandidateSource::Abbreviation if config.abbrev_cold_start_auto => {
            unique_abbrev_expansion(top, ranked)
        }
        CandidateSource::Fuzzy if config.fuzzy_heuristic_assist => {
            if fuzzy_token_long_enough(snapshot) && ranked.len() == 1 {
                Some(top)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Shared picker for TelexFix structural auto and unique abbrev/fuzzy assist.
pub(crate) fn policy_assist_candidate<'a>(
    snapshot: &CompositionSnapshot,
    ranked: &'a [Candidate],
    delimiter: Option<char>,
    intervention: InterventionConfig,
    lexicon: &Lexicon,
    allow_transform: bool,
    config: &LearningConfigV2,
) -> Option<(&'a Candidate, InterventionReason)> {
    if !policy_auto_allowed(delimiter, intervention, allow_transform) {
        return None;
    }
    if let Some(fix) = unique_telex_fix_candidate(ranked)
        && lexicon_allows_replacement(lexicon, snapshot, fix)
    {
        return Some((fix, InterventionReason::SafeStructuralFix));
    }
    unique_heuristic_candidate(snapshot, ranked, lexicon, config)
        .map(|candidate| (candidate, InterventionReason::UniqueHeuristicAssist))
}

fn learned_replace(candidate: &Candidate, breakdown: ScoreBreakdown) -> InterventionPlan {
    InterventionPlan {
        action: InterventionAction::Replace,
        reason: InterventionReason::LearnedCorrection,
        candidate_id: Some(candidate.id),
        display_candidate_ids: Vec::new(),
        score_breakdown: breakdown,
        undo_contract: UndoContract {
            required: true,
            uses_original_rendered: true,
        },
        model_transition: Some(DecisionState::Auto),
    }
}

fn suggest_reason(
    recent_revert_blocked: bool,
    source_cap: ActionCap,
    state: DecisionState,
) -> InterventionReason {
    if recent_revert_blocked {
        InterventionReason::RecentRevertCooldown
    } else if source_cap == ActionCap::Suggest {
        InterventionReason::SourceSuggestOnly
    } else if state == DecisionState::Auto {
        InterventionReason::LearnedCorrection
    } else {
        InterventionReason::LowScore
    }
}

fn replace_without_persisting(
    reason: InterventionReason,
    candidate: &Candidate,
) -> InterventionPlan {
    InterventionPlan {
        action: InterventionAction::Replace,
        reason,
        candidate_id: Some(candidate.id),
        display_candidate_ids: Vec::new(),
        score_breakdown: breakdown_for(candidate),
        undo_contract: UndoContract {
            required: true,
            uses_original_rendered: true,
        },
        model_transition: None,
    }
}

fn rule_key(
    snapshot: &CompositionSnapshot,
    candidate: &Candidate,
    input_method: InputMethod,
    left_token_nfc: Option<&str>,
) -> RuleContextKey {
    RuleContextKey {
        input_method,
        source: candidate.source,
        original_nfc: snapshot.normalized.clone(),
        candidate_nfc: candidate.text.clone(),
        left_token_nfc: left_token_nfc.map(ToOwned::to_owned),
        source_rule_id: primary_rule_id(&candidate.evidence).to_string(),
    }
}

struct GuardedCandidates<'a> {
    ranked: Cow<'a, [Candidate]>,
    cooldown_active: bool,
    stop_plan: Option<InterventionPlan>,
}

fn guarded_candidates<'a>(
    snapshot: &CompositionSnapshot,
    ranked: &'a [Candidate],
    revert_guard: Option<&RevertGuard>,
    delimiter: Option<char>,
    evaluate_at_ms: i64,
    input_method: InputMethod,
) -> GuardedCandidates<'a> {
    let Some(guard) = revert_guard.filter(|guard| guard_matches_snapshot(guard, snapshot)) else {
        return GuardedCandidates {
            ranked: Cow::Borrowed(ranked),
            cooldown_active: false,
            stop_plan: None,
        };
    };
    let guarded_ids = ranked
        .iter()
        .filter(|candidate| guard_matches_candidate(guard, snapshot, candidate, input_method))
        .map(|candidate| candidate.id)
        .collect::<Vec<_>>();
    let explained_candidate = ranked
        .iter()
        .find(|candidate| guarded_ids.contains(&candidate.id))
        .or_else(|| ranked.first());
    if guard.bypass_next_boundary && delimiter.is_some() {
        let mut plan = none_plan(InterventionReason::RevertGuardBypass);
        if let Some(candidate) = explained_candidate {
            plan.candidate_id = Some(candidate.id);
            plan.score_breakdown = breakdown_for(candidate);
        }
        return GuardedCandidates {
            ranked: Cow::Borrowed(ranked),
            cooldown_active: true,
            stop_plan: Some(plan),
        };
    }
    if evaluate_at_ms >= guard.reapply_cooldown_until_ms {
        return GuardedCandidates {
            ranked: Cow::Borrowed(ranked),
            cooldown_active: false,
            stop_plan: None,
        };
    }
    let effective = ranked
        .iter()
        .filter(|candidate| !guarded_ids.contains(&candidate.id))
        .cloned()
        .collect::<Vec<_>>();
    let stop_plan = effective.is_empty().then(|| {
        let mut plan = none_plan(InterventionReason::RevertGuardBypass);
        if let Some(candidate) = explained_candidate {
            plan.candidate_id = Some(candidate.id);
            plan.score_breakdown = breakdown_for(candidate);
        }
        plan
    });
    GuardedCandidates {
        ranked: Cow::Owned(effective),
        cooldown_active: true,
        stop_plan,
    }
}

fn precondition_plan(
    snapshot: &CompositionSnapshot,
    ranked: &[Candidate],
    config: &LearningConfigV2,
    context: InputContext,
) -> Option<InterventionPlan> {
    if !context.allow_transform {
        return Some(none_plan(InterventionReason::UnsafeContext));
    }
    if alphabetic_grapheme_count(&snapshot.normalized) < config.minimum_correction_graphemes {
        let mut plan = none_plan(InterventionReason::TokenTooShort);
        if let Some(top) = ranked.first() {
            plan.candidate_id = Some(top.id);
            plan.score_breakdown = breakdown_for(top);
        }
        return Some(plan);
    }
    ranked
        .is_empty()
        .then(|| none_plan(InterventionReason::NoCandidate))
}

/// Decide None / Suggest / Replace for one ranked candidate set.
#[must_use]
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn plan_intervention(
    snapshot: &CompositionSnapshot,
    ranked: &[Candidate],
    lexicon: &Lexicon,
    model: &dyn ModelView,
    config: &LearningConfigV2,
    intervention: InterventionConfig,
    context: InputContext,
    delimiter: Option<char>,
    revert_guard: Option<&RevertGuard>,
    evaluate_at_ms: i64,
    auto_edit_valid: bool,
    input_method: InputMethod,
    left_token_nfc: Option<&str>,
) -> InterventionPlan {
    if let Some(plan) = precondition_plan(snapshot, ranked, config, context) {
        return plan;
    }
    let guarded = guarded_candidates(
        snapshot,
        ranked,
        revert_guard,
        delimiter,
        evaluate_at_ms,
        input_method,
    );
    if let Some(plan) = guarded.stop_plan {
        return plan;
    }
    let effective_ranked = guarded.ranked;
    let guard_cooldown_active = guarded.cooldown_active;
    let Some(top) = effective_ranked.first() else {
        return none_plan(InterventionReason::NoCandidate);
    };
    let assist = policy_assist_candidate(
        snapshot,
        &effective_ranked,
        delimiter,
        intervention,
        lexicon,
        context.allow_transform,
        config,
    );
    let rule = rule_key(snapshot, top, input_method, left_token_nfc);
    let source_cap = top.source.max_action();
    let recent_revert_blocked = !model.auto_allowed(&rule, evaluate_at_ms);
    let cap = if guard_cooldown_active || recent_revert_blocked {
        ActionCap::Suggest
    } else {
        source_cap
    };
    let state = decide(
        model.state(&rule, evaluate_at_ms),
        top.final_score,
        model.confidence(&rule, evaluate_at_ms),
        model.positive_mass(&rule, evaluate_at_ms),
        cap,
        &config.decision,
    );
    let mut breakdown = ranked_breakdown_for(
        top,
        model,
        config,
        snapshot,
        input_method,
        left_token_nfc,
        evaluate_at_ms,
    );
    breakdown.top1_top2_margin = effective_ranked.get(1).map_or(1.0, |second| {
        (top.final_score - second.final_score).max(0.0)
    });
    let structural = if !guard_cooldown_active && auto_edit_valid {
        assist.and_then(|(candidate, reason)| {
            (reason == InterventionReason::SafeStructuralFix).then_some(candidate)
        })
    } else {
        None
    };
    // Product v2 prioritizes deterministic structural repair. Compatibility v1
    // keeps the historical learned-before-structural order for replay.
    let structural_preempts_learned = config.version >= 2 && structural.is_some();
    // `auto_margin` guards only this learned-Auto branch; structural and
    // heuristic replaces never consult it.
    if !structural_preempts_learned
        && !guard_cooldown_active
        && state == DecisionState::Auto
        && auto_edit_valid
    {
        if breakdown.top1_top2_margin < config.auto_margin {
            return InterventionPlan {
                action: InterventionAction::DisplaySuggestion,
                reason: InterventionReason::LowMargin,
                candidate_id: Some(top.id),
                display_candidate_ids: effective_ranked
                    .iter()
                    .map(|candidate| candidate.id)
                    .collect(),
                score_breakdown: breakdown,
                undo_contract: UndoContract {
                    required: false,
                    uses_original_rendered: true,
                },
                model_transition: Some(DecisionState::Suggest),
            };
        }
        return learned_replace(top, breakdown);
    }
    if let Some(candidate) = structural {
        return replace_without_persisting(InterventionReason::SafeStructuralFix, candidate);
    }
    if !guard_cooldown_active
        && auto_edit_valid
        && let Some((candidate, InterventionReason::UniqueHeuristicAssist)) = assist
    {
        let assist_rule = rule_key(snapshot, candidate, input_method, left_token_nfc);
        if model.auto_allowed(&assist_rule, evaluate_at_ms) {
            return replace_without_persisting(
                InterventionReason::UniqueHeuristicAssist,
                candidate,
            );
        }
    }
    let reason = if guard_cooldown_active {
        InterventionReason::RevertGuardBypass
    } else {
        suggest_reason(recent_revert_blocked, source_cap, state)
    };
    match state {
        DecisionState::Auto | DecisionState::Suggest => InterventionPlan {
            action: InterventionAction::DisplaySuggestion,
            reason,
            candidate_id: Some(top.id),
            display_candidate_ids: effective_ranked
                .iter()
                .map(|candidate| candidate.id)
                .collect(),
            score_breakdown: breakdown,
            undo_contract: UndoContract {
                required: false,
                uses_original_rendered: true,
            },
            model_transition: Some(DecisionState::Suggest),
        },
        DecisionState::Ignore => InterventionPlan {
            action: InterventionAction::None,
            reason,
            candidate_id: Some(top.id),
            display_candidate_ids: Vec::new(),
            score_breakdown: breakdown,
            undo_contract: UndoContract {
                required: false,
                uses_original_rendered: true,
            },
            model_transition: Some(DecisionState::Ignore),
        },
    }
}
