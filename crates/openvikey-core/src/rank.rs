//! Normalize, dedupe, and order candidates. Personal rerank reads `ModelView` only.

use crate::generate::abbrev::ABBREV_SEED_SHA256;
use crate::model::ModelView;
use crate::types::Candidate;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::UnicodeNormalization;

/// Versioned score calibration. Fit only on the calibration split (M9/M10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoreConfig {
    pub version: u32,
    pub hash: String,
}

impl ScoreConfig {
    #[must_use]
    pub fn abbrev_v1() -> Self {
        Self {
            version: 1,
            hash: ABBREV_SEED_SHA256.to_string(),
        }
    }
}

impl Default for ScoreConfig {
    fn default() -> Self {
        Self::abbrev_v1()
    }
}

/// Dedupe by NFC text, then score in `[0, 1]`, then stable lexical NFC tie-break.
///
/// Cold-start / empty model leaves `final_score = base_score`. Milestone 6
/// will weight by `ModelView::confidence`.
#[must_use]
pub fn rank(
    candidates: Vec<Candidate>,
    model: &dyn ModelView,
    evaluate_at_ms: i64,
    config: &ScoreConfig,
) -> Vec<Candidate> {
    let _ = (model, evaluate_at_ms, config);
    let mut merged: BTreeMap<String, Candidate> = BTreeMap::new();
    for candidate in candidates {
        let key = nfc(&candidate.text);
        match merged.get_mut(&key) {
            None => {
                let mut first = candidate;
                first.text.clone_from(&key);
                merged.insert(key, first);
            }
            Some(existing) => merge_same_text(existing, &candidate),
        }
    }

    let mut ranked: Vec<Candidate> = merged.into_values().collect();
    for candidate in &mut ranked {
        candidate.final_score = candidate.base_score.clamp(0.0, 1.0);
    }
    ranked.sort_by(|a, b| {
        b.final_score
            .total_cmp(&a.final_score)
            .then_with(|| nfc(&a.text).cmp(&nfc(&b.text)))
    });
    ranked
}

fn merge_same_text(existing: &mut Candidate, incoming: &Candidate) {
    let mut parts: BTreeSet<String> = existing
        .evidence
        .split('+')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();
    for part in incoming.evidence.split('+').filter(|part| !part.is_empty()) {
        parts.insert(part.to_string());
    }
    if incoming.base_score > existing.base_score {
        existing.base_score = incoming.base_score;
        existing.id = incoming.id;
        existing.source = incoming.source;
        existing.text = nfc(&incoming.text);
    }
    existing.evidence = parts.into_iter().collect::<Vec<_>>().join("+");
}

fn nfc(text: &str) -> String {
    text.nfc().collect()
}
