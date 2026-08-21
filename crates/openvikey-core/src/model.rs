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
const IMPLICIT_CORRECTION_MASS: f64 = 1.5;
const SETTLEMENT_ADD: f64 = 0.3;
const MAX_SETTLEMENTS_PER_RULE: usize = 24;
const PERSONAL_PROMOTE_K: u32 = 2;
const DEFAULT_MAX_RULES: usize = 10_000;
const DEFAULT_MAX_PERSONAL_PAIRS: usize = 512;

const fn default_max_rules() -> usize {
    DEFAULT_MAX_RULES
}

const fn default_max_personal_pairs() -> usize {
    DEFAULT_MAX_PERSONAL_PAIRS
}

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
    #[serde(default = "default_max_rules")]
    pub max_rules: usize,
    #[serde(default = "default_max_personal_pairs")]
    pub max_personal_pairs: usize,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            half_life_ms: DEFAULT_HALF_LIFE_MS,
            max_events_per_rule: 512,
            auto_undo_window: 10,
            max_rules: DEFAULT_MAX_RULES,
            max_personal_pairs: DEFAULT_MAX_PERSONAL_PAIRS,
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

fn find_entry_and_eviction_candidate(
    entries: &[RuleEntry],
    key: &RuleContextKey,
) -> (Option<usize>, Option<usize>) {
    let mut victim = None;
    for (index, entry) in entries.iter().enumerate() {
        if &entry.key == key {
            return (Some(index), victim);
        }
        if victim.is_none_or(|current| eviction_priority(entry, &entries[current]).is_lt()) {
            victim = Some(index);
        }
    }
    (None, victim)
}

fn eviction_candidate(entries: &[RuleEntry]) -> Option<usize> {
    entries
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| eviction_priority(left, right))
        .map(|(index, _)| index)
}

fn eviction_priority(left: &RuleEntry, right: &RuleEntry) -> std::cmp::Ordering {
    left.state
        .cmp(&right.state)
        .then_with(|| {
            left.auto_demoted_at_seq
                .is_some()
                .cmp(&right.auto_demoted_at_seq.is_some())
        })
        .then_with(|| evidence_strength(left).total_cmp(&evidence_strength(right)))
        .then_with(|| last_rule_activity(left).cmp(&last_rule_activity(right)))
        .then_with(|| left.key.cmp(&right.key))
}

fn evidence_strength(entry: &RuleEntry) -> f64 {
    entry
        .evidence
        .iter()
        .map(|event| event.positive_add.max(0.0) + event.negative_add.max(0.0))
        .sum()
}

fn last_rule_activity(entry: &RuleEntry) -> i64 {
    entry
        .evidence
        .iter()
        .map(|event| event.at_ms)
        .chain(entry.recent_auto.iter().map(|emission| emission.at_ms))
        .max()
        .unwrap_or(i64::MIN)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PersonalCountRow {
    input_method: InputMethod,
    original_nfc: String,
    replacement_nfc: String,
    count: u32,
}

/// Stable, read-only projection used by development inspection tools.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelInspectionRow {
    pub input_method: InputMethod,
    pub source: CandidateSource,
    pub original_nfc: String,
    pub candidate_nfc: String,
    pub left_token_nfc: Option<String>,
    pub source_rule_id: String,
    pub state: DecisionState,
    pub evidence_count: usize,
    pub positive_evidence: f64,
    pub negative_evidence: f64,
    pub last_evidence_at_ms: Option<i64>,
}

/// User-taught original→replacement pairs that are not engine candidates yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PersonalCorrectionStore {
    #[serde(default)]
    counts: Vec<PersonalCountRow>,
    #[serde(default)]
    promoted: Vec<PersonalCountRow>,
}

/// Event-backed model with deterministic serialization and query-time decay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdaptiveModel {
    version: u32,
    config: ModelConfig,
    entries: Vec<RuleEntry>,
    #[serde(default)]
    personal: PersonalCorrectionStore,
}

impl AdaptiveModel {
    #[must_use]
    pub fn new(config: ModelConfig) -> Self {
        Self {
            version: MODEL_VERSION,
            config,
            entries: Vec::new(),
            personal: PersonalCorrectionStore::default(),
        }
    }

    /// Applies one feedback event once. Disabled learning is a strict no-op.
    pub fn apply_feedback(
        &mut self,
        key: &RuleContextKey,
        event: &FeedbackEvent,
        allow_learning: bool,
    ) -> (f64, f64) {
        if !allow_learning {
            return (0.0, 0.0);
        }
        let max_events = self.config.max_events_per_rule.max(1);
        let undo_window = self.config.auto_undo_window.max(1);
        let entry = self.entry_mut(key);
        if !entry.handled_feedback_seqs.insert(event.seq) {
            return (0.0, 0.0);
        }
        trim_set(&mut entry.handled_feedback_seqs, max_events);

        let (positive_add, negative_add) = match event.kind {
            FeedbackKind::Accept { .. } => {
                if entry.state == DecisionState::Ignore {
                    entry.state = DecisionState::Suggest;
                }
                (1.0, 0.0)
            }
            FeedbackKind::ImplicitCorrection { .. } => {
                if entry.state == DecisionState::Ignore {
                    entry.state = DecisionState::Suggest;
                }
                (IMPLICIT_CORRECTION_MASS, 0.0)
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
                if !entry.settled_auto_ids.insert(edit_id)
                    || entry.settled_auto_ids.len() > MAX_SETTLEMENTS_PER_RULE
                {
                    (0.0, 0.0)
                } else {
                    (SETTLEMENT_ADD, 0.0)
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
        (positive_add, negative_add)
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
        stable.personal.counts.sort_by(|a, b| {
            (a.input_method, &a.original_nfc, &a.replacement_nfc).cmp(&(
                b.input_method,
                &b.original_nfc,
                &b.replacement_nfc,
            ))
        });
        stable.personal.promoted.sort_by(|a, b| {
            (a.input_method, &a.original_nfc, &a.replacement_nfc).cmp(&(
                b.input_method,
                &b.original_nfc,
                &b.replacement_nfc,
            ))
        });
        serde_json::to_vec(&stable).map_err(|error| ModelError::InvalidPayload(error.to_string()))
    }

    pub fn from_json_payload(bytes: &[u8]) -> Result<Self, ModelError> {
        let mut model: Self = serde_json::from_slice(bytes)
            .map_err(|error| ModelError::InvalidPayload(error.to_string()))?;
        if model.version != MODEL_VERSION {
            return Err(ModelError::UnsupportedVersion);
        }
        model.entries.sort_by(|a, b| a.key.cmp(&b.key));
        model.enforce_limits();
        Ok(model)
    }

    /// Return the raw positive/negative evidence totals for one exact adaptive rule.
    #[must_use]
    pub fn evidence_totals(&self, key: &RuleContextKey) -> (f64, f64) {
        self.entry(key).map_or((0.0, 0.0), |entry| {
            entry.evidence.iter().fold((0.0, 0.0), |totals, item| {
                (totals.0 + item.positive_add, totals.1 + item.negative_add)
            })
        })
    }

    /// Return owned rows without exposing mutable model internals.
    #[must_use]
    pub fn inspection_rows(&self) -> Vec<ModelInspectionRow> {
        let mut rows = self
            .entries
            .iter()
            .filter(|entry| !entry.evidence.is_empty() || entry.state != DecisionState::Ignore)
            .map(|entry| ModelInspectionRow {
                input_method: entry.key.input_method,
                source: entry.key.source,
                original_nfc: entry.key.original_nfc.clone(),
                candidate_nfc: entry.key.candidate_nfc.clone(),
                left_token_nfc: entry.key.left_token_nfc.clone(),
                source_rule_id: entry.key.source_rule_id.clone(),
                state: entry.state,
                evidence_count: entry.evidence.len(),
                positive_evidence: entry.evidence.iter().map(|item| item.positive_add).sum(),
                negative_evidence: entry.evidence.iter().map(|item| item.negative_add).sum(),
                last_evidence_at_ms: entry.evidence.iter().map(|item| item.at_ms).max(),
            })
            .collect::<Vec<_>>();
        rows.extend(self.personal.counts.iter().map(|personal| {
            let promoted = self.personal.promoted.iter().any(|row| {
                row.input_method == personal.input_method
                    && row.original_nfc == personal.original_nfc
                    && row.replacement_nfc == personal.replacement_nfc
            });
            ModelInspectionRow {
                input_method: personal.input_method,
                source: CandidateSource::Personal,
                original_nfc: personal.original_nfc.clone(),
                candidate_nfc: personal.replacement_nfc.clone(),
                left_token_nfc: None,
                source_rule_id: "personal-correction".into(),
                state: if promoted {
                    DecisionState::Suggest
                } else {
                    DecisionState::Ignore
                },
                evidence_count: usize::try_from(personal.count).unwrap_or(usize::MAX),
                positive_evidence: f64::from(personal.count),
                negative_evidence: 0.0,
                last_evidence_at_ms: None,
            }
        }));
        rows
    }

    fn entry_mut(&mut self, key: &RuleContextKey) -> &mut RuleEntry {
        let (existing, first_victim) = find_entry_and_eviction_candidate(&self.entries, key);
        if let Some(index) = existing {
            return &mut self.entries[index];
        }
        let max_rules = self.config.max_rules.max(1);
        let mut first_victim = first_victim;
        while self.entries.len() >= max_rules {
            let victim = first_victim
                .take()
                .or_else(|| eviction_candidate(&self.entries))
                .expect("a non-empty capped store has an eviction candidate");
            self.entries.remove(victim);
        }
        self.entries.push(RuleEntry::new(key.clone()));
        self.entries.last_mut().expect("entry was just pushed")
    }

    fn enforce_limits(&mut self) {
        let max_rules = self.config.max_rules.max(1);
        while self.entries.len() > max_rules {
            let victim = eviction_candidate(&self.entries)
                .expect("an over-cap rule store has an eviction candidate");
            self.entries.remove(victim);
        }
        let max_personal = self.config.max_personal_pairs.max(1);
        while self.personal.counts.len() > max_personal {
            let victim = weakest_personal_index(&self.personal, false)
                .or_else(|| weakest_personal_index(&self.personal, true))
                .expect("an over-cap personal store has an eviction candidate");
            remove_personal_at(&mut self.personal, victim);
        }
    }

    fn entry(&self, key: &RuleContextKey) -> Option<&RuleEntry> {
        self.entries.iter().find(|entry| &entry.key == key)
    }

    fn backoff_entries<'a>(
        &'a self,
        key: &'a RuleContextKey,
    ) -> impl Iterator<Item = &'a RuleEntry> {
        self.entries
            .iter()
            .filter(move |entry| same_backoff_identity(&entry.key, key))
    }

    fn mass(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> (f64, f64) {
        self.backoff_entries(key).fold((0.0, 0.0), |acc, entry| {
            let (positive, negative) =
                decayed_mass(entry, evaluate_at_ms, self.config.half_life_ms);
            (acc.0 + positive, acc.1 + negative)
        })
    }

    /// Records an unmatched rewind pair. Returns true when the pair is newly promoted.
    pub fn record_personal_correction(
        &mut self,
        input_method: InputMethod,
        original_nfc: impl Into<String>,
        replacement_nfc: impl Into<String>,
        allow_learning: bool,
    ) -> bool {
        if !allow_learning {
            return false;
        }
        let original_nfc = original_nfc.into();
        let replacement_nfc = replacement_nfc.into();
        if original_nfc.is_empty() || replacement_nfc.is_empty() || original_nfc == replacement_nfc
        {
            return false;
        }
        let (input_method, original_nfc, replacement_nfc, count) = if let Some(row) =
            self.personal.counts.iter_mut().find(|row| {
                row.input_method == input_method
                    && row.original_nfc == original_nfc
                    && row.replacement_nfc == replacement_nfc
            }) {
            row.count = row.count.saturating_add(1);
            (
                row.input_method,
                row.original_nfc.clone(),
                row.replacement_nfc.clone(),
                row.count,
            )
        } else {
            let max_personal = self.config.max_personal_pairs.max(1);
            if self.personal.counts.len() >= max_personal {
                let Some(victim) = weakest_personal_index(&self.personal, false) else {
                    return false;
                };
                remove_personal_at(&mut self.personal, victim);
            }
            self.personal.counts.push(PersonalCountRow {
                input_method,
                original_nfc: original_nfc.clone(),
                replacement_nfc: replacement_nfc.clone(),
                count: 1,
            });
            return false;
        };
        if count >= PERSONAL_PROMOTE_K {
            return promote_personal(
                &mut self.personal,
                self.config.max_personal_pairs.max(1),
                input_method,
                &original_nfc,
                &replacement_nfc,
            );
        }
        false
    }

    /// Return the number of observed corrections for one exact personal pair.
    #[must_use]
    pub fn personal_correction_count(
        &self,
        input_method: InputMethod,
        original_nfc: &str,
        replacement_nfc: &str,
    ) -> u32 {
        self.personal
            .counts
            .iter()
            .find(|row| {
                row.input_method == input_method
                    && row.original_nfc == original_nfc
                    && row.replacement_nfc == replacement_nfc
            })
            .map_or(0, |row| row.count)
    }

    #[must_use]
    pub fn personal_promoted(&self) -> Vec<(InputMethod, String, String)> {
        self.personal
            .promoted
            .iter()
            .map(|row| {
                (
                    row.input_method,
                    row.original_nfc.clone(),
                    row.replacement_nfc.clone(),
                )
            })
            .collect()
    }

    /// Physically removes one exact adaptive row and all of its metadata.
    pub fn forget_rule(&mut self, key: &RuleContextKey) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| &entry.key != key);
        self.entries.len() != before
    }

    /// Physically removes one Personal pair from probation and promoted rows.
    pub fn forget_personal_pair(
        &mut self,
        input_method: InputMethod,
        original_nfc: &str,
        replacement_nfc: &str,
    ) -> bool {
        let before_counts = self.personal.counts.len();
        let before_promoted = self.personal.promoted.len();
        self.personal.counts.retain(|row| {
            !(row.input_method == input_method
                && row.original_nfc == original_nfc
                && row.replacement_nfc == replacement_nfc)
        });
        self.personal.promoted.retain(|row| {
            !(row.input_method == input_method
                && row.original_nfc == original_nfc
                && row.replacement_nfc == replacement_nfc)
        });
        self.personal.counts.len() != before_counts
            || self.personal.promoted.len() != before_promoted
    }

    /// Clears all learned state and restores the canonical cold-start configuration.
    pub fn forget_all(&mut self) -> bool {
        let empty = Self::default();
        if self == &empty {
            return false;
        }
        *self = empty;
        true
    }

    /// Forget exactly one row previously returned by [`Self::inspection_rows`].
    pub fn forget_inspection_row(&mut self, row: &ModelInspectionRow) -> bool {
        if row.source == CandidateSource::Personal && row.source_rule_id == "personal-correction" {
            return self.forget_personal_pair(
                row.input_method,
                &row.original_nfc,
                &row.candidate_nfc,
            );
        }
        let key = RuleContextKey {
            input_method: row.input_method,
            source: row.source,
            original_nfc: row.original_nfc.clone(),
            candidate_nfc: row.candidate_nfc.clone(),
            left_token_nfc: row.left_token_nfc.clone(),
            source_rule_id: row.source_rule_id.clone(),
        };
        self.forget_rule(&key)
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
        self.backoff_entries(key)
            .map(|entry| entry.state)
            .max()
            .unwrap_or(DecisionState::Ignore)
    }

    fn auto_allowed(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> bool {
        let siblings: Vec<&RuleEntry> = self.backoff_entries(key).collect();
        if siblings.is_empty() {
            return true;
        }
        siblings.iter().any(|entry| {
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
        })
    }
}

fn trim_set(set: &mut BTreeSet<u64>, max_entries: usize) {
    while set.len() > max_entries {
        set.pop_first();
    }
}

fn same_backoff_identity(left: &RuleContextKey, right: &RuleContextKey) -> bool {
    left.input_method == right.input_method
        && left.source == right.source
        && left.original_nfc == right.original_nfc
        && left.candidate_nfc == right.candidate_nfc
        && left.source_rule_id == right.source_rule_id
}

fn decayed_mass(entry: &RuleEntry, evaluate_at_ms: i64, half_life_ms: i64) -> (f64, f64) {
    entry.evidence.iter().fold((0.0, 0.0), |acc, event| {
        let factor = decay_factor(event.at_ms, evaluate_at_ms, half_life_ms.max(1));
        (
            acc.0 + event.positive_add.max(0.0) * factor,
            acc.1 + event.negative_add.max(0.0) * factor,
        )
    })
}

fn weakest_personal_index(
    store: &PersonalCorrectionStore,
    include_promoted: bool,
) -> Option<usize> {
    store
        .counts
        .iter()
        .enumerate()
        .filter(|(_, row)| include_promoted || !personal_is_promoted(store, row))
        .min_by(|(_, left), (_, right)| {
            left.count.cmp(&right.count).then_with(|| {
                (left.input_method, &left.original_nfc, &left.replacement_nfc).cmp(&(
                    right.input_method,
                    &right.original_nfc,
                    &right.replacement_nfc,
                ))
            })
        })
        .map(|(index, _)| index)
}

fn personal_is_promoted(store: &PersonalCorrectionStore, row: &PersonalCountRow) -> bool {
    store.promoted.iter().any(|promoted| {
        promoted.input_method == row.input_method
            && promoted.original_nfc == row.original_nfc
            && promoted.replacement_nfc == row.replacement_nfc
    })
}

fn remove_personal_at(store: &mut PersonalCorrectionStore, index: usize) {
    let removed = store.counts.remove(index);
    store.promoted.retain(|promoted| {
        promoted.input_method != removed.input_method
            || promoted.original_nfc != removed.original_nfc
            || promoted.replacement_nfc != removed.replacement_nfc
    });
}

fn promote_personal(
    store: &mut PersonalCorrectionStore,
    max_personal_pairs: usize,
    input_method: InputMethod,
    original_nfc: &str,
    replacement_nfc: &str,
) -> bool {
    let exists = store.promoted.iter().any(|row| {
        row.input_method == input_method
            && row.original_nfc == original_nfc
            && row.replacement_nfc == replacement_nfc
    });
    if exists || store.promoted.len() >= max_personal_pairs {
        return false;
    }
    store.promoted.push(PersonalCountRow {
        input_method,
        original_nfc: original_nfc.to_string(),
        replacement_nfc: replacement_nfc.to_string(),
        count: 0,
    });
    true
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
