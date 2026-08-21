//! Exact-correction evidence with support-aware left-context refinement.
//!
//! Each event is recorded once in the context-free bucket and once in its
//! optional left-token bucket. Queries blend those buckets; they never sum
//! global and contextual evidence or inherit state from a sibling context.

use crate::decision::{ActionCap, DecisionState};
use crate::intervention::CorrectionIdentity;
use serde::{Deserialize, Serialize};

const DEFAULT_HALF_LIFE_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct EvidenceBucket {
    state: DecisionState,
    events: Vec<CorrectionEvidence>,
}

impl Default for EvidenceBucket {
    fn default() -> Self {
        Self {
            state: DecisionState::Ignore,
            events: Vec::new(),
        }
    }
}

impl EvidenceBucket {
    fn apply(&mut self, evidence: CorrectionEvidence) {
        if self.events.iter().any(|item| item.seq == evidence.seq) {
            return;
        }
        let evidence = CorrectionEvidence {
            positive: finite_non_negative(evidence.positive),
            negative: finite_non_negative(evidence.negative),
            ..evidence
        };
        self.events.push(evidence);
        self.events.sort_by_key(|event| (event.at_ms, event.seq));
    }

    fn mass(&self, evaluate_at_ms: i64, half_life_ms: i64) -> (f64, f64) {
        self.events.iter().fold((0.0, 0.0), |mass, event| {
            let factor = decay_factor(event.at_ms, evaluate_at_ms, half_life_ms);
            (
                mass.0 + event.positive * factor,
                mass.1 + event.negative * factor,
            )
        })
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
}

impl CorrectionRow {
    fn new(identity: CorrectionIdentity) -> Self {
        Self {
            identity,
            global: EvidenceBucket::default(),
            contexts: Vec::new(),
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
}

/// Bounded payload namespace for exact-correction rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorrectionMemory {
    #[serde(default = "default_half_life_ms")]
    half_life_ms: i64,
    rows: Vec<CorrectionRow>,
}

impl Default for CorrectionMemory {
    fn default() -> Self {
        Self {
            half_life_ms: DEFAULT_HALF_LIFE_MS,
            rows: Vec::new(),
        }
    }
}

impl CorrectionMemory {
    /// Record evidence globally and, when supplied, in exactly one context bucket.
    pub fn apply(
        &mut self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
        evidence: CorrectionEvidence,
    ) {
        let row = self.row_mut(identity);
        if row
            .global
            .events
            .iter()
            .any(|item| item.seq == evidence.seq)
        {
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
        let bucket = match left_token_nfc {
            Some(left) => row.context_mut(left),
            None => &mut row.global,
        };
        bucket.state = cap_state(identity, state);
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
        let weight = support / (support + finite_non_negative(shrinkage_k));
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
        let support = context.mass(evaluate_at_ms, self.half_life_ms);
        let total_support = support.0 + support.1;
        if total_support <= 0.0 {
            return global;
        }
        let weight = total_support / (total_support + finite_non_negative(shrinkage_k));
        (
            weight * support.0 + (1.0 - weight) * global.0,
            weight * support.1 + (1.0 - weight) * global.1,
        )
    }

    /// Return state for the requested bucket, capped by source policy.
    #[must_use]
    pub fn query_state(
        &self,
        identity: &CorrectionIdentity,
        left_token_nfc: Option<&str>,
    ) -> DecisionState {
        let Some(row) = self.row(identity) else {
            return DecisionState::Ignore;
        };
        let state = left_token_nfc
            .and_then(|left| row.context(left))
            .map_or(row.global.state, |bucket| bucket.state);
        cap_state(identity, state)
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
}

fn cap_state(identity: &CorrectionIdentity, state: DecisionState) -> DecisionState {
    if state == DecisionState::Auto && identity.source.max_action() == ActionCap::Suggest {
        DecisionState::Suggest
    } else {
        state
    }
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
