//! Deterministic adaptive model and read-only model view.
//!
//! Evidence is stored as non-negative Beta mass and decayed at query time
//! using caller-supplied timestamps. No wall clock or background task exists.

use crate::decision::DecisionState;
use crate::types::{CandidateSource, FeedbackEvent, FeedbackKind, InputMethod};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MODEL_VERSION: u32 = 1;
const DEFAULT_HALF_LIFE_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

/// Stable learning key. Two original→candidate pairs never share evidence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RuleContextKey {
    pub input_method: InputMethod,
    pub source: CandidateSource,
    pub original_nfc: String,
    pub candidate_nfc: String,
    pub left_token_nfc: Option<String>,
    pub source_rule_id: String,
}

/// Read-only query surface used by rank/decision. Time is injected.
pub trait ModelView {
    fn confidence(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> f64;
    fn positive_mass(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> f64;
    fn state(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> DecisionState;

    fn auto_allowed(&self, _key: &RuleContextKey, _evaluate_at_ms: i64) -> bool {
        true
    }
}

/// Versioned adaptive-model limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelConfig {
    pub half_life_ms: i64,
    pub max_events_per_rule: usize,
    pub auto_undo_window: usize,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            half_life_ms: DEFAULT_HALF_LIFE_MS,
            max_events_per_rule: 512,
            auto_undo_window: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct EvidenceRecord {
    seq: u64,
    at_ms: i64,
    positive_add: f64,
    negative_add: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AutoEmission {
    edit_id: u64,
    at_ms: i64,
    undone: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RuleEntry {
    key: RuleContextKey,
    state: DecisionState,
    evidence: Vec<EvidenceRecord>,
    recent_auto: Vec<AutoEmission>,
    handled_feedback_seqs: BTreeSet<u64>,
    settled_auto_ids: BTreeSet<u64>,
    settled_suggestion_ids: BTreeSet<u64>,
    #[serde(default)]
    auto_demoted_at_seq: Option<u64>,
}

impl RuleEntry {
    fn new(key: RuleContextKey) -> Self {
        Self {
            key,
            state: DecisionState::Ignore,
            evidence: Vec::new(),
            recent_auto: Vec::new(),
            handled_feedback_seqs: BTreeSet::new(),
            settled_auto_ids: BTreeSet::new(),
            settled_suggestion_ids: BTreeSet::new(),
            auto_demoted_at_seq: None,
        }
    }
}

/// Event-backed model with deterministic serialization and query-time decay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdaptiveModel {
    version: u32,
    config: ModelConfig,
    entries: Vec<RuleEntry>,
}

impl AdaptiveModel {
    #[must_use]
    pub fn new(config: ModelConfig) -> Self {
        Self {
            version: MODEL_VERSION,
            config,
            entries: Vec::new(),
        }
    }

    /// Applies one feedback event once. Disabled learning is a strict no-op.
    pub fn apply_feedback(
        &mut self,
        key: &RuleContextKey,
        event: &FeedbackEvent,
        allow_learning: bool,
    ) {
        if !allow_learning {
            return;
        }
        let max_events = self.config.max_events_per_rule.max(1);
        let undo_window = self.config.auto_undo_window.max(1);
        let entry = self.entry_mut(key);
        if !entry.handled_feedback_seqs.insert(event.seq) {
            return;
        }
        trim_set(&mut entry.handled_feedback_seqs, max_events);

        let (positive_add, negative_add) = match event.kind {
            FeedbackKind::Accept { .. } | FeedbackKind::ImplicitCorrection { .. } => {
                if entry.state == DecisionState::Ignore {
                    entry.state = DecisionState::Suggest;
                }
                (1.0, 0.0)
            }
            FeedbackKind::ExplicitReject { .. } => (0.0, 1.0),
            FeedbackKind::Undo { edit_id } => {
                if let Some(emission) = entry
                    .recent_auto
                    .iter_mut()
                    .find(|emission| emission.edit_id == edit_id)
                {
                    emission.undone = true;
                }
                if entry.recent_auto.iter().filter(|item| item.undone).count() >= 2 {
                    entry.state = DecisionState::Suggest;
                    entry.auto_demoted_at_seq.get_or_insert(event.seq);
                }
                (0.0, 1.5)
            }
            FeedbackKind::AutoSettled { edit_id } => {
                if entry.settled_auto_ids.insert(edit_id) {
                    (0.3, 0.0)
                } else {
                    (0.0, 0.0)
                }
            }
            FeedbackKind::SuggestionSettled { candidate_id } => {
                if entry.settled_suggestion_ids.insert(candidate_id) {
                    (0.0, 0.2)
                } else {
                    (0.0, 0.0)
                }
            }
        };

        if positive_add > 0.0 || negative_add > 0.0 {
            entry.evidence.push(EvidenceRecord {
                seq: event.seq,
                at_ms: event.at_ms,
                positive_add,
                negative_add,
            });
            if entry.evidence.len() > max_events {
                let excess = entry.evidence.len() - max_events;
                entry.evidence.drain(..excess);
            }
        }
        if entry.recent_auto.len() > undo_window {
            let excess = entry.recent_auto.len() - undo_window;
            entry.recent_auto.drain(..excess);
        }
        trim_set(&mut entry.settled_auto_ids, max_events);
        trim_set(&mut entry.settled_suggestion_ids, max_events);
    }

    /// Persists one operational state transition when learning is allowed.
    pub fn record_decision(
        &mut self,
        key: &RuleContextKey,
        state: DecisionState,
        allow_learning: bool,
    ) {
        if !allow_learning {
            return;
        }
        if state == DecisionState::Ignore && self.entry(key).is_none() {
            return;
        }
        let state = if state == DecisionState::Auto
            && key.source.max_action() != crate::decision::ActionCap::Auto
        {
            DecisionState::Suggest
        } else {
            state
        };
        let entry = self.entry_mut(key);
        entry.state = state;
        if state == DecisionState::Auto {
            entry.auto_demoted_at_seq = None;
        }
    }

    /// Records one auto emission and bounds the per-rule undo window.
    pub fn record_auto_emission(
        &mut self,
        key: &RuleContextKey,
        edit_id: u64,
        at_ms: i64,
        allow_learning: bool,
    ) {
        if !allow_learning || key.source.max_action() != crate::decision::ActionCap::Auto {
            return;
        }
        let undo_window = self.config.auto_undo_window.max(1);
        let entry = self.entry_mut(key);
        if entry.recent_auto.iter().any(|item| item.edit_id == edit_id) {
            return;
        }
        entry.recent_auto.push(AutoEmission {
            edit_id,
            at_ms,
            undone: false,
        });
        if entry.recent_auto.len() > undo_window {
            let excess = entry.recent_auto.len() - undo_window;
            entry.recent_auto.drain(..excess);
        }
    }

    #[must_use]
    pub fn negative_mass(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> f64 {
        self.mass(key, evaluate_at_ms).1
    }

    pub fn to_json_payload(&self) -> Result<Vec<u8>, ModelError> {
        let mut stable = self.clone();
        stable.entries.sort_by(|a, b| a.key.cmp(&b.key));
        serde_json::to_vec(&stable).map_err(|error| ModelError::InvalidPayload(error.to_string()))
    }

    pub fn from_json_payload(bytes: &[u8]) -> Result<Self, ModelError> {
        let mut model: Self = serde_json::from_slice(bytes)
            .map_err(|error| ModelError::InvalidPayload(error.to_string()))?;
        if model.version != MODEL_VERSION {
            return Err(ModelError::UnsupportedVersion);
        }
        model.entries.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(model)
    }

    fn entry_mut(&mut self, key: &RuleContextKey) -> &mut RuleEntry {
        if let Some(index) = self.entries.iter().position(|entry| &entry.key == key) {
            return &mut self.entries[index];
        }
        self.entries.push(RuleEntry::new(key.clone()));
        self.entries.last_mut().expect("entry was just pushed")
    }

    fn entry(&self, key: &RuleContextKey) -> Option<&RuleEntry> {
        self.entries.iter().find(|entry| &entry.key == key)
    }

    fn mass(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> (f64, f64) {
        let Some(entry) = self.entry(key) else {
            return (0.0, 0.0);
        };
        entry.evidence.iter().fold((0.0, 0.0), |acc, event| {
            let factor = decay_factor(event.at_ms, evaluate_at_ms, self.config.half_life_ms.max(1));
            (
                acc.0 + event.positive_add.max(0.0) * factor,
                acc.1 + event.negative_add.max(0.0) * factor,
            )
        })
    }
}

impl Default for AdaptiveModel {
    fn default() -> Self {
        Self::new(ModelConfig::default())
    }
}

impl ModelView for AdaptiveModel {
    fn confidence(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> f64 {
        let (positive, negative) = self.mass(key, evaluate_at_ms);
        (1.0 + positive) / (2.0 + positive + negative)
    }

    fn positive_mass(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> f64 {
        self.mass(key, evaluate_at_ms).0
    }

    fn state(&self, key: &RuleContextKey, _evaluate_at_ms: i64) -> DecisionState {
        self.entry(key)
            .map_or(DecisionState::Ignore, |entry| entry.state)
    }

    fn auto_allowed(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> bool {
        let Some(entry) = self.entry(key) else {
            return true;
        };
        let Some(demoted_at_seq) = entry.auto_demoted_at_seq else {
            return true;
        };
        let positive_since_demotion = entry
            .evidence
            .iter()
            .filter(|event| event.seq > demoted_at_seq)
            .map(|event| {
                event.positive_add.max(0.0)
                    * decay_factor(event.at_ms, evaluate_at_ms, self.config.half_life_ms.max(1))
            })
            .sum::<f64>();
        positive_since_demotion >= 18.0
    }
}

fn trim_set(set: &mut BTreeSet<u64>, max_entries: usize) {
    while set.len() > max_entries {
        set.pop_first();
    }
}

#[allow(clippy::cast_precision_loss)]
fn decay_factor(event_at_ms: i64, evaluate_at_ms: i64, half_life_ms: i64) -> f64 {
    let age_ms = evaluate_at_ms.saturating_sub(event_at_ms).max(0);
    2.0_f64.powf(-(age_ms as f64) / half_life_ms as f64)
}

/// Cold-start model: Beta(1,1) prior, no stored evidence, feedback is a no-op.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EmptyModel;

const EMPTY_MODEL_JSON: &str = "{\"version\":1,\"entries\":[]}";

impl EmptyModel {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn apply_feedback(&mut self, _event: &FeedbackEvent, _key: &RuleContextKey) {}

    pub fn to_json_payload(&self) -> Result<Vec<u8>, ModelError> {
        Ok(EMPTY_MODEL_JSON.as_bytes().to_vec())
    }

    pub fn from_json_payload(bytes: &[u8]) -> Result<Self, ModelError> {
        if bytes == EMPTY_MODEL_JSON.as_bytes() {
            Ok(Self)
        } else {
            Err(ModelError::UnsupportedVersion)
        }
    }
}

impl ModelView for EmptyModel {
    fn confidence(&self, _key: &RuleContextKey, _evaluate_at_ms: i64) -> f64 {
        0.5
    }

    fn positive_mass(&self, _key: &RuleContextKey, _evaluate_at_ms: i64) -> f64 {
        0.0
    }

    fn state(&self, _key: &RuleContextKey, _evaluate_at_ms: i64) -> DecisionState {
        DecisionState::Ignore
    }
}

/// Errors when reading a model payload. Distinct from store envelope errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    UnsupportedVersion,
    InvalidPayload(String),
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion => write!(f, "unsupported model payload version"),
            Self::InvalidPayload(message) => write!(f, "invalid model payload: {message}"),
        }
    }
}

impl std::error::Error for ModelError {}
