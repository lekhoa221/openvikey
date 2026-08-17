//! Core semantic types, contracts, input/output events, and undo tracking.
//!
//! Frozen at Wave 0 (ADR 0002). Add fields only with a new ADR; do not
//! change the meaning of existing variants.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

/// Engine input method type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum InputMethod {
    Telex,
    Vni,
}

/// Tone placement convention (modern vs classic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TonePlacement {
    #[default]
    Modern, // e.g. oà, uý, hoá (new accent style)
    Classic, // e.g. òa, úy, hóa (old accent style)
}

/// Operational context flags accompanying an input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputContext {
    pub allow_transform: bool,
    pub allow_learning: bool,
}

impl Default for InputContext {
    fn default() -> Self {
        Self {
            allow_transform: true,
            allow_learning: true,
        }
    }
}

/// Keyboard modifiers accompanying key events.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
    pub caps_lock: bool,
}

impl Modifiers {
    pub const fn empty() -> Self {
        Self {
            shift: false,
            control: false,
            alt: false,
            meta: false,
            caps_lock: false,
        }
    }
}

/// Event kind indicating physical or semantic action from caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputKind {
    Key {
        logical: char,
        physical: Option<u16>,
    },
    Backspace,
    Boundary {
        delimiter: char,
    },
    InsertText {
        text: String,
    },
    CursorMoved,
    SelectionChanged,
    Reset,
}

/// Deterministic input event packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputEvent {
    pub seq: u64,
    pub at_ms: i64,
    pub kind: InputKind,
    pub modifiers: Modifiers,
    pub is_repeat: bool,
    pub context: InputContext,
}

/// Snapshot of the in-progress composition buffer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositionSnapshot {
    pub revision: u64,
    pub raw_keys: String,
    pub rendered: String,
    pub normalized: String,
}

impl CompositionSnapshot {
    pub fn new(revision: u64, raw_keys: String, rendered: String) -> Self {
        let normalized = rendered.nfc().collect::<String>();
        Self {
            revision,
            raw_keys,
            rendered,
            normalized,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.raw_keys.is_empty() && self.rendered.is_empty()
    }
}

/// Basis for editing ranges in the consumer's text store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RangeBasis {
    ActiveComposition,
    CommittedBeforeCaret,
}

/// Edit range specified in grapheme clusters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditRange {
    pub basis: RangeBasis,
    pub start_grapheme: usize,
    pub length_grapheme: usize,
    pub revision: u64,
}

impl EditRange {
    pub fn grapheme_count(&self) -> usize {
        self.length_grapheme
    }
}

/// Self-contained replace action for autocorrect and expansions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplaceRangeAction {
    pub edit_id: u64,
    pub range: EditRange,
    pub original: String,
    pub replacement: String,
    pub delimiter: Option<char>,
}

impl ReplaceRangeAction {
    /// Generates an inverse action to undo this replacement.
    #[must_use]
    pub fn to_inverse(&self, current_revision: u64) -> Self {
        let replacement_graphemes = self.replacement.graphemes(true).count();
        Self {
            edit_id: self.edit_id,
            range: EditRange {
                basis: self.range.basis,
                start_grapheme: self.range.start_grapheme,
                length_grapheme: replacement_graphemes,
                revision: current_revision,
            },
            original: self.replacement.clone(),
            replacement: self.original.clone(),
            delimiter: self.delimiter,
        }
    }
}

/// Generator rule source identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CandidateSource {
    TelexFix,
    Fuzzy,
    Abbreviation,
    Diacritics,
}

/// Candidate word suggestion emitted by generators.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: u64,
    pub text: String,
    pub source: CandidateSource,
    pub evidence: String,
    pub base_score: f64,
    pub final_score: f64,
}

/// Self-contained actions produced by the engine for the consumer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EngineAction {
    UpdateComposition {
        revision: u64,
        text: String,
    },
    Commit {
        revision: u64,
        text: String,
        delimiter: Option<char>,
    },
    ReplaceRange(ReplaceRangeAction),
    ShowSuggestions {
        revision: u64,
        candidates: Vec<Candidate>,
    },
}

/// Kinds of explicit and implicit feedback signals for adaptive learning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeedbackKind {
    Accept {
        candidate_id: u64,
    },
    ExplicitReject {
        candidate_id: u64,
    },
    Undo {
        edit_id: u64,
    },
    AutoSettled {
        edit_id: u64,
    },
    SuggestionSettled {
        candidate_id: u64,
    },
    ImplicitCorrection {
        original: String,
        replacement: String,
    },
}

/// Feedback event sent to the learning model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackEvent {
    pub seq: u64,
    pub at_ms: i64,
    pub kind: FeedbackKind,
}

/// Bounded in-memory tracker for undo operations.
#[derive(Debug, Clone)]
pub struct UndoTracker {
    max_entries: usize,
    entries: VecDeque<ReplaceRangeAction>,
    is_valid: bool,
}

impl UndoTracker {
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            entries: VecDeque::with_capacity(max_entries),
            is_valid: true,
        }
    }

    /// Records an auto-applied edit.
    pub fn record_edit(&mut self, action: ReplaceRangeAction) {
        if self.entries.len() >= self.max_entries {
            self.entries.pop_front();
        }
        self.entries.push_back(action);
        self.is_valid = true;
    }

    /// Invalidates undoable edits after CursorMoved or SelectionChanged.
    pub fn invalidate_due_to_caret_break(&mut self) {
        self.entries.clear();
        self.is_valid = false;
    }

    /// Pops the most recent valid edit if the expected revision matches and generates the inverse action.
    pub fn pop_undo(&mut self, expected_revision: u64) -> Option<ReplaceRangeAction> {
        if !self.is_valid {
            return None;
        }
        let last = self.entries.back()?;
        if last.range.revision != expected_revision {
            return None;
        }
        self.entries
            .pop_back()
            .map(|action| action.to_inverse(expected_revision.wrapping_add(1)))
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
