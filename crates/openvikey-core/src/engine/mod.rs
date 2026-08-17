//! Deterministic composition engine module.

pub mod backend;

use crate::engine::backend::{is_boundary_char, transform_raw};
use crate::types::{
    CompositionSnapshot, EngineAction, InputEvent, InputKind, InputMethod, TonePlacement,
};
use serde::{Deserialize, Serialize};

/// Configuration for engine composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineConfig {
    pub method: InputMethod,
    pub tone_placement: TonePlacement,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            method: InputMethod::Telex,
            tone_placement: TonePlacement::Modern,
        }
    }
}

/// Deterministic Vietnamese keyboard engine.
#[derive(Debug, Clone)]
pub struct Engine {
    config: EngineConfig,
    raw_keys: Vec<char>,
    revision: u64,
    rendered_cache: String,
}

impl Engine {
    #[must_use]
    pub fn new(config: EngineConfig) -> Self {
        Self {
            config,
            raw_keys: Vec::new(),
            revision: 0,
            rendered_cache: String::new(),
        }
    }

    #[must_use]
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: EngineConfig) {
        self.config = config;
        self.recompute();
    }

    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub fn raw_keys(&self) -> &[char] {
        &self.raw_keys
    }

    #[must_use]
    pub fn rendered(&self) -> &str {
        &self.rendered_cache
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.raw_keys.is_empty()
    }

    #[must_use]
    pub fn snapshot(&self) -> CompositionSnapshot {
        let raw_str: String = self.raw_keys.iter().collect();
        CompositionSnapshot::new(self.revision, raw_str, self.rendered_cache.clone())
    }

    pub fn reset(&mut self) {
        self.raw_keys.clear();
        self.rendered_cache.clear();
        self.revision = self.revision.wrapping_add(1);
    }

    fn recompute(&mut self) {
        self.rendered_cache = transform_raw(
            &self.raw_keys,
            self.config.method,
            self.config.tone_placement,
        );
    }

    /// Processes an input event and produces self-contained engine actions.
    pub fn process(&mut self, event: &InputEvent) -> Vec<EngineAction> {
        // If context disables transformations, passthrough without retaining state
        if !event.context.allow_transform {
            return self.process_passthrough(event);
        }

        match &event.kind {
            InputKind::Reset => {
                self.reset();
                vec![EngineAction::UpdateComposition {
                    revision: self.revision,
                    text: String::new(),
                }]
            }
            InputKind::CursorMoved | InputKind::SelectionChanged => {
                if self.raw_keys.is_empty() {
                    Vec::new()
                } else {
                    self.reset();
                    vec![EngineAction::UpdateComposition {
                        revision: self.revision,
                        text: String::new(),
                    }]
                }
            }
            InputKind::Boundary { delimiter } => {
                let text = if self.raw_keys.is_empty() {
                    String::new()
                } else {
                    self.rendered_cache.clone()
                };
                self.reset();
                vec![EngineAction::Commit {
                    revision: self.revision,
                    text,
                    delimiter: Some(*delimiter),
                }]
            }
            InputKind::Backspace => {
                if self.raw_keys.is_empty() {
                    Vec::new()
                } else {
                    self.raw_keys.pop();
                    self.recompute();
                    self.revision = self.revision.wrapping_add(1);
                    vec![EngineAction::UpdateComposition {
                        revision: self.revision,
                        text: self.rendered_cache.clone(),
                    }]
                }
            }
            InputKind::Key { logical, .. } => {
                if is_boundary_char(*logical) {
                    let text = if self.raw_keys.is_empty() {
                        String::new()
                    } else {
                        self.rendered_cache.clone()
                    };
                    self.reset();
                    vec![EngineAction::Commit {
                        revision: self.revision,
                        text,
                        delimiter: Some(*logical),
                    }]
                } else {
                    self.raw_keys.push(*logical);
                    self.recompute();
                    self.revision = self.revision.wrapping_add(1);
                    vec![EngineAction::UpdateComposition {
                        revision: self.revision,
                        text: self.rendered_cache.clone(),
                    }]
                }
            }
            InputKind::InsertText { text } => {
                let mut actions = Vec::new();
                if !self.raw_keys.is_empty() {
                    let committed = self.rendered_cache.clone();
                    self.reset();
                    actions.push(EngineAction::Commit {
                        revision: self.revision,
                        text: committed,
                        delimiter: None,
                    });
                }
                self.revision = self.revision.wrapping_add(1);
                actions.push(EngineAction::Commit {
                    revision: self.revision,
                    text: text.clone(),
                    delimiter: None,
                });
                actions
            }
        }
    }

    fn process_passthrough(&mut self, event: &InputEvent) -> Vec<EngineAction> {
        let mut actions = Vec::new();
        if self.raw_keys.is_empty() {
            // No active composition to flush
        } else {
            let text = self.rendered_cache.clone();
            self.reset();
            actions.push(EngineAction::Commit {
                revision: self.revision,
                text,
                delimiter: None,
            });
        }

        match &event.kind {
            InputKind::Key { logical, .. } => {
                self.revision = self.revision.wrapping_add(1);
                actions.push(EngineAction::Commit {
                    revision: self.revision,
                    text: logical.to_string(),
                    delimiter: None,
                });
            }
            InputKind::Boundary { delimiter } => {
                self.revision = self.revision.wrapping_add(1);
                actions.push(EngineAction::Commit {
                    revision: self.revision,
                    text: String::new(),
                    delimiter: Some(*delimiter),
                });
            }
            InputKind::InsertText { text } => {
                self.revision = self.revision.wrapping_add(1);
                actions.push(EngineAction::Commit {
                    revision: self.revision,
                    text: text.clone(),
                    delimiter: None,
                });
            }
            _ => {}
        }
        actions
    }
}
