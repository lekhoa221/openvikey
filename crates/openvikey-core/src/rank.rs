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
    let existing_rule = primary_rule_id(&existing.evidence);
    let incoming_rule = primary_rule_id(&incoming.evidence);
    let score_order = incoming.base_score.total_cmp(&existing.base_score);
    let incoming_wins =
        score_order.is_gt() || (score_order.is_eq() && incoming_rule < existing_rule);
    let winning_rule = if incoming_wins {
        incoming_rule.to_string()
    } else {
        existing_rule.to_string()
    };

    let mut parts: BTreeSet<String> = existing
        .evidence
        .split('+')
        .chain(incoming.evidence.split('+'))
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();
    parts.remove(&winning_rule);

    if incoming_wins {
        existing.base_score = incoming.base_score;
        existing.id = incoming.id;
        existing.source = incoming.source;
        existing.text = nfc(&incoming.text);
    }

    existing.evidence = std::iter::once(winning_rule)
        .chain(parts)
        .collect::<Vec<_>>()
        .join("+");
}

fn primary_rule_id(evidence: &str) -> &str {
    evidence.split('+').next().unwrap_or("")
}

fn nfc(text: &str) -> String {
    text.nfc().collect()
}
