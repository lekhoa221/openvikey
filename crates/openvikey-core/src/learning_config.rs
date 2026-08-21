//! Versioned learning configuration for the v2 intervention planner.

use crate::decision::DecisionConfig;
use crate::rank::ScoreConfig;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Shared, versioned learning parameters. Users do not self-tune these values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LearningConfigV2 {
    pub version: u32,
    pub minimum_correction_graphemes: usize,
    pub immediate_revert_window_ms: i64,
    pub reapply_cooldown_ms: i64,
    pub abbrev_cold_start_auto: bool,
    pub fuzzy_heuristic_assist: bool,
    pub decision: DecisionConfig,
    pub score: ScoreConfig,
    pub context_shrinkage_k: f64,
    pub auto_margin: f64,
    pub weak_positive_cap: f64,
    pub max_unigrams: usize,
    pub max_bigrams: usize,
    pub max_corrections: usize,
    pub max_context_rows: usize,
    pub max_recent_events_per_bucket: usize,
}

impl LearningConfigV2 {
    /// Keep current Auto policy for Lát 1–8. Lát 2 sets the grapheme floor to 2.
    #[must_use]
    pub fn compatibility_v1() -> Self {
        Self {
            version: 1,
            minimum_correction_graphemes: 2,
            immediate_revert_window_ms: 3_000,
            reapply_cooldown_ms: 3_000,
            abbrev_cold_start_auto: true,
            fuzzy_heuristic_assist: true,
            decision: DecisionConfig::default(),
            score: ScoreConfig::abbrev_v1(),
            context_shrinkage_k: 2.0,
            auto_margin: 0.02,
            weak_positive_cap: 7.2,
            max_unigrams: 10_000,
            max_bigrams: 30_000,
            max_corrections: 10_000,
            max_context_rows: 30_000,
            max_recent_events_per_bucket: 64,
        }
    }

    /// Locked product defaults. Hosts switch to this in Lát 9.
    #[must_use]
    pub fn product_v2() -> Self {
        Self {
            version: 2,
            abbrev_cold_start_auto: false,
            fuzzy_heuristic_assist: false,
            minimum_correction_graphemes: 2,
            ..Self::compatibility_v1()
        }
    }

    #[must_use]
    pub fn hash(&self) -> String {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        hex_lower(&Sha256::digest(bytes))
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}
