//! Normalize, dedupe, and order candidates. Personal rerank reads `ModelView` only.

use crate::generate::abbrev::ABBREV_SEED_SHA256;
use crate::model::{ModelView, RuleContextKey};
use crate::types::{Candidate, InputMethod};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::UnicodeNormalization;

pub const SCORE_CONFIG_V1_HASH: &str =
    "566ca77091a5afc5cc4b89ab5137aefe3261bc081e6e5e78144b0f0d226efb8d";

const fn default_unigram_weight() -> f64 {
    0.08
}

/// Versioned score calibration. Fit only on the calibration split (M9/M10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreConfig {
    pub version: u32,
    pub hash: String,
    pub calibration_source_hash: String,
    pub personal_weight: f64,
    #[serde(default = "default_unigram_weight")]
    pub unigram_weight: f64,
}

/// Context required to map a candidate to its personal learning key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankingContext {
    pub input_method: InputMethod,
    pub original_nfc: String,
    pub left_token_nfc: Option<String>,
}

impl ScoreConfig {
    #[must_use]
    pub fn abbrev_v1() -> Self {
        Self {
            version: 1,
            hash: SCORE_CONFIG_V1_HASH.to_string(),
            calibration_source_hash: ABBREV_SEED_SHA256.to_string(),
            personal_weight: 0.2,
            unigram_weight: default_unigram_weight(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScoreContributions {
    pub exact_correction: f64,
    pub unigram: f64,
    pub final_score: f64,
}

/// Computes independently inspectable score terms for one candidate.
#[must_use]
pub fn score_contributions(
    candidate: &Candidate,
    model: &dyn ModelView,
    evaluate_at_ms: i64,
    config: &ScoreConfig,
    context: &RankingContext,
) -> ScoreContributions {
    let key = RuleContextKey {
        input_method: context.input_method,
        source: candidate.source,
        original_nfc: context.original_nfc.clone(),
        candidate_nfc: candidate.text.clone(),
        left_token_nfc: context.left_token_nfc.clone(),
        source_rule_id: primary_rule_id(&candidate.evidence).to_string(),
    };
    let exact_correction = config.personal_weight * (model.confidence(&key, evaluate_at_ms) - 0.5);
    let unigram = config.unigram_weight * model.unigram_signal(&candidate.text);
    ScoreContributions {
        exact_correction,
        unigram,
        final_score: (candidate.base_score.clamp(0.0, 1.0) + exact_correction + unigram)
            .clamp(0.0, 1.0),
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
    context: Option<&RankingContext>,
) -> Vec<Candidate> {
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
        let base = candidate.base_score.clamp(0.0, 1.0);
        candidate.final_score = if let Some(context) = context {
            score_contributions(candidate, model, evaluate_at_ms, config, context).final_score
        } else {
            base
        };
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
