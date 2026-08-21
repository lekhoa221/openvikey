//! Observable deterministic Engine → correction session used by the lab CLI.

use crate::capture::{
    CAPTURE_VERSION, CaptureHeader, CaptureLog, CaptureRecord, MAX_CAPTURE_RECORDS,
    compact_capture_after_forget, correction_identity_hash, sha256_hex, trim_capture_to,
};
use crate::document::{CommittedUnit, DocumentBuffer};
use openvikey_core::correction::{
    AutoEditContext, CorrectionSlice, InterventionConfig, candidate_rule_key,
    run_learning_correction_slice_with_guard,
};
use openvikey_core::correction_memory::PersonalTransaction;
use openvikey_core::decision::{DecisionConfig, DecisionState};
use openvikey_core::engine::{Engine, EngineConfig};
use openvikey_core::feedback::{
    CompositionPeak, CompositionRewindMiner, ImplicitCorrectionMiner, LearningSession,
    RewindEvaluate,
};
use openvikey_core::generate::abbrev::AbbrevGenerator;
use openvikey_core::generate::diacritics::DiacriticsGenerator;
use openvikey_core::generate::fuzzy::FuzzyGenerator;
use openvikey_core::generate::personal::PersonalGenerator;
use openvikey_core::generate::telex_fix::TelexFixGenerator;
use openvikey_core::generate::{Generator, LeftContext};
use openvikey_core::intervention::{
    CorrectionIdentity, InterventionReason, RevertGuard, correction_identity,
};
use openvikey_core::learning_config::LearningConfigV2;
use openvikey_core::lexicon::Lexicon;
use openvikey_core::model::{AdaptiveModel, ModelError, ModelInspectionRow, RuleContextKey};
use openvikey_core::rank::ScoreConfig;
use openvikey_core::types::{
    Candidate, CandidateSource, CompositionSnapshot, EditRange, EngineAction, FeedbackEvent,
    FeedbackKind, InputContext, InputEvent, InputKind, InputMethod, Modifiers, RangeBasis,
};
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

const MAX_EXTERNAL_LEFT_TOKEN_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionCursors {
    pub next_seq: u64,
    pub next_edit_id: u64,
}

impl Default for SessionCursors {
    fn default() -> Self {
        Self {
            next_seq: 1,
            next_edit_id: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionObservation {
    pub event_seq: u64,
    pub snapshot: CompositionSnapshot,
    pub engine_actions: Vec<EngineAction>,
    pub candidates: Vec<Candidate>,
    pub decision: Option<DecisionState>,
    pub action: Option<EngineAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptVisual {
    pub candidate_nfc: String,
    pub was_composing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoVisual {
    pub show_nfc: String,
    pub restored_composition: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearningNoticeKind {
    Accepted,
    Observed,
    Promoted,
    Forgotten,
    Corrected,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LearningNotice {
    pub kind: LearningNoticeKind,
    pub original_nfc: String,
    pub replacement_nfc: String,
    /// Evidence added by the action that produced this notice.
    pub positive_delta: f64,
    pub negative_delta: f64,
    /// Raw evidence totals for this exact rule after the action.
    pub positive_total: f64,
    pub negative_total: f64,
}

impl LearningNotice {
    #[must_use]
    pub fn display_text(&self) -> String {
        let action = match self.kind {
            LearningNoticeKind::Accepted => "Đã học",
            LearningNoticeKind::Observed => "Đã ghi nhận",
            LearningNoticeKind::Promoted => "Đã tạo gợi ý cá nhân",
            LearningNoticeKind::Forgotten => "Đã quên",
            LearningNoticeKind::Corrected => {
                return format!(
                    "Đã sửa: {} → {} · Backspace để hoàn tác",
                    self.original_nfc, self.replacement_nfc
                );
            }
        };
        if self.kind == LearningNoticeKind::Forgotten {
            return format!(
                "{action}: {} → {} · Điểm tích lũy 0",
                self.original_nfc, self.replacement_nfc
            );
        }
        let delta = if self.positive_delta > 0.0 && self.negative_delta > 0.0 {
            format!("+{:.1}/-{:.1}", self.positive_delta, self.negative_delta)
        } else if self.positive_delta > 0.0 {
            format!("+{:.1}", self.positive_delta)
        } else {
            format!("-{:.1}", self.negative_delta)
        };
        format!(
            "{action}: {} → {} · {delta} điểm · Tích lũy +{:.1}/-{:.1}",
            self.original_nfc, self.replacement_nfc, self.positive_total, self.negative_total
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LastLearned {
    Rule(RuleContextKey),
    Personal {
        input_method: InputMethod,
        original_nfc: String,
        replacement_nfc: String,
    },
}

fn last_learned_matches_row(last: &LastLearned, row: &ModelInspectionRow) -> bool {
    match last {
        LastLearned::Rule(key) => {
            key.input_method == row.input_method
                && key.source == row.source
                && key.original_nfc == row.original_nfc
                && key.candidate_nfc == row.candidate_nfc
                && key.left_token_nfc == row.left_token_nfc
                && key.source_rule_id == row.source_rule_id
        }
        LastLearned::Personal {
            input_method,
            original_nfc,
            replacement_nfc,
        } => {
            row.source == openvikey_core::types::CandidateSource::Personal
                && *input_method == row.input_method
                && *original_nfc == row.original_nfc
                && *replacement_nfc == row.candidate_nfc
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingPolicyUndo {
    raw_token: String,
    identity: CorrectionIdentity,
    rule_key: RuleContextKey,
    edit_id: u64,
    applied_at_ms: i64,
    learn_undo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingConfirmedReject {
    raw_token: String,
    rule_key: RuleContextKey,
    edit_id: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionSaveSnapshot {
    pub model: AdaptiveModel,
    pub capture_records: Vec<CaptureRecord>,
    pub cursors: SessionCursors,
    pub last_at_ms: i64,
}

pub struct LabSession {
    engine: Engine,
    lexicon: Lexicon,
    abbrev: AbbrevGenerator,
    learning: LearningSession,
    left_context: LeftContext,
    next_seq: u64,
    next_edit_id: u64,
    score_config: ScoreConfig,
    decision_config: DecisionConfig,
    document: DocumentBuffer,
    miner: ImplicitCorrectionMiner,
    mining_snapshot: Option<CommittedUnit>,
    last_slice: Option<CorrectionSlice>,
    last_original_nfc: String,
    last_left_token: Option<String>,
    last_method: InputMethod,
    last_auto_revision: Option<u64>,
    last_auto_token: Option<String>,
    last_at_ms: i64,
    capture: Vec<CaptureRecord>,
    capturing: bool,
    rewind: CompositionRewindMiner,
    intervention: InterventionConfig,
    learning_config_hash: String,
    pending_policy_undo: Option<PendingPolicyUndo>,
    pending_confirmed_reject: Option<PendingConfirmedReject>,
    revert_guard: Option<RevertGuard>,
    focus_generation: u64,
    pending_reopen: bool,
    last_learned: Option<LastLearned>,
    pending_learning_notice: Option<LearningNotice>,
}

impl LabSession {
    #[must_use]
    pub fn new(engine_config: EngineConfig, lexicon: Lexicon) -> Self {
        Self::new_with_model(
            engine_config,
            lexicon,
            AdaptiveModel::default(),
            SessionCursors::default(),
        )
    }

    #[must_use]
    pub fn new_with_model(
        engine_config: EngineConfig,
        lexicon: Lexicon,
        model: AdaptiveModel,
        cursors: SessionCursors,
    ) -> Self {
        let method = engine_config.method;
        Self {
            engine: Engine::new(engine_config),
            lexicon,
            abbrev: AbbrevGenerator::from_seed(),
            learning: LearningSession::new(model, 32),
            left_context: LeftContext::default(),
            next_seq: cursors.next_seq.max(1),
            next_edit_id: cursors.next_edit_id.max(1),
            score_config: ScoreConfig::default(),
            decision_config: DecisionConfig::default(),
            document: DocumentBuffer::new(),
            miner: ImplicitCorrectionMiner::default(),
            mining_snapshot: None,
            last_slice: None,
            last_original_nfc: String::new(),
            last_left_token: None,
            last_method: method,
            last_auto_revision: None,
            last_auto_token: None,
            last_at_ms: 0,
            capture: Vec::new(),
            capturing: true,
            rewind: CompositionRewindMiner::default(),
            intervention: InterventionConfig::win32(),
            learning_config_hash: LearningConfigV2::compatibility_v1().hash(),
            pending_policy_undo: None,
            pending_confirmed_reject: None,
            revert_guard: None,
            focus_generation: 0,
            pending_reopen: false,
            last_learned: None,
            pending_learning_notice: None,
        }
    }

    pub fn set_intervention_config(&mut self, config: InterventionConfig) {
        self.intervention = config;
    }

    #[must_use]
    pub fn intervention_config(&self) -> InterventionConfig {
        self.intervention
    }

    pub fn type_text(
        &mut self,
        text: &str,
        context: InputContext,
        first_at_ms: i64,
    ) -> Vec<SessionObservation> {
        text.chars()
            .enumerate()
            .map(|(index, logical)| {
                let offset = i64::try_from(index).unwrap_or(i64::MAX);
                self.inject(
                    InputKind::Key {
                        logical,
                        physical: None,
                    },
                    context,
                    first_at_ms.saturating_add(offset),
                )
            })
            .collect()
    }

    pub fn inject(
        &mut self,
        kind: InputKind,
        context: InputContext,
        at_ms: i64,
    ) -> SessionObservation {
        let seq = self.take_seq();
        self.process_event(&InputEvent {
            seq,
            at_ms,
            kind,
            modifiers: Modifiers::empty(),
            is_repeat: false,
            context,
        })
    }

    // Keep the event state transitions together so their ordering stays explicit.
    #[allow(clippy::too_many_lines)]
    pub fn process_event(&mut self, event: &InputEvent) -> SessionObservation {
        let mut event = event.clone();
        event.kind = map_extra_boundary_kind(event.kind);
        self.next_seq = self.next_seq.max(event.seq.saturating_add(1));
        self.last_at_ms = self.last_at_ms.max(event.at_ms);
        let allow_transform = event.context.allow_transform;
        let allow_learning = event.context.allow_learning;
        if self.capturing && allow_transform && allow_learning {
            self.record_capture(CaptureRecord::Input {
                event: event.clone(),
            });
        }
        if matches!(
            event.kind,
            InputKind::CursorMoved | InputKind::SelectionChanged | InputKind::Reset
        ) {
            self.invalidate_caret();
        }
        if matches!(event.kind, InputKind::InsertText { .. }) {
            self.rewind.invalidate();
        }
        if self.revert_guard.is_some()
            && matches!(
                event.kind,
                InputKind::Key { .. } | InputKind::Backspace | InputKind::InsertText { .. }
            )
        {
            self.revert_guard = None;
            self.pending_confirmed_reject = None;
        }
        if self.pending_reopen {
            if matches!(event.kind, InputKind::Key { .. }) && allow_transform {
                let _ = self.reopen_previous_token();
            } else {
                self.pending_reopen = false;
            }
        }

        let before = self.engine.snapshot();
        if before.is_empty() && !matches!(event.kind, InputKind::Backspace) {
            self.pending_policy_undo = None;
        }
        let peak_candidates = self
            .last_slice
            .as_ref()
            .map(|slice| slice.display_candidates().to_vec());
        if matches!(event.kind, InputKind::Backspace)
            && !before.is_empty()
            && allow_learning
            && let Some(candidates) = peak_candidates
        {
            self.rewind.on_backspace(CompositionPeak {
                original_nfc: before.normalized.clone(),
                raw_keys: before.raw_keys.clone(),
                candidates,
                left_token_nfc: self.left_context.prev_token_nfc.clone(),
                input_method: self.engine.config().method,
                started_at_ms: event.at_ms,
            });
        } else if matches!(event.kind, InputKind::Key { .. }) && self.rewind.is_active() {
            self.rewind.on_key();
        }
        if matches!(event.kind, InputKind::Backspace)
            && before.is_empty()
            && self.rewind.is_active()
        {
            self.rewind.invalidate();
        }

        if matches!(event.kind, InputKind::Backspace)
            && before.is_empty()
            && self.try_restore_policy_undo(event.seq, event.at_ms, allow_learning)
        {
            let snapshot = self.engine.snapshot();
            return SessionObservation {
                event_seq: event.seq,
                snapshot: snapshot.clone(),
                engine_actions: vec![EngineAction::UpdateComposition {
                    revision: snapshot.revision,
                    text: snapshot.rendered.clone(),
                }],
                candidates: Vec::new(),
                decision: None,
                action: None,
            };
        }

        let engine_actions = self.engine.process(&event);
        self.pop_document_on_empty_backspace(
            &event.kind,
            before.is_empty(),
            allow_transform,
            allow_learning,
        );
        let commits = Self::commits(&engine_actions, allow_transform);
        let primary = commits.iter().find(|(text, _)| !text.is_empty()).cloned();
        let snapshot = if primary.is_some() && !before.is_empty() {
            before
        } else {
            self.engine.snapshot()
        };
        let method = self.engine.config().method;
        let slice = self.correction_slice(
            &snapshot,
            &event,
            primary.is_some(),
            primary.as_ref().and_then(|(_, delimiter)| *delimiter),
        );
        self.last_slice = Some(slice.clone());
        self.last_original_nfc.clone_from(&snapshot.normalized);
        self.last_left_token
            .clone_from(&self.left_context.prev_token_nfc);
        self.last_method = method;
        let snapshot_raw_keys = snapshot.raw_keys.clone();
        let observation = SessionObservation {
            event_seq: event.seq,
            snapshot,
            engine_actions,
            candidates: slice.display_candidates().to_vec(),
            decision: slice.decision,
            action: slice.action.clone(),
        };
        let recorded_auto = matches!(&slice.action, Some(EngineAction::ReplaceRange(_)));
        let mut used_slice = false;
        for (text, delimiter) in commits {
            if !used_slice && !text.is_empty() {
                self.commit_token(
                    &text,
                    delimiter,
                    &snapshot_raw_keys,
                    &slice,
                    &event,
                    allow_learning,
                );
                used_slice = true;
            } else {
                if text.is_empty() {
                    self.rewind.invalidate();
                }
                self.push_plain_commit(&text, delimiter, method, &event, allow_learning);
                self.clear_auto_anchor();
            }
        }
        if allow_learning && !recorded_auto {
            let settled = self
                .learning
                .observe_input_or_edit(self.next_seq, event.at_ms, true);
            if self.capturing {
                for feedback in &settled {
                    if let FeedbackKind::AutoSettled { edit_id } = feedback.kind {
                        self.record_capture(CaptureRecord::InterventionSettled {
                            seq: feedback.seq,
                            at_ms: feedback.at_ms,
                            edit_id,
                        });
                    }
                }
            }
            self.next_seq = self
                .next_seq
                .saturating_add(u64::try_from(settled.len()).unwrap_or(0));
        }
        observation
    }

    pub fn accept_top(&mut self, at_ms: i64) -> Option<AcceptVisual> {
        self.accept_top_with_learning(at_ms, true)
    }

    pub fn accept_top_with_learning(
        &mut self,
        at_ms: i64,
        allow_learning: bool,
    ) -> Option<AcceptVisual> {
        let slice = self.last_slice.clone()?;
        let top = slice.display_candidates().first().cloned()?;
        let was_composing = !self.engine.snapshot().is_empty();
        let seq = self.take_seq();
        if self.capturing && allow_learning {
            self.record_capture(CaptureRecord::AcceptTop { seq, at_ms });
        }
        let key = self.top_rule_key(&top);
        let feedback = FeedbackEvent {
            seq,
            at_ms,
            kind: FeedbackKind::Accept {
                candidate_id: top.id,
            },
        };
        let (positive_delta, negative_delta) =
            self.learning
                .model_mut()
                .apply_feedback(&key, &feedback, allow_learning);
        if self.capturing && allow_learning {
            self.record_capture(CaptureRecord::CorrectionConfirmed {
                seq,
                at_ms,
                identity: rule_identity(&key),
                left_token_nfc: key.left_token_nfc.clone(),
            });
        }
        if allow_learning {
            let (positive_total, negative_total) = self.learning.model().evidence_totals(&key);
            self.pending_learning_notice = Some(LearningNotice {
                kind: LearningNoticeKind::Accepted,
                original_nfc: key.original_nfc.clone(),
                replacement_nfc: key.candidate_nfc.clone(),
                positive_delta,
                negative_delta,
                positive_total,
                negative_total,
            });
            self.last_learned = Some(LastLearned::Rule(key));
        }
        self.clear_auto_anchor();
        let candidate_nfc = top.text.clone();
        if was_composing {
            let raw_keys = self.engine.snapshot().raw_keys;
            self.reset_engine(at_ms);
            self.document.push_commit(CommittedUnit::new(
                top.text,
                Some(' '),
                self.last_original_nfc.clone(),
                Some(raw_keys),
                self.last_left_token.clone(),
                self.last_method,
                slice.display_candidates().to_vec(),
            ));
        } else {
            self.document.replace_last_token(top.text);
        }
        self.sync_left_context();
        self.last_slice = None;
        Some(AcceptVisual {
            candidate_nfc,
            was_composing,
        })
    }

    pub fn reject_top(&mut self, at_ms: i64) {
        self.reject_top_with_learning(at_ms, true);
    }

    pub fn reject_top_with_learning(&mut self, at_ms: i64, allow_learning: bool) {
        let Some(slice) = self.last_slice.clone() else {
            return;
        };
        let Some(top) = slice.display_candidates().first().cloned() else {
            return;
        };
        let seq = self.take_seq();
        if self.capturing && allow_learning {
            self.record_capture(CaptureRecord::RejectTop { seq, at_ms });
        }
        let key = self.top_rule_key(&top);
        let feedback = FeedbackEvent {
            seq,
            at_ms,
            kind: FeedbackKind::ExplicitReject {
                candidate_id: top.id,
            },
        };
        self.learning
            .model_mut()
            .apply_feedback(&key, &feedback, allow_learning);
        self.last_slice = None;
        let _ = slice;
    }

    pub fn undo_last(&mut self, at_ms: i64) -> Option<UndoVisual> {
        self.undo_last_with_learning(at_ms, true)
    }

    pub fn undo_last_with_learning(
        &mut self,
        at_ms: i64,
        allow_learning: bool,
    ) -> Option<UndoVisual> {
        let revision = self.last_auto_revision?;
        if !self.auto_token_is_last() {
            return None;
        }
        let seq = self.next_seq;
        let outcome = self.learning.undo(revision, seq, at_ms, allow_learning)?;
        let _ = self.take_seq();
        if self.capturing && allow_learning {
            self.record_capture(CaptureRecord::UndoLast { seq, at_ms });
            self.record_capture(CaptureRecord::InterventionReverted {
                seq,
                at_ms,
                edit_id: outcome.inverse.edit_id,
                revert_kind: "explicit_undo".into(),
            });
        }
        let show_nfc = outcome.inverse.replacement.clone();
        self.document
            .replace_last_token(outcome.inverse.replacement);
        self.clear_auto_anchor();
        self.sync_left_context();
        self.last_slice = None;
        Some(UndoVisual {
            show_nfc,
            restored_composition: false,
        })
    }

    #[must_use]
    pub fn clone_model(&self) -> AdaptiveModel {
        self.model().clone()
    }

    #[must_use]
    pub fn save_snapshot(&self) -> SessionSaveSnapshot {
        SessionSaveSnapshot {
            model: self.model().clone(),
            capture_records: self.capture.clone(),
            cursors: self.cursors(),
            last_at_ms: self.last_at_ms,
        }
    }

    #[must_use]
    pub fn document_text(&self) -> String {
        self.document.rendered()
    }

    /// Drop per-document context when the foreground surface changes.
    pub fn clear_document_context(&mut self) {
        self.document = DocumentBuffer::new();
        self.left_context = LeftContext::default();
        self.pending_reopen = false;
        self.invalidate_caret();
        self.last_slice = None;
        self.last_original_nfc.clear();
        self.last_left_token = None;
    }

    /// Replace document-derived context with one bounded token observed by the host.
    ///
    /// Rebasing is a caret boundary: it invalidates edit/learning anchors but does
    /// not emit an input event, capture record, or persistence mutation.
    #[allow(clippy::needless_pass_by_value)] // Public host seam owns the snapshot token.
    pub fn rebase_left_context(&mut self, token_nfc: Option<String>) {
        let external_token = token_nfc
            .as_deref()
            .and_then(|text| text.unicode_words().next_back())
            .map(|token| token.nfc().collect::<String>())
            .filter(|token| token.len() <= MAX_EXTERNAL_LEFT_TOKEN_BYTES);
        self.clear_document_context();
        self.left_context.prev_token_nfc = external_token;
    }

    /// Swap Telex/VNI/tone settings after the host has issued a caret break.
    pub fn set_engine_config(&mut self, config: EngineConfig) {
        self.engine.set_config(config);
        self.pending_reopen = false;
        self.revert_guard = None;
        self.pending_confirmed_reject = None;
        self.last_slice = None;
        self.last_original_nfc.clear();
        self.last_left_token = None;
        self.rewind.invalidate();
    }

    #[must_use]
    pub fn engine_config(&self) -> EngineConfig {
        *self.engine.config()
    }

    /// Drain one local UI notice after a real learning mutation.
    pub fn take_learning_notice(&mut self) -> Option<LearningNotice> {
        self.pending_learning_notice.take()
    }

    #[must_use]
    pub fn model(&self) -> &AdaptiveModel {
        self.learning.model()
    }

    pub fn model_mut(&mut self) -> &mut AdaptiveModel {
        self.learning.model_mut()
    }

    pub fn model_payload(&self) -> Result<Vec<u8>, ModelError> {
        self.learning.model().to_json_payload()
    }

    pub fn set_capturing(&mut self, capturing: bool) {
        self.capturing = capturing;
    }

    pub fn drain_capture(&mut self) -> Vec<CaptureRecord> {
        std::mem::take(&mut self.capture)
    }

    pub fn restore_capture(&mut self, records: Vec<CaptureRecord>) {
        self.capture = records;
        trim_capture_to(&mut self.capture, MAX_CAPTURE_RECORDS);
    }

    fn record_capture(&mut self, record: CaptureRecord) {
        self.capture.push(record);
        trim_capture_to(&mut self.capture, MAX_CAPTURE_RECORDS);
    }

    fn record_candidate_set(
        &mut self,
        snapshot: &CompositionSnapshot,
        event: &InputEvent,
        method: InputMethod,
        slice: &CorrectionSlice,
    ) {
        if !self.capturing || !event.context.allow_learning || slice.candidates.is_empty() {
            return;
        }
        self.record_capture(CaptureRecord::CandidateSetEvaluated {
            seq: event.seq,
            at_ms: event.at_ms,
            ids: slice
                .candidates
                .iter()
                .map(|candidate| candidate.id)
                .collect(),
            sources: slice
                .candidates
                .iter()
                .map(|candidate| candidate.source)
                .collect(),
            identity_hashes: slice
                .candidates
                .iter()
                .map(|candidate| {
                    correction_identity_hash(&correction_identity(snapshot, candidate, method))
                })
                .collect(),
            config_hash: self.learning_config_hash.clone(),
        });
    }

    pub fn restore_last_at_ms(&mut self, last_at_ms: i64) {
        self.last_at_ms = last_at_ms;
    }

    #[must_use]
    pub fn last_at_ms(&self) -> i64 {
        self.last_at_ms
    }

    #[must_use]
    pub fn capture_log(&self) -> CaptureLog {
        CaptureLog {
            header: CaptureHeader {
                v: CAPTURE_VERSION,
                next_seq: self.next_seq,
                next_edit_id: self.next_edit_id,
                last_at_ms: self.last_at_ms,
                model_sha256: self
                    .model_payload()
                    .ok()
                    .map(|bytes| sha256_hex(&bytes))
                    .unwrap_or_default(),
            },
            records: self.capture.clone(),
        }
    }

    #[must_use]
    pub fn cursors(&self) -> SessionCursors {
        SessionCursors {
            next_seq: self.next_seq,
            next_edit_id: self.next_edit_id,
        }
    }

    #[must_use]
    pub fn last_decision(&self) -> Option<DecisionState> {
        self.last_slice.as_ref().and_then(|slice| slice.decision)
    }

    #[must_use]
    pub fn composition_text(&self) -> String {
        self.engine.snapshot().rendered
    }

    #[must_use]
    pub fn top_suggestion(&self) -> Option<String> {
        self.last_slice
            .as_ref()
            .and_then(|slice| slice.display_candidates().first())
            .map(|candidate| candidate.text.clone())
    }

    /// Whether deleting the latest space left an exact committed token eligible for reopening.
    #[must_use]
    pub fn has_pending_reopen(&self) -> bool {
        self.pending_reopen
    }

    /// Cancel a pending reopen when the host can no longer prove the visible token identity.
    pub fn cancel_pending_reopen(&mut self) {
        self.pending_reopen = false;
    }

    #[must_use]
    pub fn candidate_texts(&self) -> Vec<String> {
        self.last_slice
            .as_ref()
            .map(|slice| {
                slice
                    .display_candidates()
                    .iter()
                    .map(|candidate| candidate.text.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn pop_document_on_empty_backspace(
        &mut self,
        kind: &InputKind,
        composition_was_empty: bool,
        allow_transform: bool,
        allow_learning: bool,
    ) {
        if !matches!(kind, InputKind::Backspace) || !composition_was_empty || !allow_transform {
            return;
        }
        let Some(outcome) = self.document.pop_grapheme() else {
            return;
        };
        self.clear_auto_anchor();
        if let Some(unit) = outcome.started_deleting
            && allow_learning
        {
            self.miner.record_deleted_token(unit.full_token_nfc.clone());
            self.mining_snapshot = Some(unit);
        }
        self.sync_left_context();
        self.pending_reopen =
            outcome.removed_delimiter == Some(' ') && self.reopen_engine_for_last_token().is_some();
    }

    fn reopen_engine_for_last_token(&self) -> Option<Engine> {
        if !self.engine.is_empty() {
            return None;
        }
        let unit = self.document.last()?;
        if unit.delimiter.is_some()
            || unit.remaining_nfc != unit.full_token_nfc
            || unit.input_method != self.engine.config().method
        {
            return None;
        }
        let raw_keys = unit.raw_keys.as_deref()?;
        if raw_keys.is_empty() {
            return None;
        }
        let mut reopened = self.engine.clone();
        reopened.restore_raw_keys(raw_keys);
        (reopened.rendered() == unit.full_token_nfc).then_some(reopened)
    }

    fn reopen_previous_token(&mut self) -> bool {
        self.pending_reopen = false;
        let Some(reopened) = self.reopen_engine_for_last_token() else {
            return false;
        };
        let _ = self.document.pop_last();
        self.engine = reopened;
        self.last_slice = None;
        self.clear_auto_anchor();
        self.sync_left_context();
        true
    }

    fn commits(actions: &[EngineAction], allow_transform: bool) -> Vec<(String, Option<char>)> {
        if !allow_transform {
            return Vec::new();
        }
        actions
            .iter()
            .filter_map(|action| match action {
                EngineAction::Commit {
                    text, delimiter, ..
                } => Some((text.clone(), *delimiter)),
                _ => None,
            })
            .collect()
    }

    fn correction_slice(
        &mut self,
        snapshot: &CompositionSnapshot,
        event: &InputEvent,
        at_commit: bool,
        delimiter: Option<char>,
    ) -> CorrectionSlice {
        let method = self.engine.config().method;
        let telex_fix = TelexFixGenerator::new(method, self.engine.config().tone_placement);
        let fuzzy = FuzzyGenerator::new(&self.lexicon, 5);
        let diacritics = DiacriticsGenerator::new(&self.lexicon, 5);
        let personal =
            PersonalGenerator::for_method(method, &self.learning.model().personal_promoted());
        let generators: [&dyn Generator; 5] =
            [&self.abbrev, &telex_fix, &fuzzy, &diacritics, &personal];
        let auto_edit = (at_commit && !snapshot.is_empty()).then_some(AutoEditContext {
            edit_id: self.next_edit_id,
            range: EditRange {
                basis: RangeBasis::ActiveComposition,
                start_grapheme: 0,
                length_grapheme: snapshot.rendered.graphemes(true).count(),
                revision: snapshot.revision,
            },
            delimiter,
        });
        let guard = self.revert_guard.clone();
        let slice = run_learning_correction_slice_with_guard(
            snapshot,
            &self.left_context,
            event.context,
            &generators,
            method,
            &mut self.learning,
            event.at_ms,
            &self.score_config,
            &self.decision_config,
            auto_edit,
            &self.lexicon,
            self.intervention,
            guard.as_ref(),
        );
        self.record_candidate_set(snapshot, event, method, &slice);
        if event.context.allow_learning {
            for candidate in slice.display_candidates() {
                let key = candidate_rule_key(snapshot, &self.left_context, method, candidate);
                self.learning
                    .model_mut()
                    .record_impression(&key, event.seq, event.at_ms, true);
            }
        }
        if at_commit && let Some(EngineAction::ReplaceRange(action)) = &slice.action {
            let reason = slice.plan.as_ref().map(|plan| plan.reason);
            let structural = reason == Some(InterventionReason::SafeStructuralFix);
            let chosen = slice
                .plan
                .as_ref()
                .and_then(|plan| plan.candidate_id)
                .and_then(|id| {
                    slice
                        .candidates
                        .iter()
                        .find(|candidate| candidate.id == id)
                        .cloned()
                });
            if let Some(fix) = chosen {
                let rule = candidate_rule_key(snapshot, &self.left_context, method, &fix);
                if self.capturing && event.context.allow_learning {
                    self.record_capture(CaptureRecord::InterventionApplied {
                        seq: event.seq,
                        at_ms: event.at_ms,
                        edit_id: action.edit_id,
                        candidate_id: fix.id,
                        reason: format!("{:?}", reason.unwrap_or(InterventionReason::LowScore)),
                    });
                }
                self.pending_policy_undo = Some(PendingPolicyUndo {
                    raw_token: snapshot.raw_keys.clone(),
                    identity: correction_identity(snapshot, &fix, method),
                    rule_key: rule.clone(),
                    edit_id: action.edit_id,
                    applied_at_ms: event.at_ms,
                    learn_undo: !structural,
                });
                if !structural {
                    self.pending_learning_notice = Some(LearningNotice {
                        kind: LearningNoticeKind::Corrected,
                        original_nfc: snapshot.normalized.clone(),
                        replacement_nfc: action.replacement.clone(),
                        positive_delta: 0.0,
                        negative_delta: 0.0,
                        positive_total: 0.0,
                        negative_total: 0.0,
                    });
                }
                if event.context.allow_learning {
                    self.last_learned = Some(LastLearned::Rule(rule));
                }
            }
        }
        slice
    }

    fn commit_token(
        &mut self,
        text: &str,
        delimiter: Option<char>,
        raw_keys: &str,
        slice: &CorrectionSlice,
        event: &InputEvent,
        allow_learning: bool,
    ) {
        let method = self.engine.config().method;
        let token_text = if let Some(EngineAction::ReplaceRange(action)) = &slice.action {
            self.last_auto_revision = Some(action.range.revision);
            self.last_auto_token = Some(action.replacement.clone());
            self.next_edit_id = self.next_edit_id.saturating_add(1);
            action.replacement.clone()
        } else {
            self.clear_auto_anchor();
            text.to_string()
        };
        let leftover = self.leftover_committed_prefix();
        let left_at_commit = self.left_context.prev_token_nfc.clone();
        self.document.push_commit(CommittedUnit::new(
            token_text.clone(),
            delimiter,
            self.last_original_nfc.clone(),
            Some(raw_keys.to_string()),
            left_at_commit,
            method,
            slice.display_candidates().to_vec(),
        ));
        if delimiter.is_some()
            && self
                .revert_guard
                .as_ref()
                .is_some_and(|guard| guard.bypass_next_boundary && guard.raw_token == raw_keys)
        {
            self.confirm_reverted_original(raw_keys, event.at_ms, allow_learning);
            self.revert_guard = None;
        }
        self.finish_implicit(
            &token_text,
            event.seq,
            event.at_ms,
            allow_learning,
            leftover,
        );
        self.sync_left_context();
    }

    fn push_plain_commit(
        &mut self,
        text: &str,
        delimiter: Option<char>,
        method: InputMethod,
        event: &InputEvent,
        allow_learning: bool,
    ) {
        let leftover = self.leftover_committed_prefix();
        let left_at_commit = self.left_context.prev_token_nfc.clone();
        self.document.push_commit(CommittedUnit::new(
            text.to_string(),
            delimiter,
            text.to_string(),
            None,
            left_at_commit,
            method,
            Vec::new(),
        ));
        if !text.is_empty() {
            self.finish_implicit(text, event.seq, event.at_ms, allow_learning, leftover);
        }
        self.sync_left_context();
    }

    fn auto_token_is_last(&self) -> bool {
        match (&self.last_auto_token, self.document.last()) {
            (Some(expected), Some(unit)) => {
                unit.full_token_nfc == *expected && unit.remaining_nfc == *expected
            }
            _ => false,
        }
    }

    fn clear_auto_anchor(&mut self) {
        self.last_auto_revision = None;
        self.last_auto_token = None;
        self.pending_policy_undo = None;
    }

    fn confirm_reverted_original(&mut self, raw_token: &str, at_ms: i64, allow_learning: bool) {
        let Some(pending) = self.pending_confirmed_reject.take() else {
            return;
        };
        if pending.raw_token != raw_token || !allow_learning {
            return;
        }
        let feedback = FeedbackEvent {
            seq: self.take_seq(),
            at_ms,
            // The existing Undo signal carries the calibrated strong veto weight.
            // Text was already rolled back, so this call confirms learning only.
            kind: FeedbackKind::Undo {
                edit_id: pending.edit_id,
            },
        };
        self.learning
            .model_mut()
            .apply_feedback(&pending.rule_key, &feedback, true);
        if self.capturing {
            self.record_capture(CaptureRecord::CorrectionConfirmed {
                seq: feedback.seq,
                at_ms,
                identity: rule_identity(&pending.rule_key),
                left_token_nfc: pending.rule_key.left_token_nfc,
            });
        }
    }

    fn leftover_committed_prefix(&self) -> bool {
        self.document.last().is_some_and(|unit| {
            !unit.remaining_nfc.is_empty() && unit.remaining_nfc != unit.full_token_nfc
        })
    }

    fn finish_implicit(
        &mut self,
        replacement: &str,
        seq: u64,
        at_ms: i64,
        allow_learning: bool,
        leftover: bool,
    ) {
        if leftover {
            self.miner.invalidate_due_to_caret_break();
            self.mining_snapshot = None;
            self.rewind.invalidate();
            return;
        }
        self.finish_committed_implicit(replacement, seq, at_ms, allow_learning);
        self.finish_rewind_implicit(replacement, seq, at_ms, allow_learning);
    }

    fn finish_committed_implicit(
        &mut self,
        replacement: &str,
        seq: u64,
        at_ms: i64,
        allow_learning: bool,
    ) {
        let Some(feedback) = self.miner.finish_replacement(replacement, seq, at_ms) else {
            return;
        };
        let Some(snapshot) = self.mining_snapshot.take() else {
            return;
        };
        if !allow_learning {
            return;
        }
        let Some(candidate) = snapshot
            .candidates
            .iter()
            .find(|candidate| candidate.text == replacement)
        else {
            return;
        };
        let key = RuleContextKey {
            input_method: snapshot.input_method,
            source: candidate.source,
            original_nfc: snapshot.original_nfc.clone(),
            candidate_nfc: candidate.text.clone(),
            left_token_nfc: snapshot.left_token_nfc.clone(),
            source_rule_id: candidate
                .evidence
                .split('+')
                .next()
                .unwrap_or("")
                .to_string(),
        };
        let (positive_delta, negative_delta) = self
            .learning
            .model_mut()
            .apply_feedback(&key, &feedback, true);
        let (positive_total, negative_total) = self.learning.model().evidence_totals(&key);
        if self.capturing {
            self.record_capture(CaptureRecord::CorrectionConfirmed {
                seq,
                at_ms,
                identity: rule_identity(&key),
                left_token_nfc: key.left_token_nfc.clone(),
            });
        }
        self.pending_learning_notice = Some(LearningNotice {
            kind: LearningNoticeKind::Observed,
            original_nfc: key.original_nfc.clone(),
            replacement_nfc: key.candidate_nfc.clone(),
            positive_delta,
            negative_delta,
            positive_total,
            negative_total,
        });
        self.last_learned = Some(LastLearned::Rule(key));
    }

    fn finish_rewind_implicit(
        &mut self,
        replacement: &str,
        seq: u64,
        at_ms: i64,
        allow_learning: bool,
    ) {
        match self.rewind.evaluate(replacement, seq, at_ms) {
            RewindEvaluate::Ignored => {}
            RewindEvaluate::Matched { key, feedback } => {
                if allow_learning {
                    let (positive_delta, negative_delta) = self
                        .learning
                        .model_mut()
                        .apply_feedback(&key, &feedback, true);
                    let (positive_total, negative_total) =
                        self.learning.model().evidence_totals(&key);
                    if self.capturing {
                        self.record_capture(CaptureRecord::CorrectionConfirmed {
                            seq,
                            at_ms,
                            identity: rule_identity(&key),
                            left_token_nfc: key.left_token_nfc.clone(),
                        });
                    }
                    self.pending_learning_notice = Some(LearningNotice {
                        kind: LearningNoticeKind::Observed,
                        original_nfc: key.original_nfc.clone(),
                        replacement_nfc: key.candidate_nfc.clone(),
                        positive_delta,
                        negative_delta,
                        positive_total,
                        negative_total,
                    });
                    self.last_learned = Some(LastLearned::Rule(key));
                }
            }
            RewindEvaluate::Unmatched {
                original_nfc,
                replacement_nfc,
                input_method,
            } => {
                if allow_learning {
                    let previous_count = self.learning.model().personal_correction_count(
                        input_method,
                        &original_nfc,
                        &replacement_nfc,
                    );
                    let promoted = self.learning.model_mut().record_personal_correction(
                        input_method,
                        original_nfc.clone(),
                        replacement_nfc.clone(),
                        PersonalTransaction { anchor: seq, at_ms },
                        true,
                    );
                    let count = self.learning.model().personal_correction_count(
                        input_method,
                        &original_nfc,
                        &replacement_nfc,
                    );
                    if count > previous_count {
                        if self.capturing {
                            self.record_capture(CaptureRecord::CorrectionConfirmed {
                                seq,
                                at_ms,
                                identity: CorrectionIdentity {
                                    input_method,
                                    source: CandidateSource::Personal,
                                    original_nfc: original_nfc.clone(),
                                    candidate_nfc: replacement_nfc.clone(),
                                    source_rule_id: "personal-correction".into(),
                                },
                                left_token_nfc: None,
                            });
                        }
                        self.pending_learning_notice = Some(LearningNotice {
                            kind: if promoted {
                                LearningNoticeKind::Promoted
                            } else {
                                LearningNoticeKind::Observed
                            },
                            original_nfc: original_nfc.clone(),
                            replacement_nfc: replacement_nfc.clone(),
                            positive_delta: f64::from(count - previous_count),
                            negative_delta: 0.0,
                            positive_total: f64::from(count),
                            negative_total: 0.0,
                        });
                        self.last_learned = Some(LastLearned::Personal {
                            input_method,
                            original_nfc,
                            replacement_nfc,
                        });
                    }
                }
            }
        }
    }

    fn try_restore_policy_undo(
        &mut self,
        event_seq: u64,
        at_ms: i64,
        allow_learning: bool,
    ) -> bool {
        let Some(pending) = self.pending_policy_undo.clone() else {
            return false;
        };
        let config = LearningConfigV2::compatibility_v1();
        if at_ms < pending.applied_at_ms
            || at_ms.saturating_sub(pending.applied_at_ms) > config.immediate_revert_window_ms
        {
            return false;
        }
        if !self.auto_token_is_last() {
            return false;
        }
        let Some(revision) = self.last_auto_revision else {
            return false;
        };
        let learn_undo = allow_learning && pending.learn_undo;
        if self
            .learning
            .revert(revision, self.next_seq, learn_undo)
            .is_none()
        {
            return false;
        }
        let _ = self.take_seq();
        if self.capturing && allow_learning {
            self.record_capture(CaptureRecord::InterventionReverted {
                seq: event_seq,
                at_ms,
                edit_id: pending.edit_id,
                revert_kind: "immediate_backspace".into(),
            });
        }
        self.document.pop_last();
        self.engine.restore_raw_keys(&pending.raw_token);
        self.clear_auto_anchor();
        self.pending_confirmed_reject = learn_undo.then_some(PendingConfirmedReject {
            raw_token: pending.raw_token.clone(),
            rule_key: pending.rule_key,
            edit_id: pending.edit_id,
        });
        let restored = self.engine.snapshot();
        self.revert_guard = Some(RevertGuard {
            identity: pending.identity,
            raw_token: pending.raw_token,
            focus_generation: self.focus_generation,
            composition_revision: restored.revision,
            reapply_cooldown_until_ms: at_ms.saturating_add(config.reapply_cooldown_ms),
            bypass_next_boundary: true,
        });
        self.sync_left_context();
        self.last_slice = None;
        true
    }

    pub fn forget_last_rule(&mut self) -> bool {
        let (changed, original_nfc, replacement_nfc, identity) = match self.last_learned.take() {
            Some(LastLearned::Rule(key)) => {
                let identity = rule_identity(&key);
                (
                    self.learning.model_mut().forget_rule(&key),
                    key.original_nfc,
                    key.candidate_nfc,
                    identity,
                )
            }
            Some(LastLearned::Personal {
                input_method,
                original_nfc,
                replacement_nfc,
            }) => {
                let identity = CorrectionIdentity {
                    input_method,
                    source: CandidateSource::Personal,
                    original_nfc: original_nfc.clone(),
                    candidate_nfc: replacement_nfc.clone(),
                    source_rule_id: "personal-correction".into(),
                };
                (
                    self.learning.model_mut().forget_personal_pair(
                        input_method,
                        &original_nfc,
                        &replacement_nfc,
                    ),
                    original_nfc,
                    replacement_nfc,
                    identity,
                )
            }
            None => return false,
        };
        if !changed {
            return false;
        }
        self.checkpoint_after_forget(&identity);
        self.pending_learning_notice = Some(LearningNotice {
            kind: LearningNoticeKind::Forgotten,
            original_nfc,
            replacement_nfc,
            positive_delta: 0.0,
            negative_delta: 0.0,
            positive_total: 0.0,
            negative_total: 0.0,
        });
        true
    }

    pub(crate) fn forget_identity_hash_for_replay(&mut self, identity_hash: &str) {
        let rows = self.learning.model().inspection_rows();
        for row in rows {
            let identity = CorrectionIdentity {
                input_method: row.input_method,
                source: row.source,
                original_nfc: row.original_nfc.clone(),
                candidate_nfc: row.candidate_nfc.clone(),
                source_rule_id: row.source_rule_id.clone(),
            };
            if correction_identity_hash(&identity) == identity_hash {
                self.learning.model_mut().forget_inspection_row(&row);
            }
        }
    }

    /// Forget exactly one learned row selected from the model inspection projection.
    pub fn forget_inspection_row(&mut self, row: &ModelInspectionRow) -> bool {
        let changed = self.learning.model_mut().forget_inspection_row(row);
        if changed {
            if self
                .last_learned
                .as_ref()
                .is_some_and(|last| last_learned_matches_row(last, row))
            {
                self.last_learned = None;
            }
            let identity = CorrectionIdentity {
                input_method: row.input_method,
                source: row.source,
                original_nfc: row.original_nfc.clone(),
                candidate_nfc: row.candidate_nfc.clone(),
                source_rule_id: row.source_rule_id.clone(),
            };
            self.checkpoint_after_forget(&identity);
            self.pending_learning_notice = Some(LearningNotice {
                kind: LearningNoticeKind::Forgotten,
                original_nfc: row.original_nfc.clone(),
                replacement_nfc: row.candidate_nfc.clone(),
                positive_delta: 0.0,
                negative_delta: 0.0,
                positive_total: 0.0,
                negative_total: 0.0,
            });
        }
        changed
    }

    fn checkpoint_after_forget(&mut self, identity: &CorrectionIdentity) {
        compact_capture_after_forget(&mut self.capture, identity);
        if self.capturing {
            let seq = self.take_seq();
            self.record_capture(CaptureRecord::DataForgotten {
                seq,
                at_ms: self.last_at_ms,
                identity: correction_identity_hash(identity),
            });
        }
        self.learning.invalidate_due_to_caret_break();
        self.miner.invalidate_due_to_caret_break();
        self.rewind.invalidate();
        self.mining_snapshot = None;
        self.clear_auto_anchor();
        self.revert_guard = None;
        self.pending_confirmed_reject = None;
    }

    fn invalidate_caret(&mut self) {
        self.pending_reopen = false;
        self.revert_guard = None;
        self.pending_confirmed_reject = None;
        self.focus_generation = self.focus_generation.saturating_add(1);
        self.miner.invalidate_due_to_caret_break();
        self.rewind.invalidate();
        self.learning.invalidate_due_to_caret_break();
        self.mining_snapshot = None;
        self.clear_auto_anchor();
    }

    fn sync_left_context(&mut self) {
        self.left_context.prev_token_nfc = self.document.context_token();
    }

    fn top_rule_key(&self, top: &Candidate) -> RuleContextKey {
        candidate_rule_key(
            &CompositionSnapshot::new(
                0,
                self.last_original_nfc.clone(),
                self.last_original_nfc.clone(),
            ),
            &LeftContext {
                prev_token_nfc: self.last_left_token.clone(),
            },
            self.last_method,
            top,
        )
    }

    fn reset_engine(&mut self, at_ms: i64) {
        self.engine.process(&InputEvent {
            seq: self.next_seq,
            at_ms,
            kind: InputKind::Reset,
            modifiers: Modifiers::empty(),
            is_repeat: false,
            context: InputContext {
                allow_transform: true,
                allow_learning: true,
            },
        });
    }

    fn take_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        seq
    }

    /// Cheap pre-inject checkpoint (no `AbbrevGenerator` / lexicon).
    ///
    /// Always clones [`LearningSession`]: letter keys still run `record_decision` /
    /// pending auto-settlement observation, so `InjectError` must restore model state.
    #[must_use]
    pub fn checkpoint_for_inject(&self, _kind: &InputKind) -> SessionInjectCheckpoint {
        SessionInjectCheckpoint {
            engine: self.engine.clone(),
            document: self.document.clone(),
            left_context: self.left_context.clone(),
            next_seq: self.next_seq,
            next_edit_id: self.next_edit_id,
            miner: self.miner.clone(),
            mining_snapshot: self.mining_snapshot.clone(),
            last_slice: self.last_slice.clone(),
            last_original_nfc: self.last_original_nfc.clone(),
            last_left_token: self.last_left_token.clone(),
            last_method: self.last_method,
            last_auto_revision: self.last_auto_revision,
            last_auto_token: self.last_auto_token.clone(),
            last_at_ms: self.last_at_ms,
            capture: self.capture.clone(),
            learning: self.learning.clone(),
            rewind: self.rewind.clone(),
            intervention: self.intervention,
            pending_policy_undo: self.pending_policy_undo.clone(),
            pending_confirmed_reject: self.pending_confirmed_reject.clone(),
            revert_guard: self.revert_guard.clone(),
            focus_generation: self.focus_generation,
            pending_reopen: self.pending_reopen,
            last_learned: self.last_learned.clone(),
            pending_learning_notice: self.pending_learning_notice.clone(),
        }
    }

    /// Restore persistent learning/capture state while preserving non-learning composition.
    ///
    /// `next_edit_id` is left alone: an on-screen Auto still records semantic undo even
    /// when learning is disabled, so rolling the cursor back would reuse edit ids.
    pub fn restore_persistent_state(&mut self, checkpoint: &SessionInjectCheckpoint) {
        self.next_seq = checkpoint.next_seq;
        self.last_at_ms = checkpoint.last_at_ms;
        self.capture.clone_from(&checkpoint.capture);
        *self.learning.model_mut() = checkpoint.learning.model().clone();
        self.last_learned.clone_from(&checkpoint.last_learned);
        self.pending_learning_notice
            .clone_from(&checkpoint.pending_learning_notice);
    }

    /// Restore state captured by [`Self::checkpoint_for_inject`].
    pub fn restore_inject_checkpoint(&mut self, checkpoint: SessionInjectCheckpoint) {
        self.engine = checkpoint.engine;
        self.document = checkpoint.document;
        self.left_context = checkpoint.left_context;
        self.next_seq = checkpoint.next_seq;
        self.next_edit_id = checkpoint.next_edit_id;
        self.miner = checkpoint.miner;
        self.mining_snapshot = checkpoint.mining_snapshot;
        self.last_slice = checkpoint.last_slice;
        self.last_original_nfc = checkpoint.last_original_nfc;
        self.last_left_token = checkpoint.last_left_token;
        self.last_method = checkpoint.last_method;
        self.last_auto_revision = checkpoint.last_auto_revision;
        self.last_auto_token = checkpoint.last_auto_token;
        self.last_at_ms = checkpoint.last_at_ms;
        self.capture = checkpoint.capture;
        self.learning = checkpoint.learning;
        self.rewind = checkpoint.rewind;
        self.intervention = checkpoint.intervention;
        self.pending_policy_undo = checkpoint.pending_policy_undo;
        self.pending_confirmed_reject = checkpoint.pending_confirmed_reject;
        self.revert_guard = checkpoint.revert_guard;
        self.focus_generation = checkpoint.focus_generation;
        self.pending_reopen = checkpoint.pending_reopen;
        self.last_learned = checkpoint.last_learned;
        self.pending_learning_notice = checkpoint.pending_learning_notice;
    }
}

/// Pre-inject snapshot for rolling back when OS SendInput fails.
pub struct SessionInjectCheckpoint {
    engine: Engine,
    document: DocumentBuffer,
    left_context: LeftContext,
    next_seq: u64,
    next_edit_id: u64,
    miner: ImplicitCorrectionMiner,
    mining_snapshot: Option<CommittedUnit>,
    last_slice: Option<CorrectionSlice>,
    last_original_nfc: String,
    last_left_token: Option<String>,
    last_method: InputMethod,
    last_auto_revision: Option<u64>,
    last_auto_token: Option<String>,
    last_at_ms: i64,
    capture: Vec<CaptureRecord>,
    learning: LearningSession,
    rewind: CompositionRewindMiner,
    intervention: InterventionConfig,
    pending_policy_undo: Option<PendingPolicyUndo>,
    pending_confirmed_reject: Option<PendingConfirmedReject>,
    revert_guard: Option<RevertGuard>,
    focus_generation: u64,
    pending_reopen: bool,
    last_learned: Option<LastLearned>,
    pending_learning_notice: Option<LearningNotice>,
}

fn rule_identity(key: &RuleContextKey) -> CorrectionIdentity {
    CorrectionIdentity {
        input_method: key.input_method,
        source: key.source,
        original_nfc: key.original_nfc.clone(),
        candidate_nfc: key.candidate_nfc.clone(),
        source_rule_id: key.source_rule_id.clone(),
    }
}

/// Unicode punctuation the engine's ASCII boundary table does not treat as commit.
#[must_use]
pub fn extra_boundary_char(ch: char) -> bool {
    matches!(
        ch,
        '…' | '–' | '—' | '\u{201C}' | '\u{201D}' | '\u{2018}' | '\u{2019}' | '«' | '»'
    )
}

#[must_use]
pub fn map_extra_boundary_kind(kind: InputKind) -> InputKind {
    match kind {
        InputKind::Key { logical, .. } if extra_boundary_char(logical) => {
            InputKind::Boundary { delimiter: logical }
        }
        other => other,
    }
}
