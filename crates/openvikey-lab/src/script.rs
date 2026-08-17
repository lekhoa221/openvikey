//! Deterministic JSONL learning-script replay.

use openvikey_core::model::{AdaptiveModel, ModelError, RuleContextKey};
use openvikey_core::types::{FeedbackEvent, FeedbackKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScriptError {
    #[error("invalid JSON at line {line}: {source}")]
    JsonLine {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum ScriptOperation {
    Accept {
        seq: u64,
        at_ms: i64,
        candidate_id: u64,
        rule: RuleContextKey,
        #[serde(default = "default_true")]
        allow_learning: bool,
    },
    Reject {
        seq: u64,
        at_ms: i64,
        candidate_id: u64,
        rule: RuleContextKey,
        #[serde(default = "default_true")]
        allow_learning: bool,
    },
    AutoEmission {
        edit_id: u64,
        at_ms: i64,
        rule: RuleContextKey,
        #[serde(default = "default_true")]
        allow_learning: bool,
    },
    Undo {
        seq: u64,
        at_ms: i64,
        edit_id: u64,
        rule: RuleContextKey,
        #[serde(default = "default_true")]
        allow_learning: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScriptReport {
    pub schema_version: u32,
    pub script_sha256: String,
    pub model_sha256: String,
    pub operations_applied: u64,
    pub feedback_events_applied: u64,
}

impl ScriptReport {
    pub fn to_pretty_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut bytes = serde_json::to_vec_pretty(self)?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

pub fn run_script_jsonl(script: &str) -> Result<ScriptReport, ScriptError> {
    let mut model = AdaptiveModel::default();
    let mut operations = 0_u64;
    let mut feedback_events = 0_u64;
    for (index, raw) in script.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let operation: ScriptOperation =
            serde_json::from_str(line).map_err(|source| ScriptError::JsonLine {
                line: index + 1,
                source,
            })?;
        operations = operations.saturating_add(1);
        match operation {
            ScriptOperation::Accept {
                seq,
                at_ms,
                candidate_id,
                rule,
                allow_learning,
            } => {
                model.apply_feedback(
                    &rule,
                    &FeedbackEvent {
                        seq,
                        at_ms,
                        kind: FeedbackKind::Accept { candidate_id },
                    },
                    allow_learning,
                );
                feedback_events = feedback_events.saturating_add(1);
            }
            ScriptOperation::Reject {
                seq,
                at_ms,
                candidate_id,
                rule,
                allow_learning,
            } => {
                model.apply_feedback(
                    &rule,
                    &FeedbackEvent {
                        seq,
                        at_ms,
                        kind: FeedbackKind::ExplicitReject { candidate_id },
                    },
                    allow_learning,
                );
                feedback_events = feedback_events.saturating_add(1);
            }
            ScriptOperation::AutoEmission {
                edit_id,
                at_ms,
                rule,
                allow_learning,
            } => model.record_auto_emission(&rule, edit_id, at_ms, allow_learning),
            ScriptOperation::Undo {
                seq,
                at_ms,
                edit_id,
                rule,
                allow_learning,
            } => {
                model.apply_feedback(
                    &rule,
                    &FeedbackEvent {
                        seq,
                        at_ms,
                        kind: FeedbackKind::Undo { edit_id },
                    },
                    allow_learning,
                );
                feedback_events = feedback_events.saturating_add(1);
            }
        }
    }
    let payload = model.to_json_payload()?;
    Ok(ScriptReport {
        schema_version: 1,
        script_sha256: sha256(script.as_bytes()),
        model_sha256: sha256(&payload),
        operations_applied: operations,
        feedback_events_applied: feedback_events,
    })
}

const fn default_true() -> bool {
    true
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
