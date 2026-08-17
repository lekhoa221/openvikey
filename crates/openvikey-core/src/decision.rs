//! Versioned decision policy surface.
//!
//! Wave 0 locks thresholds and source caps. The ignore/suggest/auto
//! state machine itself is Milestone 5/6.

use crate::types::CandidateSource;
use serde::{Deserialize, Serialize};

/// Hard cap a source may never exceed, even at high confidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionCap {
    Suggest,
    Auto,
}

/// Learned / scored action for one rule-context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum DecisionState {
    #[default]
    Ignore,
    Suggest,
    Auto,
}

/// Versioned score/decision thresholds. Changing values requires a new `version`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionConfig {
    pub version: u32,
    pub suggest_on: f64,
    pub suggest_off: f64,
    pub auto_score: f64,
    pub auto_confidence: f64,
    pub promote_positive_mass: f64,
}

impl Default for DecisionConfig {
    fn default() -> Self {
        Self {
            version: 1,
            suggest_on: 0.70,
            suggest_off: 0.60,
            auto_score: 0.90,
            auto_confidence: 0.95,
            promote_positive_mass: 18.0,
        }
    }
}

impl CandidateSource {
    /// Diacritics are suggestion-only in v1.
    #[must_use]
    pub fn max_action(self) -> ActionCap {
        match self {
            Self::Diacritics => ActionCap::Suggest,
            Self::TelexFix | Self::Fuzzy | Self::Abbreviation => ActionCap::Auto,
        }
    }
}
