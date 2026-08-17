//! Personal model view.
//!
//! Wave 0 locks a read-only `ModelView` plus an empty adapter. Milestone 6
//! replaces the empty body with event-backed Beta mass. Store (M8) persists
//! the JSON payload without knowing Beta internals.

use crate::decision::DecisionState;
use crate::types::{CandidateSource, FeedbackEvent, InputMethod};
use serde::{Deserialize, Serialize};

/// Stable learning key. Two original→candidate pairs never share evidence.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    fn state(&self, key: &RuleContextKey, evaluate_at_ms: i64) -> DecisionState;
}

/// Versioned JSON envelope. M6 fills `entries`; Wave 0 keeps them empty.
const EMPTY_MODEL_JSON: &str = "{\"version\":1,\"entries\":[]}";

/// Cold-start model: Beta(1,1) prior, no stored evidence, feedback is a no-op.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EmptyModel;

impl EmptyModel {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Wave 0 no-op. Milestone 6 records evidence here.
    pub fn apply_feedback(&mut self, _event: &FeedbackEvent, _key: &RuleContextKey) {}

    /// Opaque versioned JSON. Store must treat this as bytes, not a typed model.
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
        // Beta(1,1) ⇒ (1)/(1+1) = 0.5
        0.5
    }

    fn state(&self, _key: &RuleContextKey, _evaluate_at_ms: i64) -> DecisionState {
        DecisionState::Ignore
    }
}

/// Errors when reading a model payload. Distinct from store envelope errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    UnsupportedVersion,
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion => write!(f, "unsupported model payload version"),
        }
    }
}

impl std::error::Error for ModelError {}
