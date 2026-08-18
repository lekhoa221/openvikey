//! Feedback integration, bounded auto-edit undo, and implicit correction mining.

use crate::model::{AdaptiveModel, RuleContextKey};
use crate::types::{
    Candidate, FeedbackEvent, FeedbackKind, InputMethod, ReplaceRangeAction, UndoTracker,
};
use std::collections::{BTreeMap, VecDeque};

const IMPLICIT_MAX_DURATION_MS: i64 = 10_000;

/// Result of undoing one auto-applied edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoOutcome {
    pub inverse: ReplaceRangeAction,
    pub feedback: FeedbackEvent,
}

#[derive(Debug, Clone)]
struct PendingAutoSettlement {
    key: RuleContextKey,
    edit_id: u64,
    remaining_events: u8,
    started_at_ms: i64,
}

/// Couples an adaptive model with a bounded semantic edit log.
#[derive(Debug, Clone)]
pub struct LearningSession {
    model: AdaptiveModel,
    undo: UndoTracker,
    max_undo_entries: usize,
    edit_rules: BTreeMap<u64, RuleContextKey>,
    edit_order: VecDeque<u64>,
    pending_auto_settlements: Vec<PendingAutoSettlement>,
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
            pending_auto_settlements: Vec::with_capacity(max_undo_entries),
        }
    }

    #[must_use]
    pub fn model(&self) -> &AdaptiveModel {
        &self.model
    }

    pub fn model_mut(&mut self) -> &mut AdaptiveModel {
        &mut self.model
    }

    /// Persists an operational decision transition when learning is allowed.
    pub fn record_decision(
        &mut self,
        key: &RuleContextKey,
        state: crate::decision::DecisionState,
        allow_learning: bool,
    ) {
        self.model.record_decision(key, state, allow_learning);
    }

    /// Records an auto edit in the semantic undo log and, when allowed, the learning window.
    pub fn record_auto_edit(
        &mut self,
        key: RuleContextKey,
        action: ReplaceRangeAction,
        at_ms: i64,
        allow_learning: bool,
    ) {
        self.model
            .record_auto_emission(&key, action.edit_id, at_ms, allow_learning);
        while self.edit_order.len() >= self.max_undo_entries {
            if let Some(expired) = self.edit_order.pop_front() {
                self.edit_rules.remove(&expired);
                self.pending_auto_settlements
                    .retain(|pending| pending.edit_id != expired);
            }
        }
        self.edit_order.push_back(action.edit_id);
        self.edit_rules.insert(action.edit_id, key.clone());
        self.pending_auto_settlements
            .retain(|pending| pending.edit_id != action.edit_id);
        if allow_learning {
            self.pending_auto_settlements.push(PendingAutoSettlement {
                key,
                edit_id: action.edit_id,
                remaining_events: 10,
                started_at_ms: at_ms,
            });
        }
        self.undo.record_edit(action);
    }

    /// Advances pending auto settlements by one subsequent input/edit event.
    pub fn observe_input_or_edit(
        &mut self,
        first_feedback_seq: u64,
        at_ms: i64,
        allow_learning: bool,
    ) -> Vec<FeedbackEvent> {
        if !allow_learning {
            return Vec::new();
        }
        for pending in &mut self.pending_auto_settlements {
            pending.remaining_events = pending.remaining_events.saturating_sub(1);
        }
        let mut ready = Vec::new();
        self.pending_auto_settlements.retain(|pending| {
            if pending.remaining_events != 0 || at_ms.saturating_sub(pending.started_at_ms) < 3_000
            {
                return true;
            }
            ready.push((pending.key.clone(), pending.edit_id));
            false
        });
        let mut settled = Vec::new();
        for (key, edit_id) in ready {
            let event = FeedbackEvent {
                seq: first_feedback_seq.saturating_add(settled.len() as u64),
                at_ms,
                kind: FeedbackKind::AutoSettled { edit_id },
            };
            self.model.apply_feedback(&key, &event, true);
            settled.push(event);
        }
        settled
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
        self.pending_auto_settlements
            .retain(|pending| pending.edit_id != inverse.edit_id);
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
        self.pending_auto_settlements.clear();
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

/// Peak composition captured at the start of a Backspace rewind.
#[derive(Debug, Clone)]
pub struct CompositionPeak {
    pub original_nfc: String,
    pub raw_keys: String,
    pub candidates: Vec<Candidate>,
    pub left_token_nfc: Option<String>,
    pub input_method: InputMethod,
    pub started_at_ms: i64,
}

#[derive(Debug, Clone, Default)]
enum RewindPhase {
    #[default]
    Idle,
    Rewinding(CompositionPeak),
    Typing(CompositionPeak),
}

/// Composition-session rewind miner. Last peak wins; VALUE is committed at Space.
#[derive(Debug, Clone, Default)]
pub struct CompositionRewindMiner {
    phase: RewindPhase,
}

/// Outcome of evaluating a rewind against the committed VALUE.
#[derive(Debug, Clone, PartialEq)]
pub enum RewindEvaluate {
    Matched {
        key: RuleContextKey,
        feedback: FeedbackEvent,
    },
    Unmatched {
        original_nfc: String,
        replacement_nfc: String,
        input_method: InputMethod,
    },
    Ignored,
}

impl CompositionRewindMiner {
    #[must_use]
    pub fn is_active(&self) -> bool {
        !matches!(self.phase, RewindPhase::Idle)
    }

    pub fn on_backspace(&mut self, peak: CompositionPeak) {
        match &self.phase {
            RewindPhase::Idle | RewindPhase::Typing(_) => {
                self.phase = RewindPhase::Rewinding(peak);
            }
            RewindPhase::Rewinding(_) => {}
        }
    }

    pub fn on_key(&mut self) {
        if let RewindPhase::Rewinding(peak) = &self.phase {
            self.phase = RewindPhase::Typing(peak.clone());
        }
    }

    pub fn invalidate(&mut self) {
        self.phase = RewindPhase::Idle;
    }

    pub fn evaluate(&mut self, value: &str, seq: u64, at_ms: i64) -> RewindEvaluate {
        let peak = match std::mem::take(&mut self.phase) {
            RewindPhase::Idle => return RewindEvaluate::Ignored,
            RewindPhase::Rewinding(peak) | RewindPhase::Typing(peak) => peak,
        };
        if at_ms.saturating_sub(peak.started_at_ms) > IMPLICIT_MAX_DURATION_MS {
            return RewindEvaluate::Ignored;
        }
        if value.is_empty() || value == peak.original_nfc {
            return RewindEvaluate::Ignored;
        }
        if let Some(candidate) = peak
            .candidates
            .iter()
            .find(|candidate| candidate.text == value)
        {
            let key = RuleContextKey {
                input_method: peak.input_method,
                source: candidate.source,
                original_nfc: peak.original_nfc.clone(),
                candidate_nfc: candidate.text.clone(),
                left_token_nfc: peak.left_token_nfc.clone(),
                source_rule_id: candidate
                    .evidence
                    .split('+')
                    .next()
                    .unwrap_or("")
                    .to_string(),
            };
            RewindEvaluate::Matched {
                key,
                feedback: FeedbackEvent {
                    seq,
                    at_ms,
                    kind: FeedbackKind::ImplicitCorrection {
                        original: peak.original_nfc,
                        replacement: value.to_string(),
                    },
                },
            }
        } else {
            RewindEvaluate::Unmatched {
                original_nfc: peak.original_nfc,
                replacement_nfc: value.to_string(),
                input_method: peak.input_method,
            }
        }
    }

    #[must_use]
    pub fn last_peak_original(&self) -> Option<&str> {
        match &self.phase {
            RewindPhase::Idle => None,
            RewindPhase::Rewinding(peak) | RewindPhase::Typing(peak) => {
                Some(peak.original_nfc.as_str())
            }
        }
    }
}
