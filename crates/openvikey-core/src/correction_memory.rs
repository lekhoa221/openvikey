//! Exact-correction evidence with support-aware left-context refinement.
//!
//! Each event is recorded once in the context-free bucket and once in its
//! optional left-token bucket. Queries blend those buckets; they never sum
//! global and contextual evidence or inherit state from a sibling context.

use crate::decision::{ActionCap, DecisionState};
use crate::intervention::CorrectionIdentity;
use crate::types::{CandidateSource, FeedbackEvent, FeedbackKind, InputMethod};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::BTreeSet;

const DEFAULT_HALF_LIFE_MS: i64 = 30 * 24 * 60 * 60 * 1_000;
const IMPLICIT_CORRECTION_MASS: f64 = 1.5;
const SETTLEMENT_ADD: f64 = 0.3;
const PERSONAL_PROMOTE_K: u32 = 2;
const MAX_PERSONAL_TRANSACTIONS: usize = 64;
const PERSONAL_RULE_ID: &str = "personal-correction";

const fn default_half_life_ms() -> i64 {
    DEFAULT_HALF_LIFE_MS
}

/// One non-negative evidence delta identified by its replay sequence.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CorrectionEvidence {
    pub seq: u64,
    pub at_ms: i64,
    pub positive: f64,
    pub negative: f64,
}

/// Durable identity of one independently observed Personal correction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PersonalTransaction {
    pub anchor: u64,
    pub at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
struct EvidenceSummary {
    positive_at_checkpoint: f64,
    negative_at_checkpoint: f64,
    checkpoint_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AutoEmission {
    edit_id: u64,
    at_ms: i64,
    undone: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct EvidenceBucket {
    state: DecisionState,
    #[serde(default)]
    summary: EvidenceSummary,
    events: Vec<CorrectionEvidence>,
    #[serde(default)]
    handled_sequences: BTreeSet<u64>,
    #[serde(default)]
    recent_auto: Vec<AutoEmission>,
    #[serde(default)]
    settled_auto_ids: BTreeSet<u64>,
    #[serde(default)]
    weak_positive_total: f64,
    #[serde(default)]
    settled_suggestion_ids: BTreeSet<u64>,
    #[serde(default)]
    auto_demoted_at_seq: Option<u64>,
}

impl Default for EvidenceBucket {
    fn default() -> Self {
        Self {
            state: DecisionState::Ignore,
            summary: EvidenceSummary::default(),
            events: Vec::new(),
            handled_sequences: BTreeSet::new(),
            recent_auto: Vec::new(),
            settled_auto_ids: BTreeSet::new(),
            weak_positive_total: 0.0,
            settled_suggestion_ids: BTreeSet::new(),
            auto_demoted_at_seq: None,
        }
    }
}

impl EvidenceBucket {
    fn apply(&mut self, evidence: CorrectionEvidence) {
        if !self.handled_sequences.insert(evidence.seq) {
            return;
        }
        let evidence = CorrectionEvidence {
            positive: finite_non_negative(evidence.positive),
            negative: finite_non_negative(evidence.negative),
            ..evidence
        };
        if evidence.positive > 0.0 || evidence.negative > 0.0 {
            self.events.push(evidence);
            self.events.sort_by_key(|event| (event.at_ms, event.seq));
        }
    }

    fn mass(&self, evaluate_at_ms: i64, half_life_ms: i64) -> (f64, f64) {
        let summary_factor = self.summary.checkpoint_at_ms.map_or(0.0, |checkpoint| {
            decay_factor(checkpoint, evaluate_at_ms, half_life_ms)
        });
        self.events.iter().fold(
            (
                self.summary.positive_at_checkpoint * summary_factor,
                self.summary.negative_at_checkpoint * summary_factor,
            ),
            |mass, event| {
                let factor = decay_factor(event.at_ms, evaluate_at_ms, half_life_ms);
                (
                    mass.0 + event.positive * factor,
                    mass.1 + event.negative * factor,
                )
            },
        )
    }

    fn mass_through(&self, evaluate_at_ms: i64, half_life_ms: i64, through_seq: u64) -> (f64, f64) {
        let summary_factor = self.summary.checkpoint_at_ms.map_or(0.0, |checkpoint| {
            chart_checkpoint_factor(checkpoint, evaluate_at_ms, half_life_ms)
        });
        self.events
            .iter()
            .filter(|event| (event.at_ms, event.seq) <= (evaluate_at_ms, through_seq))
            .fold(
                (
                    self.summary.positive_at_checkpoint * summary_factor,
                    self.summary.negative_at_checkpoint * summary_factor,
                ),
                |mass, event| {
                    let factor = decay_factor(event.at_ms, evaluate_at_ms, half_life_ms);
                    (
                        mass.0 + event.positive * factor,
                        mass.1 + event.negative * factor,
                    )
                },
            )
    }

    fn raw_mass(&self) -> (f64, f64) {
        self.events.iter().fold(
            (
                self.summary.positive_at_checkpoint,
                self.summary.negative_at_checkpoint,
            ),
            |mass, event| (mass.0 + event.positive, mass.1 + event.negative),
        )
    }

    fn compact_at(&mut self, evaluate_at_ms: i64, max_recent_events: usize, half_life_ms: i64) {
        if self.events.len() <= max_recent_events {
            return;
        }
        let excess = self.events.len() - max_recent_events;
        let summary_factor = self.summary.checkpoint_at_ms.map_or(0.0, |checkpoint| {
            decay_factor(checkpoint, evaluate_at_ms, half_life_ms)
        });
        let mut positive = self.summary.positive_at_checkpoint * summary_factor;
        let mut negative = self.summary.negative_at_checkpoint * summary_factor;
        for event in &self.events[..excess] {
            let factor = decay_factor(event.at_ms, evaluate_at_ms, half_life_ms);
            positive += event.positive * factor;
            negative += event.negative * factor;
        }
        self.summary = EvidenceSummary {
            positive_at_checkpoint: positive,
            negative_at_checkpoint: negative,
            checkpoint_at_ms: Some(evaluate_at_ms),
        };
        self.events.drain(..excess);
        trim_set(
            &mut self.handled_sequences,
            max_recent_events.saturating_mul(2),
        );
    }

    fn record_auto_emission(&mut self, edit_id: u64, at_ms: i64, undo_window: usize) {
        if self.recent_auto.iter().any(|item| item.edit_id == edit_id) {
            return;
        }
        self.recent_auto.push(AutoEmission {
            edit_id,
            at_ms,
            undone: false,
        });
        trim_vec_front(&mut self.recent_auto, undo_window);
    }

    fn mark_undo(&mut self, edit_id: u64, seq: u64) {
        if let Some(emission) = self
            .recent_auto
            .iter_mut()
            .find(|emission| emission.edit_id == edit_id)
        {
            emission.undone = true;
        }
        if self.recent_auto.iter().filter(|item| item.undone).count() >= 2 {
            self.state = DecisionState::Suggest;
            self.auto_demoted_at_seq.get_or_insert(seq);
        }
    }

    fn auto_allowed(&self, evaluate_at_ms: i64, half_life_ms: i64) -> bool {
        let Some(demoted_at_seq) = self.auto_demoted_at_seq else {
            return true;
        };
        self.events
            .iter()
            .filter(|event| event.seq > demoted_at_seq)
            .map(|event| {
                event.positive * decay_factor(event.at_ms, evaluate_at_ms, half_life_ms.max(1))
            })
            .sum::<f64>()
            >= 18.0
    }

    fn last_activity_at_ms(&self) -> Option<i64> {
        self.events
            .iter()
            .map(|event| event.at_ms)
            .chain(self.recent_auto.iter().map(|emission| emission.at_ms))
            .chain(self.summary.checkpoint_at_ms)
            .max()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct ContextBucket {
    left_token_nfc: String,
    evidence: EvidenceBucket,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CorrectionRow {
    identity: CorrectionIdentity,
    global: EvidenceBucket,
    contexts: Vec<ContextBucket>,
    #[serde(default, rename = "personal_observation_count", skip_serializing)]
    legacy_personal_observation_count: u32,
    #[serde(default)]
    personal_transactions: Vec<PersonalTransaction>,
    #[serde(default)]
    personal_promoted: bool,
    #[serde(default)]
    shown_count: u64,
    #[serde(default)]
    selected_count: u64,
    #[serde(default)]
    last_shown_at_ms: Option<i64>,
    #[serde(default)]
    last_impression_seq: Option<u64>,
}

impl CorrectionRow {
    fn new(identity: CorrectionIdentity) -> Self {
        Self {
            identity,
            global: EvidenceBucket::default(),
            contexts: Vec::new(),
            legacy_personal_observation_count: 0,
            personal_transactions: Vec::new(),
            personal_promoted: false,
            shown_count: 0,
            selected_count: 0,
            last_shown_at_ms: None,
            last_impression_seq: None,
        }
    }

    fn context(&self, left_token_nfc: &str) -> Option<&EvidenceBucket> {
        self.contexts
            .binary_search_by(|bucket| bucket.left_token_nfc.as_str().cmp(left_token_nfc))
            .ok()
            .map(|index| &self.contexts[index].evidence)
    }

    fn context_mut(&mut self, left_token_nfc: &str) -> &mut EvidenceBucket {
        match self
            .contexts
            .binary_search_by(|bucket| bucket.left_token_nfc.as_str().cmp(left_token_nfc))
        {
            Ok(index) => &mut self.contexts[index].evidence,
            Err(index) => {
                self.contexts.insert(
                    index,
                    ContextBucket {
                        left_token_nfc: left_token_nfc.to_string(),
                        evidence: EvidenceBucket::default(),
                    },
                );
                &mut self.contexts[index].evidence
            }
        }
    }

    fn retention_state(&self) -> DecisionState {
        self.contexts
            .iter()
            .map(|context| context.evidence.state)
            .chain(std::iter::once(self.global.state))
            .max()
            .unwrap_or(DecisionState::Ignore)
    }

    fn has_demotion_marker(&self) -> bool {
        self.global.auto_demoted_at_seq.is_some()
            || self
                .contexts
                .iter()
                .any(|context| context.evidence.auto_demoted_at_seq.is_some())
    }
}

/// Owned read projection used by the payload facade and inspection UI.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CorrectionInspection {
    pub identity: CorrectionIdentity,
    pub left_token_nfc: Option<String>,
    pub state: DecisionState,
    pub evidence_count: usize,
    pub positive_evidence: f64,
    pub negative_evidence: f64,
    pub last_evidence_at_ms: Option<i64>,
    pub shown_count: u64,
    pub selected_count: u64,
    pub last_shown_at_ms: Option<i64>,
    pub recent_auto_count: usize,
    pub recent_undo_count: usize,
}

pub(crate) struct ImportedOperationalMetadata<'a> {
    pub recent_auto: &'a [(u64, i64, bool)],
    pub handled_sequences: &'a BTreeSet<u64>,
    pub settled_auto_ids: &'a BTreeSet<u64>,
    pub settled_suggestion_ids: &'a BTreeSet<u64>,
    pub auto_demoted_at_seq: Option<u64>,
}

/// Bounded payload namespace for exact-correction rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorrectionMemory {
    #[serde(default = "default_half_life_ms")]
    half_life_ms: i64,
    rows: Vec<CorrectionRow>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pruned_rows: u64,
}

impl Default for CorrectionMemory {
    fn default() -> Self {
        Self::with_half_life(DEFAULT_HALF_LIFE_MS)
    }
}

impl CorrectionMemory {
    pub(crate) fn with_half_life(half_life_ms: i64) -> Self {
        Self {
            half_life_ms: half_life_ms.max(1),
            rows: Vec::new(),
            pruned_rows: 0,
        }
    }

    /// Record evidence globally and, when supplied, in exactly one context bucket.
    pub fn apply(
        &mut self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
        evidence: CorrectionEvidence,
    ) {
        let row = self.row_mut(identity);
        if row.global.handled_sequences.contains(&evidence.seq) {
            return;
        }
        row.global.apply(evidence);
        if let Some(left_token_nfc) = left_token_nfc {
            row.context_mut(left_token_nfc).apply(evidence);
        }
    }

    /// Persist hysteresis state only for the queried bucket, never its siblings.
    pub fn record_state(
        &mut self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
        state: DecisionState,
    ) {
        let row = self.row_mut(identity);
        let bucket = bucket_mut(row, left_token_nfc);
        bucket.state = cap_state(identity, state);
        if bucket.state == DecisionState::Auto {
            bucket.auto_demoted_at_seq = None;
        }
    }

    /// Records one actually displayed suggestion without changing confidence evidence.
    pub fn record_impression(
        &mut self,
        identity: &CorrectionIdentity,
        seq: u64,
        at_ms: i64,
    ) -> bool {
        let row = self.row_mut(identity);
        if row
            .last_impression_seq
            .is_some_and(|last_seq| seq <= last_seq)
        {
            return false;
        }
        row.shown_count = row.shown_count.saturating_add(1);
        row.last_shown_at_ms = Some(row.last_shown_at_ms.map_or(at_ms, |last| last.max(at_ms)));
        row.last_impression_seq = Some(seq);
        true
    }

    /// Confidence blended toward global evidence according to context support.
    #[must_use]
    pub fn blended_confidence(
        &self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
        evaluate_at_ms: i64,
        shrinkage_k: f64,
    ) -> f64 {
        let Some(row) = self.row(identity) else {
            return 0.5;
        };
        let global_mass = row.global.mass(evaluate_at_ms, self.half_life_ms);
        let global_confidence = confidence(global_mass);
        let Some(context) = left_token_nfc.and_then(|left| row.context(left)) else {
            return global_confidence;
        };
        let context_mass = context.mass(evaluate_at_ms, self.half_life_ms);
        let support = context_mass.0 + context_mass.1;
        if support <= 0.0 {
            return global_confidence;
        }
        let weight = context_weight(support, shrinkage_k);
        weight * confidence(context_mass) + (1.0 - weight) * global_confidence
    }

    /// Positive/negative mass blended with the same support weight as confidence.
    #[must_use]
    pub fn blended_mass(
        &self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
        evaluate_at_ms: i64,
        shrinkage_k: f64,
    ) -> (f64, f64) {
        let Some(row) = self.row(identity) else {
            return (0.0, 0.0);
        };
        let global = row.global.mass(evaluate_at_ms, self.half_life_ms);
        let Some(context) = left_token_nfc.and_then(|left| row.context(left)) else {
            return global;
        };
        let contextual = context.mass(evaluate_at_ms, self.half_life_ms);
        let support = contextual.0 + contextual.1;
        if support <= 0.0 {
            return global;
        }
        let weight = context_weight(support, shrinkage_k);
        (
            weight * contextual.0 + (1.0 - weight) * global.0,
            weight * contextual.1 + (1.0 - weight) * global.1,
        )
    }

    /// Move old events into a decayed checkpoint summary for every bucket.
    pub fn compact_at(&mut self, evaluate_at_ms: i64, max_recent_events: usize) {
        for row in &mut self.rows {
            row.global
                .compact_at(evaluate_at_ms, max_recent_events, self.half_life_ms);
            for context in &mut row.contexts {
                context
                    .evidence
                    .compact_at(evaluate_at_ms, max_recent_events, self.half_life_ms);
            }
        }
    }

    /// Number of raw events still retained in one queried bucket.
    #[must_use]
    pub fn recent_event_count(
        &self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
    ) -> usize {
        self.bucket(identity, left_token_nfc)
            .map_or(0, |bucket| bucket.events.len())
    }

    /// Global-bucket events plus compaction checkpoint time for chart building.
    ///
    /// Settings/idle read path only; never called from the hook path.
    pub(crate) fn chart_source(
        &self,
        identity: &CorrectionIdentity,
    ) -> Option<(Vec<CorrectionEvidence>, Option<i64>)> {
        let row = self.row(identity)?;
        Some((
            row.global.events.clone(),
            row.global.summary.checkpoint_at_ms,
        ))
    }

    /// Causal confidence at one retained event, excluding later sequences.
    #[must_use]
    pub(crate) fn chart_confidence_through(
        &self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
        evaluate_at_ms: i64,
        through_seq: u64,
        shrinkage_k: f64,
    ) -> (f64, f64) {
        let Some(row) = self.row(identity) else {
            return (0.5, 0.5);
        };
        let global_mass = row
            .global
            .mass_through(evaluate_at_ms, self.half_life_ms, through_seq);
        let global_confidence = confidence(global_mass);
        let Some(context) = left_token_nfc.and_then(|left| row.context(left)) else {
            return (global_confidence, global_confidence);
        };
        let context_mass = context.mass_through(evaluate_at_ms, self.half_life_ms, through_seq);
        let support = context_mass.0 + context_mass.1;
        if support <= 0.0 {
            return (global_confidence, global_confidence);
        }
        let weight = context_weight(support, shrinkage_k);
        (
            global_confidence,
            weight * confidence(context_mass) + (1.0 - weight) * global_confidence,
        )
    }

    /// Stable SHA-256 over canonical in-memory ordering.
    #[must_use]
    pub fn stable_hash(&self) -> String {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        hex_lower(&Sha256::digest(bytes))
    }

    /// Return state for the requested bucket, capped by source policy.
    #[must_use]
    pub fn query_state(
        &self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
    ) -> DecisionState {
        self.bucket(identity, left_token_nfc)
            .map_or(DecisionState::Ignore, |bucket| {
                cap_state(identity, bucket.state)
            })
    }

    pub(crate) fn contains(&self, identity: &CorrectionIdentity) -> bool {
        self.row(identity).is_some()
    }

    pub(crate) fn prepare_for(
        &mut self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
        max_corrections: usize,
        max_context_rows: usize,
    ) {
        if self.row(identity).is_none() {
            while self.rows.len() >= max_corrections.max(1) {
                let victim = weakest_row_index(&self.rows)
                    .expect("a non-empty capped correction store has a victim");
                self.rows.remove(victim);
                self.pruned_rows = self.pruned_rows.saturating_add(1);
            }
        }
        if let Some(left) = left_token_nfc {
            let context_exists = self
                .row(identity)
                .and_then(|row| row.context(left))
                .is_some();
            if !context_exists {
                self.evict_contexts_to_fit(max_context_rows.max(1));
            }
        }
    }

    pub(crate) fn apply_feedback(
        &mut self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
        event: &FeedbackEvent,
        max_events: usize,
        undo_window: usize,
        weak_positive_cap: f64,
    ) -> (f64, f64) {
        if self
            .row(identity)
            .is_some_and(|row| row.global.handled_sequences.contains(&event.seq))
        {
            return (0.0, 0.0);
        }
        let delta = self.feedback_delta(
            identity,
            left_token_nfc,
            event,
            finite_non_negative(weak_positive_cap),
        );
        self.apply(
            identity,
            left_token_nfc,
            CorrectionEvidence {
                seq: event.seq,
                at_ms: event.at_ms,
                positive: delta.0,
                negative: delta.1,
            },
        );
        self.trim_operational(
            identity,
            left_token_nfc,
            max_events.max(1),
            undo_window.max(1),
        );
        delta
    }

    /// Records one auto emission into the bounded undo window.
    pub(crate) fn record_auto_emission(
        &mut self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
        edit_id: u64,
        at_ms: i64,
        undo_window: usize,
    ) {
        let row = self.row_mut(identity);
        row.global
            .record_auto_emission(edit_id, at_ms, undo_window.max(1));
        if let Some(left) = left_token_nfc {
            row.context_mut(left)
                .record_auto_emission(edit_id, at_ms, undo_window.max(1));
        }
    }

    /// Records the operational veto from an immediate revert without adding evidence mass.
    pub(crate) fn record_immediate_revert(
        &mut self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
        edit_id: u64,
        seq: u64,
    ) {
        let Some(row) = self
            .rows
            .binary_search_by(|row| row.identity.cmp(identity))
            .ok()
            .map(|index| &mut self.rows[index])
        else {
            return;
        };
        row.global.mark_undo(edit_id, seq);
        if let Some(context) = left_token_nfc.and_then(|left| {
            row.contexts
                .iter_mut()
                .find(|context| context.left_token_nfc == left)
        }) {
            context.evidence.mark_undo(edit_id, seq);
        }
    }

    pub(crate) fn raw_totals(
        &self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
    ) -> (f64, f64) {
        self.bucket(identity, left_token_nfc)
            .map_or((0.0, 0.0), EvidenceBucket::raw_mass)
    }

    pub(crate) fn auto_allowed(
        &self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
        evaluate_at_ms: i64,
    ) -> bool {
        self.bucket(identity, left_token_nfc)
            .is_none_or(|bucket| bucket.auto_allowed(evaluate_at_ms, self.half_life_ms))
    }

    pub(crate) fn inspection_rows(&self) -> Vec<CorrectionInspection> {
        let mut rows = Vec::new();
        for row in &self.rows {
            // The global bucket duplicates all contextual evidence by design.
            // Expose it only when no context row exists, avoiding a duplicate UI row.
            if row.contexts.is_empty() {
                push_inspection(&mut rows, row, None, &row.global);
            } else {
                for context in &row.contexts {
                    push_inspection(
                        &mut rows,
                        row,
                        Some(context.left_token_nfc.clone()),
                        &context.evidence,
                    );
                }
            }
        }
        rows
    }

    #[must_use]
    pub const fn pruned_row_count(&self) -> u64 {
        self.pruned_rows
    }

    pub(crate) fn max_recorded_edit_id(&self) -> u64 {
        self.rows
            .iter()
            .flat_map(|row| {
                std::iter::once(&row.global)
                    .chain(row.contexts.iter().map(|context| &context.evidence))
            })
            .fold(0, |maximum, bucket| {
                let auto_max = bucket
                    .recent_auto
                    .iter()
                    .map(|emission| emission.edit_id)
                    .max()
                    .unwrap_or(0);
                let settled_max = bucket.settled_auto_ids.last().copied().unwrap_or(0);
                maximum.max(auto_max).max(settled_max)
            })
    }

    /// Remove a global correction row, all contexts, and all Personal metadata.
    pub fn forget_identity(&mut self, identity: &CorrectionIdentity) -> bool {
        let before = self.rows.len();
        self.rows.retain(|row| &row.identity != identity);
        self.rows.len() != before
    }

    /// Observe one independently anchored Personal correction transaction.
    pub fn observe_personal(
        &mut self,
        input_method: InputMethod,
        original_nfc: &str,
        replacement_nfc: &str,
        transaction: PersonalTransaction,
    ) -> bool {
        if original_nfc.is_empty() || replacement_nfc.is_empty() || original_nfc == replacement_nfc
        {
            return false;
        }
        let identity = personal_identity(
            input_method,
            original_nfc.to_string(),
            replacement_nfc.to_string(),
        );
        let half_life_ms = self.half_life_ms;
        let row = self.row_mut(&identity);
        if row
            .personal_transactions
            .iter()
            .any(|observed| observed.anchor == transaction.anchor)
        {
            return false;
        }
        row.personal_transactions.push(transaction);
        row.personal_transactions.sort_unstable();
        trim_vec_front(&mut row.personal_transactions, MAX_PERSONAL_TRANSACTIONS);
        row.global.apply(CorrectionEvidence {
            seq: transaction.anchor,
            at_ms: transaction.at_ms,
            positive: 1.0,
            negative: 0.0,
        });
        row.global
            .compact_at(transaction.at_ms, MAX_PERSONAL_TRANSACTIONS, half_life_ms);
        if row.personal_transactions.len() < PERSONAL_PROMOTE_K as usize || row.personal_promoted {
            return false;
        }
        row.personal_promoted = true;
        row.global.state = DecisionState::Suggest;
        true
    }

    pub(crate) fn observe_personal_bounded(
        &mut self,
        input_method: InputMethod,
        original_nfc: &str,
        replacement_nfc: &str,
        transaction: PersonalTransaction,
        max_personal_pairs: usize,
        max_corrections: usize,
    ) -> bool {
        let identity = personal_identity(
            input_method,
            original_nfc.to_string(),
            replacement_nfc.to_string(),
        );
        if self.row(&identity).is_none() {
            if !self.make_room_for_personal(max_personal_pairs.max(1)) {
                return false;
            }
            self.prepare_for(&identity, None, max_corrections, usize::MAX);
        }
        self.observe_personal(input_method, original_nfc, replacement_nfc, transaction)
    }

    pub(crate) fn import_personal(
        &mut self,
        input_method: InputMethod,
        original_nfc: &str,
        replacement_nfc: &str,
        count: u32,
        promoted: bool,
    ) {
        let required = if promoted {
            count.max(PERSONAL_PROMOTE_K)
        } else {
            count
        };
        let identity = personal_identity(
            input_method,
            original_nfc.to_string(),
            replacement_nfc.to_string(),
        );
        let row = self.row_mut(&identity);
        row.personal_transactions = synthetic_personal_transactions(required);
        row.personal_promoted = row.personal_transactions.len() >= PERSONAL_PROMOTE_K as usize;
        if row.personal_promoted {
            row.global.state = DecisionState::Suggest;
        }
    }

    /// Number of independent observations retained for one Personal pair.
    #[must_use]
    pub fn personal_observation_count(
        &self,
        input_method: InputMethod,
        original_nfc: &str,
        replacement_nfc: &str,
    ) -> u32 {
        let identity = personal_identity(
            input_method,
            original_nfc.to_string(),
            replacement_nfc.to_string(),
        );
        self.row(&identity).map_or(0, |row| {
            u32::try_from(row.personal_transactions.len()).unwrap_or(u32::MAX)
        })
    }

    /// Promoted Personal pairs for a pure, method-specific generator table.
    #[must_use]
    pub fn promoted_personal(&self, input_method: InputMethod) -> Vec<(String, String)> {
        self.rows
            .iter()
            .filter(|row| {
                row.identity.source == CandidateSource::Personal
                    && row.identity.input_method == input_method
                    && row.personal_promoted
            })
            .map(|row| {
                (
                    row.identity.original_nfc.clone(),
                    row.identity.candidate_nfc.clone(),
                )
            })
            .collect()
    }

    pub(crate) fn all_promoted_personal(&self) -> Vec<(InputMethod, String, String)> {
        self.rows
            .iter()
            .filter(|row| row.identity.source == CandidateSource::Personal && row.personal_promoted)
            .map(|row| {
                (
                    row.identity.input_method,
                    row.identity.original_nfc.clone(),
                    row.identity.candidate_nfc.clone(),
                )
            })
            .collect()
    }

    /// Source policy and demotion markers jointly gate Auto.
    #[must_use]
    pub fn allows_auto(&self, identity: &CorrectionIdentity, evaluate_at_ms: i64) -> bool {
        identity.source.max_action() == ActionCap::Auto
            && self.auto_allowed(identity, None, evaluate_at_ms)
    }

    pub(crate) fn forget_personal(
        &mut self,
        input_method: InputMethod,
        original_nfc: &str,
        replacement_nfc: &str,
    ) -> bool {
        self.forget_identity(&personal_identity(
            input_method,
            original_nfc.to_string(),
            replacement_nfc.to_string(),
        ))
    }

    pub(crate) fn import_operational_metadata(
        &mut self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
        metadata: &ImportedOperationalMetadata<'_>,
    ) {
        let row = self.row_mut(identity);
        import_bucket_metadata(&mut row.global, metadata);
        if let Some(left) = left_token_nfc {
            import_bucket_metadata(row.context_mut(left), metadata);
        }
    }

    pub(crate) fn import_impressions(&mut self, identity: &CorrectionIdentity, shown_count: u64) {
        let row = self.row_mut(identity);
        row.shown_count = row.shown_count.max(shown_count);
    }

    pub(crate) fn import_weak_positive_total(
        &mut self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
        total: f64,
    ) {
        let row = self.row_mut(identity);
        row.global.weak_positive_total = row.global.weak_positive_total.max(total);
        if let Some(left) = left_token_nfc {
            let context = row.context_mut(left);
            context.weak_positive_total = context.weak_positive_total.max(total);
        }
    }

    pub(crate) fn set_global_fallback_state(
        &mut self,
        identity: &CorrectionIdentity,
        state: DecisionState,
    ) {
        let row = self.row_mut(identity);
        if row.global.state == DecisionState::Ignore {
            row.global.state = cap_state(identity, state.min(DecisionState::Suggest));
        }
    }

    pub(crate) fn normalize_weak_positive_totals(&mut self, weak_positive_cap: f64) {
        let cap = finite_non_negative(weak_positive_cap);
        for row in &mut self.rows {
            let global_count = u32::try_from(row.global.settled_auto_ids.len()).unwrap_or(u32::MAX);
            row.global.weak_positive_total = row
                .global
                .weak_positive_total
                .max((f64::from(global_count) * SETTLEMENT_ADD).min(cap));
            for context in &mut row.contexts {
                let context_count =
                    u32::try_from(context.evidence.settled_auto_ids.len()).unwrap_or(u32::MAX);
                context.evidence.weak_positive_total = context
                    .evidence
                    .weak_positive_total
                    .max((f64::from(context_count) * SETTLEMENT_ADD).min(cap));
            }
        }
    }

    pub(crate) fn normalize_legacy_suggestion_settlements(&mut self) {
        for row in &mut self.rows {
            let mut legacy_ids = row.global.settled_suggestion_ids.clone();
            for context in &row.contexts {
                legacy_ids.extend(context.evidence.settled_suggestion_ids.iter().copied());
            }
            if legacy_ids.is_empty() {
                continue;
            }
            row.shown_count = row
                .shown_count
                .max(u64::try_from(legacy_ids.len()).unwrap_or(u64::MAX));
            remove_legacy_suggestion_mass(&mut row.global);
            for context in &mut row.contexts {
                remove_legacy_suggestion_mass(&mut context.evidence);
            }
        }
    }

    pub(crate) fn normalize_personal_transactions(&mut self) {
        for row in &mut self.rows {
            if row.identity.source != CandidateSource::Personal {
                continue;
            }
            let required = if row.personal_promoted {
                row.legacy_personal_observation_count
                    .max(PERSONAL_PROMOTE_K)
            } else {
                row.legacy_personal_observation_count
            };
            for transaction in synthetic_personal_transactions(required) {
                if row
                    .personal_transactions
                    .iter()
                    .any(|observed| observed.anchor == transaction.anchor)
                {
                    continue;
                }
                row.personal_transactions.push(transaction);
            }
            row.personal_transactions.sort_unstable();
            row.legacy_personal_observation_count = 0;
            if row.personal_transactions.len() >= PERSONAL_PROMOTE_K as usize {
                row.personal_promoted = true;
                row.global.state = DecisionState::Suggest;
            }
        }
    }

    pub(crate) fn enforce_limits(&mut self, max_corrections: usize, max_context_rows: usize) {
        while self.rows.len() > max_corrections.max(1) {
            let victim = weakest_row_index(&self.rows)
                .expect("an over-cap correction store has an eviction candidate");
            self.rows.remove(victim);
            self.pruned_rows = self.pruned_rows.saturating_add(1);
        }
        while self.context_count() > max_context_rows.max(1) {
            self.remove_weakest_context();
        }
    }

    fn feedback_delta(
        &mut self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
        event: &FeedbackEvent,
        weak_positive_cap: f64,
    ) -> (f64, f64) {
        match event.kind {
            FeedbackKind::Accept { .. } => {
                self.promote_suggest(identity, left_token_nfc);
                let row = self.row_mut(identity);
                row.selected_count = row.selected_count.saturating_add(1);
                (1.0, 0.0)
            }
            FeedbackKind::ImplicitCorrection { .. } => {
                self.promote_suggest(identity, left_token_nfc);
                (IMPLICIT_CORRECTION_MASS, 0.0)
            }
            FeedbackKind::ExplicitReject { .. } => (0.0, 1.0),
            FeedbackKind::Undo { edit_id } => {
                self.mark_undo(identity, left_token_nfc, edit_id, event.seq);
                (0.0, 1.5)
            }
            FeedbackKind::AutoSettled { edit_id } => {
                let row = self.row_mut(identity);
                if !row.global.settled_auto_ids.insert(edit_id) {
                    return (0.0, 0.0);
                }
                if let Some(left) = left_token_nfc {
                    row.context_mut(left).settled_auto_ids.insert(edit_id);
                }
                let remaining = (weak_positive_cap - row.global.weak_positive_total).max(0.0);
                let delta = SETTLEMENT_ADD.min(remaining);
                row.global.weak_positive_total += delta;
                if let Some(left) = left_token_nfc {
                    row.context_mut(left).weak_positive_total += delta;
                }
                (delta, 0.0)
            }
            FeedbackKind::SuggestionSettled { candidate_id } => {
                let row = self.row_mut(identity);
                let newly_seen = row.global.settled_suggestion_ids.insert(candidate_id);
                if newly_seen {
                    if let Some(left) = left_token_nfc {
                        row.context_mut(left)
                            .settled_suggestion_ids
                            .insert(candidate_id);
                    }
                    self.record_impression(identity, event.seq, event.at_ms);
                }
                (0.0, 0.0)
            }
        }
    }

    fn promote_suggest(&mut self, identity: &CorrectionIdentity, left_token_nfc: Option<&str>) {
        let row = self.row_mut(identity);
        if row.global.state == DecisionState::Ignore {
            row.global.state = DecisionState::Suggest;
        }
        if let Some(left) = left_token_nfc {
            let context = row.context_mut(left);
            if context.state == DecisionState::Ignore {
                context.state = DecisionState::Suggest;
            }
        }
    }

    fn mark_undo(
        &mut self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
        edit_id: u64,
        seq: u64,
    ) {
        let row = self.row_mut(identity);
        row.global.mark_undo(edit_id, seq);
        if let Some(left) = left_token_nfc {
            row.context_mut(left).mark_undo(edit_id, seq);
        }
    }

    fn trim_operational(
        &mut self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
        max_events: usize,
        undo_window: usize,
    ) {
        let row = self.row_mut(identity);
        trim_bucket_operational(&mut row.global, max_events, undo_window);
        if let Some(left) = left_token_nfc {
            trim_bucket_operational(row.context_mut(left), max_events, undo_window);
        }
    }

    fn bucket(
        &self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
    ) -> Option<&EvidenceBucket> {
        let row = self.row(identity)?;
        Some(
            left_token_nfc
                .and_then(|left| row.context(left))
                .unwrap_or(&row.global),
        )
    }

    fn row(&self, identity: &CorrectionIdentity) -> Option<&CorrectionRow> {
        self.rows
            .binary_search_by(|row| row.identity.cmp(identity))
            .ok()
            .map(|index| &self.rows[index])
    }

    fn row_mut(&mut self, identity: &CorrectionIdentity) -> &mut CorrectionRow {
        match self.rows.binary_search_by(|row| row.identity.cmp(identity)) {
            Ok(index) => &mut self.rows[index],
            Err(index) => {
                self.rows
                    .insert(index, CorrectionRow::new(identity.clone()));
                &mut self.rows[index]
            }
        }
    }

    fn context_count(&self) -> usize {
        self.rows.iter().map(|row| row.contexts.len()).sum()
    }

    fn evict_contexts_to_fit(&mut self, max_context_rows: usize) {
        while self.context_count() >= max_context_rows {
            self.remove_weakest_context();
        }
    }

    fn remove_weakest_context(&mut self) {
        let victim = self
            .rows
            .iter()
            .enumerate()
            .flat_map(|(row_index, row)| {
                row.contexts
                    .iter()
                    .enumerate()
                    .map(move |(context_index, context)| (row_index, context_index, row, context))
            })
            .min_by(|left, right| context_priority(left.2, left.3, right.2, right.3))
            .map(|(row_index, context_index, _, _)| (row_index, context_index))
            .expect("a non-empty context store has an eviction candidate");
        self.rows[victim.0].contexts.remove(victim.1);
        self.pruned_rows = self.pruned_rows.saturating_add(1);
    }

    fn make_room_for_personal(&mut self, max_personal_pairs: usize) -> bool {
        let personal_count = self
            .rows
            .iter()
            .filter(|row| row.identity.source == CandidateSource::Personal)
            .count();
        if personal_count < max_personal_pairs {
            return true;
        }
        let victim = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                row.identity.source == CandidateSource::Personal && !row.personal_promoted
            })
            .min_by(|(_, left), (_, right)| {
                left.personal_transactions
                    .len()
                    .cmp(&right.personal_transactions.len())
                    .then_with(|| left.identity.cmp(&right.identity))
            })
            .map(|(index, _)| index);
        let Some(victim) = victim else {
            return false;
        };
        self.rows.remove(victim);
        self.pruned_rows = self.pruned_rows.saturating_add(1);
        true
    }
}

fn remove_legacy_suggestion_mass(bucket: &mut EvidenceBucket) {
    if bucket.settled_suggestion_ids.is_empty() {
        return;
    }
    bucket.events.retain(|event| {
        event.positive.abs() >= f64::EPSILON || (event.negative - 0.2).abs() >= f64::EPSILON
    });
}

fn bucket_mut<'a>(
    row: &'a mut CorrectionRow,
    left_token_nfc: Option<&str>,
) -> &'a mut EvidenceBucket {
    match left_token_nfc {
        Some(left) => row.context_mut(left),
        None => &mut row.global,
    }
}

fn import_bucket_metadata(bucket: &mut EvidenceBucket, metadata: &ImportedOperationalMetadata<'_>) {
    for &(edit_id, at_ms, undone) in metadata.recent_auto {
        if !bucket
            .recent_auto
            .iter()
            .any(|item| item.edit_id == edit_id)
        {
            bucket.recent_auto.push(AutoEmission {
                edit_id,
                at_ms,
                undone,
            });
        }
    }
    bucket
        .recent_auto
        .sort_by_key(|item| (item.at_ms, item.edit_id));
    bucket
        .handled_sequences
        .extend(metadata.handled_sequences.iter().copied());
    bucket
        .settled_auto_ids
        .extend(metadata.settled_auto_ids.iter().copied());
    bucket
        .settled_suggestion_ids
        .extend(metadata.settled_suggestion_ids.iter().copied());
    bucket.auto_demoted_at_seq = match (bucket.auto_demoted_at_seq, metadata.auto_demoted_at_seq) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    };
}

fn trim_bucket_operational(bucket: &mut EvidenceBucket, max_events: usize, undo_window: usize) {
    trim_set(&mut bucket.handled_sequences, max_events);
    trim_set(&mut bucket.settled_auto_ids, max_events);
    trim_set(&mut bucket.settled_suggestion_ids, max_events);
    trim_vec_front(&mut bucket.recent_auto, undo_window);
}

fn push_inspection(
    output: &mut Vec<CorrectionInspection>,
    row: &CorrectionRow,
    left_token_nfc: Option<String>,
    bucket: &EvidenceBucket,
) {
    if bucket.events.is_empty()
        && bucket.summary.checkpoint_at_ms.is_none()
        && bucket.state == DecisionState::Ignore
        && row.personal_transactions.is_empty()
        && row.shown_count == 0
        && row.selected_count == 0
    {
        return;
    }
    let raw = bucket.raw_mass();
    let personal_count = row.personal_transactions.len();
    output.push(CorrectionInspection {
        identity: row.identity.clone(),
        left_token_nfc,
        state: cap_state(&row.identity, bucket.state),
        evidence_count: bucket.events.len().max(personal_count),
        positive_evidence: raw.0,
        negative_evidence: raw.1,
        last_evidence_at_ms: bucket.last_activity_at_ms(),
        shown_count: row.shown_count,
        selected_count: row.selected_count,
        last_shown_at_ms: row.last_shown_at_ms,
        recent_auto_count: bucket.recent_auto.len(),
        recent_undo_count: bucket
            .recent_auto
            .iter()
            .filter(|emission| emission.undone)
            .count(),
    });
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_zero(value: &u64) -> bool {
    *value == 0
}

fn weakest_row_index(rows: &[CorrectionRow]) -> Option<usize> {
    rows.iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| row_priority(left, right))
        .map(|(index, _)| index)
}

fn row_priority(left: &CorrectionRow, right: &CorrectionRow) -> Ordering {
    left.retention_state()
        .cmp(&right.retention_state())
        // A demotion marker is a recent user veto. Keep it so eviction cannot
        // silently erase the safety signal and allow an unwanted Auto again.
        .then_with(|| left.has_demotion_marker().cmp(&right.has_demotion_marker()))
        .then_with(|| left.personal_promoted.cmp(&right.personal_promoted))
        // Repeatedly shown but never selected is an eviction signal, not negative evidence.
        .then_with(|| unused_impressions(right).cmp(&unused_impressions(left)))
        .then_with(|| evidence_strength(&left.global).total_cmp(&evidence_strength(&right.global)))
        .then_with(|| {
            left.global
                .last_activity_at_ms()
                .cmp(&right.global.last_activity_at_ms())
        })
        .then_with(|| left.identity.cmp(&right.identity))
}

fn context_priority(
    left_row: &CorrectionRow,
    left: &ContextBucket,
    right_row: &CorrectionRow,
    right: &ContextBucket,
) -> Ordering {
    left.evidence
        .state
        .cmp(&right.evidence.state)
        .then_with(|| {
            left.evidence
                .auto_demoted_at_seq
                .is_some()
                .cmp(&right.evidence.auto_demoted_at_seq.is_some())
        })
        .then_with(|| {
            evidence_strength(&left.evidence).total_cmp(&evidence_strength(&right.evidence))
        })
        .then_with(|| {
            left.evidence
                .last_activity_at_ms()
                .cmp(&right.evidence.last_activity_at_ms())
        })
        .then_with(|| left_row.identity.cmp(&right_row.identity))
        .then_with(|| left.left_token_nfc.cmp(&right.left_token_nfc))
}

fn unused_impressions(row: &CorrectionRow) -> u64 {
    if row.selected_count == 0 {
        row.shown_count
    } else {
        0
    }
}

fn evidence_strength(bucket: &EvidenceBucket) -> f64 {
    let mass = bucket.raw_mass();
    mass.0 + mass.1
}

// Legacy Personal counts have no timestamps. These synthetic anchors preserve
// deduplication/promotion only; migration must not turn them into recency evidence.
fn synthetic_personal_transactions(count: u32) -> Vec<PersonalTransaction> {
    let retained = count.min(64);
    (0..retained)
        .map(|index| PersonalTransaction {
            anchor: u64::MAX - u64::from(index),
            at_ms: 0,
        })
        .collect()
}

fn personal_identity(
    input_method: InputMethod,
    original_nfc: String,
    replacement_nfc: String,
) -> CorrectionIdentity {
    CorrectionIdentity {
        input_method,
        source: CandidateSource::Personal,
        original_nfc,
        candidate_nfc: replacement_nfc,
        source_rule_id: PERSONAL_RULE_ID.to_string(),
    }
}

fn cap_state(identity: &CorrectionIdentity, state: DecisionState) -> DecisionState {
    if state == DecisionState::Auto && identity.source.max_action() == ActionCap::Suggest {
        DecisionState::Suggest
    } else {
        state
    }
}

fn context_weight(support: f64, shrinkage_k: f64) -> f64 {
    support / (support + finite_non_negative(shrinkage_k))
}

fn trim_set(set: &mut BTreeSet<u64>, max_entries: usize) {
    while set.len() > max_entries {
        set.pop_first();
    }
}

fn trim_vec_front<T>(items: &mut Vec<T>, max_entries: usize) {
    if items.len() > max_entries {
        let excess = items.len() - max_entries;
        items.drain(..excess);
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}

fn confidence((positive, negative): (f64, f64)) -> f64 {
    (1.0 + positive) / (2.0 + positive + negative)
}

fn finite_non_negative(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

#[allow(clippy::cast_precision_loss)]
fn decay_factor(event_at_ms: i64, evaluate_at_ms: i64, half_life_ms: i64) -> f64 {
    let age_ms = evaluate_at_ms.saturating_sub(event_at_ms).max(0);
    2.0_f64.powf(-(age_ms as f64) / half_life_ms.max(1) as f64)
}

#[allow(clippy::cast_precision_loss)]
fn chart_checkpoint_factor(checkpoint_at_ms: i64, evaluate_at_ms: i64, half_life_ms: i64) -> f64 {
    let elapsed_ms = evaluate_at_ms.saturating_sub(checkpoint_at_ms);
    let exponent = -(elapsed_ms as f64 / half_life_ms.max(1) as f64);
    2.0_f64.powf(exponent.clamp(-1_023.0, 1_023.0))
}
