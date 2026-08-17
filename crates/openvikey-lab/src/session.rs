//! Observable deterministic Engine → correction session used by the lab CLI.

use crate::capture::{CAPTURE_VERSION, CaptureHeader, CaptureLog, CaptureRecord, sha256_hex};
use crate::document::{CommittedUnit, DocumentBuffer};
use openvikey_core::correction::{AutoEditContext, CorrectionSlice, run_learning_correction_slice};
use openvikey_core::decision::{DecisionConfig, DecisionState};
use openvikey_core::engine::{Engine, EngineConfig};
use openvikey_core::feedback::{ImplicitCorrectionMiner, LearningSession};
use openvikey_core::generate::abbrev::AbbrevGenerator;
use openvikey_core::generate::diacritics::DiacriticsGenerator;
use openvikey_core::generate::fuzzy::FuzzyGenerator;
use openvikey_core::generate::telex_fix::TelexFixGenerator;
use openvikey_core::generate::{Generator, LeftContext};
use openvikey_core::lexicon::Lexicon;
use openvikey_core::model::{AdaptiveModel, ModelError, RuleContextKey};
use openvikey_core::rank::ScoreConfig;
use openvikey_core::types::{
    Candidate, CompositionSnapshot, EditRange, EngineAction, FeedbackEvent, FeedbackKind,
    InputContext, InputEvent, InputKind, InputMethod, Modifiers, RangeBasis,
};
use serde::Serialize;
use unicode_segmentation::UnicodeSegmentation;

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
        }
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

    pub fn process_event(&mut self, event: &InputEvent) -> SessionObservation {
        let mut event = event.clone();
        event.kind = map_extra_boundary_kind(event.kind);
        self.next_seq = self.next_seq.max(event.seq.saturating_add(1));
        self.last_at_ms = self.last_at_ms.max(event.at_ms);
        let allow_transform = event.context.allow_transform;
        let allow_learning = event.context.allow_learning;
        if self.capturing && allow_transform && allow_learning {
            self.capture.push(CaptureRecord::Input {
                event: event.clone(),
            });
        }
        if matches!(
            event.kind,
            InputKind::CursorMoved | InputKind::SelectionChanged | InputKind::Reset
        ) {
            self.invalidate_caret();
        }

        let before = self.engine.snapshot();
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
        let observation = SessionObservation {
            event_seq: event.seq,
            snapshot,
            engine_actions,
            candidates: slice.candidates.clone(),
            decision: slice.decision,
            action: slice.action.clone(),
        };
        let recorded_auto = matches!(&slice.action, Some(EngineAction::ReplaceRange(_)));
        let mut used_slice = false;
        for (text, delimiter) in commits {
            if !used_slice && !text.is_empty() {
                self.commit_token(&text, delimiter, &slice, &event, allow_learning, method);
                used_slice = true;
            } else {
                self.push_plain_commit(&text, delimiter, method, &event, allow_learning);
                self.clear_auto_anchor();
            }
        }
        if allow_learning && !recorded_auto {
            let settled = self
                .learning
                .observe_input_or_edit(self.next_seq, event.at_ms, true);
            self.next_seq = self
                .next_seq
                .saturating_add(u64::try_from(settled.len()).unwrap_or(0));
        }
        observation
    }

    pub fn accept_top(&mut self, at_ms: i64) {
        let Some(slice) = self.last_slice.clone() else {
            return;
        };
        let Some(top) = slice.candidates.first().cloned() else {
            return;
        };
        let seq = self.take_seq();
        if self.capturing {
            self.capture.push(CaptureRecord::AcceptTop { seq, at_ms });
        }
        let key = self.top_rule_key(&top);
        let feedback = FeedbackEvent {
            seq,
            at_ms,
            kind: FeedbackKind::Accept {
                candidate_id: top.id,
            },
        };
        self.learning
            .model_mut()
            .apply_feedback(&key, &feedback, true);
        self.clear_auto_anchor();
        if self.engine.snapshot().is_empty() {
            self.document.replace_last_token(top.text);
        } else {
            self.reset_engine(at_ms);
            self.document.push_commit(CommittedUnit::new(
                top.text,
                Some(' '),
                self.last_original_nfc.clone(),
                self.last_left_token.clone(),
                self.last_method,
                slice.candidates,
            ));
        }
        self.sync_left_context();
    }

    pub fn reject_top(&mut self, at_ms: i64) {
        let Some(slice) = self.last_slice.clone() else {
            return;
        };
        let Some(top) = slice.candidates.first().cloned() else {
            return;
        };
        let seq = self.take_seq();
        if self.capturing {
            self.capture.push(CaptureRecord::RejectTop { seq, at_ms });
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
            .apply_feedback(&key, &feedback, true);
        let _ = slice;
    }

    pub fn undo_last(&mut self, at_ms: i64) {
        let Some(revision) = self.last_auto_revision else {
            return;
        };
        if !self.auto_token_is_last() {
            return;
        }
        let seq = self.next_seq;
        let Some(outcome) = self.learning.undo(revision, seq, at_ms, true) else {
            return;
        };
        let _ = self.take_seq();
        if self.capturing {
            self.capture.push(CaptureRecord::UndoLast { seq, at_ms });
        }
        self.document
            .replace_last_token(outcome.inverse.replacement);
        self.clear_auto_anchor();
        self.sync_left_context();
    }

    #[must_use]
    pub fn document_text(&self) -> String {
        self.document.rendered()
    }

    #[must_use]
    pub fn model(&self) -> &AdaptiveModel {
        self.learning.model()
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
            .and_then(|slice| slice.candidates.first())
            .map(|candidate| candidate.text.clone())
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
        let generators: [&dyn Generator; 4] = [&self.abbrev, &telex_fix, &fuzzy, &diacritics];
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
        run_learning_correction_slice(
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
        )
    }

    fn commit_token(
        &mut self,
        text: &str,
        delimiter: Option<char>,
        slice: &CorrectionSlice,
        event: &InputEvent,
        allow_learning: bool,
        method: InputMethod,
    ) {
        let token_text = if let Some(EngineAction::ReplaceRange(action)) = &slice.action {
            self.last_auto_revision = Some(action.range.revision);
            self.last_auto_token = Some(action.replacement.clone());
            self.next_edit_id = self.next_edit_id.saturating_add(1);
            action.replacement.clone()
        } else {
            self.clear_auto_anchor();
            text.to_string()
        };
        let left_at_commit = self.left_context.prev_token_nfc.clone();
        self.document.push_commit(CommittedUnit::new(
            token_text,
            delimiter,
            self.last_original_nfc.clone(),
            left_at_commit,
            method,
            slice.candidates.clone(),
        ));
        self.finish_implicit(text, event.seq, event.at_ms, allow_learning);
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
        let left_at_commit = self.left_context.prev_token_nfc.clone();
        self.document.push_commit(CommittedUnit::new(
            text.to_string(),
            delimiter,
            text.to_string(),
            left_at_commit,
            method,
            Vec::new(),
        ));
        if !text.is_empty() {
            self.finish_implicit(text, event.seq, event.at_ms, allow_learning);
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
    }

    fn finish_implicit(&mut self, replacement: &str, seq: u64, at_ms: i64, allow_learning: bool) {
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
        self.learning
            .model_mut()
            .apply_feedback(&key, &feedback, true);
    }

    fn invalidate_caret(&mut self) {
        self.miner.invalidate_due_to_caret_break();
        self.learning.invalidate_due_to_caret_break();
        self.mining_snapshot = None;
        self.clear_auto_anchor();
    }

    fn sync_left_context(&mut self) {
        self.left_context.prev_token_nfc = self.document.context_token();
    }

    fn top_rule_key(&self, top: &Candidate) -> RuleContextKey {
        RuleContextKey {
            input_method: self.last_method,
            source: top.source,
            original_nfc: self.last_original_nfc.clone(),
            candidate_nfc: top.text.clone(),
            left_token_nfc: self.last_left_token.clone(),
            source_rule_id: top.evidence.split('+').next().unwrap_or("").to_string(),
        }
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
