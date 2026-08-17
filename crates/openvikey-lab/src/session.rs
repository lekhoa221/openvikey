//! Observable deterministic Engine → correction session used by the lab CLI.

use openvikey_core::correction::run_learning_correction_slice;
use openvikey_core::decision::{DecisionConfig, DecisionState};
use openvikey_core::engine::{Engine, EngineConfig};
use openvikey_core::feedback::LearningSession;
use openvikey_core::generate::abbrev::AbbrevGenerator;
use openvikey_core::generate::diacritics::DiacriticsGenerator;
use openvikey_core::generate::fuzzy::FuzzyGenerator;
use openvikey_core::generate::telex_fix::TelexFixGenerator;
use openvikey_core::generate::{Generator, LeftContext};
use openvikey_core::lexicon::Lexicon;
use openvikey_core::model::{AdaptiveModel, ModelError};
use openvikey_core::rank::ScoreConfig;
use openvikey_core::types::{
    Candidate, CompositionSnapshot, EngineAction, InputContext, InputEvent, InputKind, Modifiers,
};
use serde::Serialize;

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
    score_config: ScoreConfig,
    decision_config: DecisionConfig,
}

impl LabSession {
    #[must_use]
    pub fn new(engine_config: EngineConfig, lexicon: Lexicon) -> Self {
        Self {
            engine: Engine::new(engine_config),
            lexicon,
            abbrev: AbbrevGenerator::from_seed(),
            learning: LearningSession::new(AdaptiveModel::default(), 32),
            left_context: LeftContext::default(),
            next_seq: 1,
            score_config: ScoreConfig::default(),
            decision_config: DecisionConfig::default(),
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
                let seq = self.take_seq();
                self.process_event(&InputEvent {
                    seq,
                    at_ms: first_at_ms.saturating_add(offset),
                    kind: InputKind::Key {
                        logical,
                        physical: None,
                    },
                    modifiers: Modifiers::empty(),
                    is_repeat: false,
                    context,
                })
            })
            .collect()
    }

    pub fn process_event(&mut self, event: &InputEvent) -> SessionObservation {
        self.next_seq = self.next_seq.max(event.seq.saturating_add(1));
        let before = self.engine.snapshot();
        let engine_actions = self.engine.process(event);
        let committed_token = if event.context.allow_transform {
            engine_actions.iter().find_map(|action| match action {
                EngineAction::Commit { text, .. } if !text.is_empty() => Some(text.clone()),
                _ => None,
            })
        } else {
            None
        };
        let snapshot = if committed_token.is_some() && !before.is_empty() {
            before
        } else {
            self.engine.snapshot()
        };
        let method = self.engine.config().method;
        let telex_fix = TelexFixGenerator::new(method, self.engine.config().tone_placement);
        let fuzzy = FuzzyGenerator::new(&self.lexicon, 5);
        let diacritics = DiacriticsGenerator::new(&self.lexicon, 5);
        let generators: [&dyn Generator; 4] = [&self.abbrev, &telex_fix, &fuzzy, &diacritics];
        let slice = run_learning_correction_slice(
            &snapshot,
            &self.left_context,
            event.context,
            &generators,
            method,
            &mut self.learning,
            event.at_ms,
            &self.score_config,
            &self.decision_config,
            None,
        );
        let observation = SessionObservation {
            event_seq: event.seq,
            snapshot,
            engine_actions,
            candidates: slice.candidates,
            decision: slice.decision,
            action: slice.action,
        };
        if let Some(committed) = committed_token {
            self.left_context.prev_token_nfc = Some(committed);
        }
        observation
    }

    pub fn model_payload(&self) -> Result<Vec<u8>, ModelError> {
        self.learning.model().to_json_payload()
    }

    fn take_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        seq
    }
}
