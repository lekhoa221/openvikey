//! Feedback integration, bounded auto-edit undo, and implicit correction mining.

use crate::model::{AdaptiveModel, RuleContextKey};
use crate::types::{FeedbackEvent, FeedbackKind, ReplaceRangeAction, UndoTracker};
use std::collections::{BTreeMap, VecDeque};

/// Result of undoing one auto-applied edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoOutcome {
    pub inverse: ReplaceRangeAction,
    pub feedback: FeedbackEvent,
}

/// Couples an adaptive model with a bounded semantic edit log.
#[derive(Debug, Clone)]
pub struct LearningSession {
    model: AdaptiveModel,
    undo: UndoTracker,
    max_undo_entries: usize,
    edit_rules: BTreeMap<u64, RuleContextKey>,
    edit_order: VecDeque<u64>,
}

impl LearningSession {
    #[must_use]
    pub fn new(model: AdaptiveModel, max_undo_entries: usize) -> Self {
        let max_undo_entries = max_undo_entries.max(1);
        Self {
            model,
            undo: UndoTracker::new(max_undo_entries),
            max_undo_entries,
            edit_rules: BTreeMap::new(),
            edit_order: VecDeque::with_capacity(max_undo_entries),
        }
    }

    #[must_use]
    pub fn model(&self) -> &AdaptiveModel {
        &self.model
    }

    pub fn model_mut(&mut self) -> &mut AdaptiveModel {
        &mut self.model
    }

    /// Records an auto edit in both the semantic undo log and the rule window.
    pub fn record_auto_edit(
        &mut self,
        key: RuleContextKey,
        action: ReplaceRangeAction,
        at_ms: i64,
    ) {
        self.model.record_auto_emission(&key, action.edit_id, at_ms);
        while self.edit_order.len() >= self.max_undo_entries {
            if let Some(expired) = self.edit_order.pop_front() {
                self.edit_rules.remove(&expired);
            }
        }
        self.edit_order.push_back(action.edit_id);
        self.edit_rules.insert(action.edit_id, key);
        self.undo.record_edit(action);
    }

    /// Produces the exact inverse edit and, when allowed, negative evidence.
    pub fn undo(
        &mut self,
        expected_revision: u64,
        seq: u64,
        at_ms: i64,
        allow_learning: bool,
    ) -> Option<UndoOutcome> {
        let inverse = self.undo.pop_undo(expected_revision)?;
        let key = self.edit_rules.remove(&inverse.edit_id)?;
        self.edit_order
            .retain(|edit_id| *edit_id != inverse.edit_id);
        let feedback = FeedbackEvent {
            seq,
            at_ms,
            kind: FeedbackKind::Undo {
                edit_id: inverse.edit_id,
            },
        };
        self.model.apply_feedback(&key, &feedback, allow_learning);
        Some(UndoOutcome { inverse, feedback })
    }

    pub fn invalidate_due_to_caret_break(&mut self) {
        self.undo.invalidate_due_to_caret_break();
        self.edit_rules.clear();
        self.edit_order.clear();
    }
}

/// Minimal contiguous delete→retype miner. Caret/selection breaks invalidate it.
#[derive(Debug, Clone, Default)]
pub struct ImplicitCorrectionMiner {
    deleted_token: Option<String>,
    is_valid: bool,
}

impl ImplicitCorrectionMiner {
    pub fn record_deleted_token(&mut self, original: impl Into<String>) {
        self.deleted_token = Some(original.into());
        self.is_valid = true;
    }

    pub fn invalidate_due_to_caret_break(&mut self) {
        self.deleted_token = None;
        self.is_valid = false;
    }

    pub fn finish_replacement(
        &mut self,
        replacement: impl Into<String>,
        seq: u64,
        at_ms: i64,
    ) -> Option<FeedbackEvent> {
        if !self.is_valid {
            return None;
        }
        let original = self.deleted_token.take()?;
        self.is_valid = false;
        let replacement = replacement.into();
        if original.is_empty() || replacement.is_empty() || original == replacement {
            return None;
        }
        Some(FeedbackEvent {
            seq,
            at_ms,
            kind: FeedbackKind::ImplicitCorrection {
                original,
                replacement,
            },
        })
    }
}
