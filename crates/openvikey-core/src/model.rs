//! Deterministic adaptive-model facade and read-only model view.
//!
//! Payload v2 owns exact corrections through [`CorrectionMemory`]. Legacy v1
//! rows are decoded only by the explicit migration path; serde defaults never
//! reinterpret a partial v2 payload as v1 state.

use crate::correction_memory::{
    CorrectionEvidence, CorrectionMemory, ImportedOperationalMetadata, PersonalTransaction,
};
use crate::decision::DecisionState;
use crate::intervention::CorrectionIdentity;
use crate::learning_config::LearningConfigV2;
use crate::types::{CandidateSource, FeedbackEvent, InputMethod};
use crate::user_language::{UnigramInspectionRow, UserLanguageModel};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const MODEL_VERSION: u32 = 2;
const DEFAULT_HALF_LIFE_MS: i64 = 30 * 24 * 60 * 60 * 1_000;
const DEFAULT_MAX_RULES: usize = 10_000;
const DEFAULT_MAX_CONTEXT_ROWS: usize = 30_000;
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

impl RuleContextKey {
    fn identity(&self) -> CorrectionIdentity {
        CorrectionIdentity {
            input_method: self.input_method,
            source: self.source,
            original_nfc: self.original_nfc.clone(),
            candidate_nfc: self.candidate_nfc.clone(),
            source_rule_id: self.source_rule_id.clone(),
        }
    }
}

/// Read-only query surface used by rank/decision. Time is injected.
pub trait ModelView {
    fn confidence(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> f64;
    fn positive_mass(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> f64;
    fn state(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> DecisionState;

    fn auto_allowed(&self, _key: &RuleContextKey, _evaluate_at_ms: i64) -> bool {
        true
    }

    fn unigram_signal(&self, _token_nfc: &str) -> f64 {
        0.0
    }

    fn bigram_signal(&self, _left_token_nfc: &str, _token_nfc: &str) -> f64 {
        0.0
    }
}

/// Versioned operational limits retained across model saves.
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
    pub shown_count: u64,
    pub selected_count: u64,
    pub last_shown_at_ms: Option<i64>,
    pub recent_auto_count: usize,
    pub recent_undo_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MaintenanceMetadata {
    model_config: ModelConfig,
}

/// Event-backed model with deterministic payload-v2 serialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdaptiveModel {
    version: u32,
    config_hash: String,
    correction_memory: CorrectionMemory,
    user_language_model: UserLanguageModel,
    maintenance_metadata: MaintenanceMetadata,
}

impl AdaptiveModel {
    #[must_use]
    pub fn new(config: ModelConfig) -> Self {
        Self {
            version: MODEL_VERSION,
            config_hash: LearningConfigV2::compatibility_v1().hash(),
            correction_memory: CorrectionMemory::with_half_life(config.half_life_ms),
            user_language_model: UserLanguageModel::default(),
            maintenance_metadata: MaintenanceMetadata {
                model_config: config,
            },
        }
    }

    /// Current persisted payload schema version.
    #[must_use]
    pub const fn payload_version(&self) -> u32 {
        self.version
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
        self.prepare_for(key);
        let config = self.config().clone();
        self.correction_memory.apply_feedback(
            &key.identity(),
            key.left_token_nfc.as_deref(),
            event,
            config.max_events_per_rule.max(1),
            config.auto_undo_window.max(1),
            LearningConfigV2::compatibility_v1().weak_positive_cap,
        )
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
        let identity = key.identity();
        if state == DecisionState::Ignore && !self.correction_memory.contains(&identity) {
            return;
        }
        self.prepare_for(key);
        self.correction_memory
            .record_state(&identity, key.left_token_nfc.as_deref(), state);
    }

    /// Records one auto emission and bounds the per-correction undo window.
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
        self.prepare_for(key);
        let undo_window = self.config().auto_undo_window.max(1);
        self.correction_memory.record_auto_emission(
            &key.identity(),
            key.left_token_nfc.as_deref(),
            edit_id,
            at_ms,
            undo_window,
        );
    }

    /// Records one suggestion that was actually exposed by the intervention plan.
    pub fn record_impression(
        &mut self,
        key: &RuleContextKey,
        seq: u64,
        at_ms: i64,
        allow_learning: bool,
    ) -> bool {
        if !allow_learning {
            return false;
        }
        self.prepare_for(key);
        self.correction_memory
            .record_impression(&key.identity(), seq, at_ms)
    }

    /// Records one safe committed token exactly once. Disabled learning is a strict no-op.
    pub fn record_language_commit(
        &mut self,
        token: &str,
        left_token: Option<&str>,
        at_ms: i64,
        transaction_id: u64,
        allow_learning: bool,
    ) -> bool {
        if !allow_learning {
            return false;
        }
        let config = LearningConfigV2::compatibility_v1();
        self.user_language_model.commit_transaction_bounded(
            token,
            left_token,
            at_ms,
            transaction_id,
            config.max_unigrams,
            config.max_bigrams,
        )
    }

    #[must_use]
    pub fn unigram_count(&self, token: &str) -> u64 {
        self.user_language_model.unigram(token)
    }

    /// Read-only correction-memory view for Settings/idle chart building.
    #[must_use]
    pub const fn correction_memory(&self) -> &CorrectionMemory {
        &self.correction_memory
    }

    #[must_use]
    pub fn bigram_count(&self, left_token: &str, token: &str) -> u64 {
        self.user_language_model.bigram(left_token, token)
    }

    #[must_use]
    pub fn language_unigrams(&self) -> Vec<UnigramInspectionRow> {
        self.user_language_model.unigram_rows()
    }

    /// Removes only language-history rows for one token.
    pub fn forget_token(&mut self, token: &str) -> bool {
        self.user_language_model.forget_token(token)
    }

    /// Records the operational veto from an immediate revert without adding evidence mass.
    pub fn record_immediate_revert(
        &mut self,
        key: &RuleContextKey,
        edit_id: u64,
        seq: u64,
        allow_learning: bool,
    ) {
        if !allow_learning {
            return;
        }
        self.correction_memory.record_immediate_revert(
            &key.identity(),
            key.left_token_nfc.as_deref(),
            edit_id,
            seq,
        );
    }

    #[must_use]
    pub fn negative_mass(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> f64 {
        self.mass(key, evaluate_at_ms).1
    }

    pub fn to_json_payload(&self) -> Result<Vec<u8>, ModelError> {
        serde_json::to_vec(self).map_err(|error| ModelError::InvalidPayload(error.to_string()))
    }

    pub fn from_json_payload(bytes: &[u8]) -> Result<Self, ModelError> {
        let version: PayloadVersion = serde_json::from_slice(bytes)
            .map_err(|error| ModelError::InvalidPayload(error.to_string()))?;
        match version.version {
            1 => {
                let legacy: LegacyModelV1 = serde_json::from_slice(bytes)
                    .map_err(|error| ModelError::InvalidPayload(error.to_string()))?;
                Ok(Self::migrate_v1(legacy))
            }
            MODEL_VERSION => {
                let mut model: Self = serde_json::from_slice(bytes)
                    .map_err(|error| ModelError::InvalidPayload(error.to_string()))?;
                if model.version != MODEL_VERSION || model.config_hash.len() != 64 {
                    return Err(ModelError::InvalidPayload(
                        "invalid v2 version or config hash".into(),
                    ));
                }
                let max_rules = model.config().max_rules;
                let weak_positive_cap = LearningConfigV2::compatibility_v1().weak_positive_cap;
                model
                    .correction_memory
                    .normalize_weak_positive_totals(weak_positive_cap);
                model
                    .correction_memory
                    .normalize_legacy_suggestion_settlements();
                model.correction_memory.normalize_personal_transactions();
                model
                    .correction_memory
                    .enforce_limits(max_rules, DEFAULT_MAX_CONTEXT_ROWS);
                let config = LearningConfigV2::compatibility_v1();
                model
                    .user_language_model
                    .enforce_limits(config.max_unigrams, config.max_bigrams);
                Ok(model)
            }
            _ => Err(ModelError::UnsupportedVersion),
        }
    }

    /// Largest edit id retained by correction metadata for cursor validation.
    #[must_use]
    pub fn max_recorded_edit_id(&self) -> u64 {
        self.correction_memory.max_recorded_edit_id()
    }

    /// Return checkpoint/raw positive and negative totals for one exact bucket.
    #[must_use]
    pub fn evidence_totals(&self, key: &RuleContextKey) -> (f64, f64) {
        self.correction_memory
            .raw_totals(&key.identity(), key.left_token_nfc.as_deref())
    }

    /// Return owned rows without exposing mutable model internals.
    #[must_use]
    pub fn inspection_rows(&self) -> Vec<ModelInspectionRow> {
        self.correction_memory
            .inspection_rows()
            .into_iter()
            .map(|row| ModelInspectionRow {
                input_method: row.identity.input_method,
                source: row.identity.source,
                original_nfc: row.identity.original_nfc,
                candidate_nfc: row.identity.candidate_nfc,
                left_token_nfc: row.left_token_nfc,
                source_rule_id: row.identity.source_rule_id,
                state: row.state,
                evidence_count: row.evidence_count,
                positive_evidence: row.positive_evidence,
                negative_evidence: row.negative_evidence,
                last_evidence_at_ms: row.last_evidence_at_ms,
                shown_count: row.shown_count,
                selected_count: row.selected_count,
                last_shown_at_ms: row.last_shown_at_ms,
                recent_auto_count: row.recent_auto_count,
                recent_undo_count: row.recent_undo_count,
            })
            .collect()
    }

    /// Number of correction/context rows evicted by bounded-store policy.
    #[must_use]
    pub const fn pruned_row_count(&self) -> u64 {
        self.correction_memory.pruned_row_count()
    }

    /// Records an unmatched rewind pair. Returns true when newly promoted.
    pub fn record_personal_correction(
        &mut self,
        input_method: InputMethod,
        original_nfc: impl Into<String>,
        replacement_nfc: impl Into<String>,
        transaction: PersonalTransaction,
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
        let max_personal_pairs = self.config().max_personal_pairs.max(1);
        let max_corrections = self.config().max_rules.max(1);
        self.correction_memory.observe_personal_bounded(
            input_method,
            &original_nfc,
            &replacement_nfc,
            transaction,
            max_personal_pairs,
            max_corrections,
        )
    }

    /// Return observed count for one exact Personal pair.
    #[must_use]
    pub fn personal_correction_count(
        &self,
        input_method: InputMethod,
        original_nfc: &str,
        replacement_nfc: &str,
    ) -> u32 {
        self.correction_memory.personal_observation_count(
            input_method,
            original_nfc,
            replacement_nfc,
        )
    }

    #[must_use]
    pub fn personal_promoted(&self) -> Vec<(InputMethod, String, String)> {
        self.correction_memory.all_promoted_personal()
    }

    /// Physically removes a correction identity and all context/metadata rows.
    pub fn forget_rule(&mut self, key: &RuleContextKey) -> bool {
        self.correction_memory.forget_identity(&key.identity())
    }

    /// Physically removes one Personal pair from probation and promoted state.
    pub fn forget_personal_pair(
        &mut self,
        input_method: InputMethod,
        original_nfc: &str,
        replacement_nfc: &str,
    ) -> bool {
        self.correction_memory
            .forget_personal(input_method, original_nfc, replacement_nfc)
    }

    /// Clears all learned state and restores the canonical cold start.
    pub fn forget_all(&mut self) -> bool {
        let empty = Self::default();
        if self == &empty {
            return false;
        }
        *self = empty;
        true
    }

    /// Forget exactly one identity previously returned by inspection.
    pub fn forget_inspection_row(&mut self, row: &ModelInspectionRow) -> bool {
        self.correction_memory.forget_identity(&CorrectionIdentity {
            input_method: row.input_method,
            source: row.source,
            original_nfc: row.original_nfc.clone(),
            candidate_nfc: row.candidate_nfc.clone(),
            source_rule_id: row.source_rule_id.clone(),
        })
    }

    fn config(&self) -> &ModelConfig {
        &self.maintenance_metadata.model_config
    }

    fn prepare_for(&mut self, key: &RuleContextKey) {
        let max_rules = self.config().max_rules.max(1);
        self.correction_memory.prepare_for(
            &key.identity(),
            key.left_token_nfc.as_deref(),
            max_rules,
            DEFAULT_MAX_CONTEXT_ROWS,
        );
    }

    fn mass(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> (f64, f64) {
        self.correction_memory.blended_mass(
            &key.identity(),
            key.left_token_nfc.as_deref(),
            evaluate_at_ms,
            LearningConfigV2::compatibility_v1().context_shrinkage_k,
        )
    }

    fn migrate_v1(mut legacy: LegacyModelV1) -> Self {
        legacy
            .entries
            .sort_by(|left, right| left.key.cmp(&right.key));
        let mut memory = CorrectionMemory::with_half_life(legacy.config.half_life_ms);
        let mut fallback_states = BTreeMap::<CorrectionIdentity, (bool, DecisionState)>::new();
        let mut legacy_impressions = BTreeMap::<CorrectionIdentity, BTreeSet<u64>>::new();
        let mut legacy_auto_settlements = BTreeMap::<CorrectionIdentity, BTreeSet<u64>>::new();
        let weak_positive_cap = LearningConfigV2::compatibility_v1().weak_positive_cap;
        for entry in legacy.entries {
            migrate_legacy_rule_entry(
                &mut memory,
                &mut fallback_states,
                &mut legacy_impressions,
                &mut legacy_auto_settlements,
                weak_positive_cap,
                entry,
            );
        }
        for (identity, (has_global, state)) in fallback_states {
            if !has_global {
                memory.set_global_fallback_state(&identity, state);
            }
        }
        for (identity, impression_ids) in legacy_impressions {
            memory.import_impressions(
                &identity,
                u64::try_from(impression_ids.len()).unwrap_or(u64::MAX),
            );
        }
        for (identity, settlement_ids) in legacy_auto_settlements {
            let total = f64::from(u32::try_from(settlement_ids.len()).unwrap_or(u32::MAX)) * 0.3;
            memory.import_weak_positive_total(&identity, None, total.min(weak_positive_cap));
        }
        let promoted = legacy
            .personal
            .promoted
            .iter()
            .map(PersonalCountRow::identity_tuple)
            .collect::<BTreeSet<_>>();
        for row in legacy.personal.counts {
            let is_promoted = promoted.contains(&row.identity_tuple());
            memory.import_personal(
                row.input_method,
                &row.original_nfc,
                &row.replacement_nfc,
                row.count,
                is_promoted,
            );
        }
        for row in legacy.personal.promoted {
            if memory.personal_observation_count(
                row.input_method,
                &row.original_nfc,
                &row.replacement_nfc,
            ) == 0
            {
                memory.import_personal(
                    row.input_method,
                    &row.original_nfc,
                    &row.replacement_nfc,
                    0,
                    true,
                );
            }
        }
        memory.enforce_limits(legacy.config.max_rules, DEFAULT_MAX_CONTEXT_ROWS);
        Self {
            version: MODEL_VERSION,
            config_hash: LearningConfigV2::compatibility_v1().hash(),
            correction_memory: memory,
            user_language_model: UserLanguageModel::default(),
            maintenance_metadata: MaintenanceMetadata {
                model_config: legacy.config,
            },
        }
    }
}

fn migrate_legacy_rule_entry(
    memory: &mut CorrectionMemory,
    fallback_states: &mut BTreeMap<CorrectionIdentity, (bool, DecisionState)>,
    legacy_impressions: &mut BTreeMap<CorrectionIdentity, BTreeSet<u64>>,
    legacy_auto_settlements: &mut BTreeMap<CorrectionIdentity, BTreeSet<u64>>,
    weak_positive_cap: f64,
    entry: LegacyRuleEntry,
) {
    let identity = entry.key.identity();
    let fallback = fallback_states
        .entry(identity.clone())
        .or_insert((false, DecisionState::Ignore));
    fallback.0 |= entry.key.left_token_nfc.is_none();
    fallback.1 = fallback.1.max(entry.state);
    legacy_impressions
        .entry(identity.clone())
        .or_default()
        .extend(entry.settled_suggestion_ids.iter().copied());
    legacy_auto_settlements
        .entry(identity.clone())
        .or_default()
        .extend(entry.settled_auto_ids.iter().copied());
    let context_weak_total =
        f64::from(u32::try_from(entry.settled_auto_ids.len()).unwrap_or(u32::MAX)) * 0.3;
    memory.import_weak_positive_total(
        &identity,
        entry.key.left_token_nfc.as_deref(),
        context_weak_total.min(weak_positive_cap),
    );
    let had_legacy_suggestion_settlement = !entry.settled_suggestion_ids.is_empty();
    for evidence in entry.evidence {
        if had_legacy_suggestion_settlement
            && evidence.positive_add.abs() < f64::EPSILON
            && (evidence.negative_add - 0.2).abs() < f64::EPSILON
        {
            continue;
        }
        memory.apply(
            &identity,
            entry.key.left_token_nfc.as_deref(),
            CorrectionEvidence {
                seq: evidence.seq,
                at_ms: evidence.at_ms,
                positive: evidence.positive_add,
                negative: evidence.negative_add,
            },
        );
    }
    memory.record_state(&identity, entry.key.left_token_nfc.as_deref(), entry.state);
    let recent_auto = entry
        .recent_auto
        .iter()
        .map(|item| (item.edit_id, item.at_ms, item.undone))
        .collect::<Vec<_>>();
    memory.import_operational_metadata(
        &identity,
        entry.key.left_token_nfc.as_deref(),
        &ImportedOperationalMetadata {
            recent_auto: &recent_auto,
            handled_sequences: &entry.handled_feedback_seqs,
            settled_auto_ids: &entry.settled_auto_ids,
            settled_suggestion_ids: &entry.settled_suggestion_ids,
            auto_demoted_at_seq: entry.auto_demoted_at_seq,
        },
    );
}

impl Default for AdaptiveModel {
    fn default() -> Self {
        Self::new(ModelConfig::default())
    }
}

impl ModelView for AdaptiveModel {
    fn confidence(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> f64 {
        self.correction_memory.blended_confidence(
            &key.identity(),
            key.left_token_nfc.as_deref(),
            evaluate_at_ms,
            LearningConfigV2::compatibility_v1().context_shrinkage_k,
        )
    }

    fn positive_mass(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> f64 {
        self.mass(key, evaluate_at_ms).0
    }

    fn state(&self, key: &RuleContextKey, _evaluate_at_ms: i64) -> DecisionState {
        self.correction_memory
            .query_state(&key.identity(), key.left_token_nfc.as_deref())
    }

    fn auto_allowed(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> bool {
        self.correction_memory.auto_allowed(
            &key.identity(),
            key.left_token_nfc.as_deref(),
            evaluate_at_ms,
        )
    }

    fn unigram_signal(&self, token_nfc: &str) -> f64 {
        self.user_language_model.unigram_signal(token_nfc)
    }

    fn bigram_signal(&self, left_token_nfc: &str, token_nfc: &str) -> f64 {
        self.user_language_model
            .bigram_signal(left_token_nfc, token_nfc)
    }
}

#[derive(Debug, Deserialize)]
struct PayloadVersion {
    version: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct LegacyEvidenceRecord {
    seq: u64,
    at_ms: i64,
    positive_add: f64,
    negative_add: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LegacyAutoEmission {
    edit_id: u64,
    at_ms: i64,
    undone: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct LegacyRuleEntry {
    key: RuleContextKey,
    state: DecisionState,
    evidence: Vec<LegacyEvidenceRecord>,
    recent_auto: Vec<LegacyAutoEmission>,
    handled_feedback_seqs: BTreeSet<u64>,
    settled_auto_ids: BTreeSet<u64>,
    settled_suggestion_ids: BTreeSet<u64>,
    #[serde(default)]
    auto_demoted_at_seq: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PersonalCountRow {
    input_method: InputMethod,
    original_nfc: String,
    replacement_nfc: String,
    count: u32,
}

impl PersonalCountRow {
    fn identity_tuple(&self) -> (InputMethod, String, String) {
        (
            self.input_method,
            self.original_nfc.clone(),
            self.replacement_nfc.clone(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
struct LegacyPersonalCorrectionStore {
    #[serde(default)]
    counts: Vec<PersonalCountRow>,
    #[serde(default)]
    promoted: Vec<PersonalCountRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct LegacyModelV1 {
    version: u32,
    config: ModelConfig,
    entries: Vec<LegacyRuleEntry>,
    #[serde(default)]
    personal: LegacyPersonalCorrectionStore,
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
