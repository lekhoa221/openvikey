//! Seeded abbreviation expansions. No model access.

use crate::generate::{Generator, LeftContext};
use crate::types::{Candidate, CandidateSource, CompositionSnapshot};
use serde::Deserialize;
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

pub const ABBREV_SEED_JSONL: &str = include_str!("abbrev_seed.jsonl");

/// SHA-256 of `abbrev_seed.jsonl` (source of `ScoreConfig.hash`).
pub const ABBREV_SEED_SHA256: &str =
    "dbb797aebbd7cc4dbc6f4dbb520ebafe0a132af3fd35a5fd8c2fc1b9ce87441d";

#[derive(Debug, Deserialize)]
struct SeedFileRow {
    input_nfc: String,
    expansion_nfc: String,
    evidence: String,
    base_score: f64,
}

struct SeedRow {
    expansion: String,
    evidence: String,
    base_score: f64,
}

/// Table-driven abbreviation generator.
pub struct AbbrevGenerator {
    by_input: BTreeMap<String, Vec<SeedRow>>,
}

impl AbbrevGenerator {
    #[must_use]
    pub fn from_seed() -> Self {
        Self::from_jsonl(ABBREV_SEED_JSONL)
    }

    #[must_use]
    pub fn from_jsonl(jsonl: &str) -> Self {
        let mut by_input: BTreeMap<String, Vec<SeedRow>> = BTreeMap::new();
        for line in jsonl.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let row: SeedFileRow = serde_json::from_str(line).expect("abbrev seed jsonl");
            by_input
                .entry(nfc(&row.input_nfc))
                .or_default()
                .push(SeedRow {
                    expansion: nfc(&row.expansion_nfc),
                    evidence: row.evidence,
                    base_score: row.base_score,
                });
        }
        Self { by_input }
    }

    #[must_use]
    pub fn expansions_for(&self, input_nfc: &str) -> Vec<(String, String, f64)> {
        self.by_input
            .get(&nfc(input_nfc))
            .map(|rows| {
                rows.iter()
                    .map(|row| (row.expansion.clone(), row.evidence.clone(), row.base_score))
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Generator for AbbrevGenerator {
    fn source(&self) -> CandidateSource {
        CandidateSource::Abbreviation
    }

    fn generate(
        &self,
        snapshot: &CompositionSnapshot,
        _left_context: &LeftContext,
    ) -> Vec<Candidate> {
        let key = nfc(&snapshot.normalized);
        let Some(rows) = self.by_input.get(&key) else {
            return Vec::new();
        };
        rows.iter()
            .enumerate()
            .map(|(idx, row)| Candidate {
                id: u64::try_from(idx).unwrap_or(u64::MAX) + 1,
                text: row.expansion.clone(),
                source: CandidateSource::Abbreviation,
                evidence: row.evidence.clone(),
                base_score: row.base_score,
                final_score: 0.0,
            })
            .collect()
    }
}

fn nfc(text: &str) -> String {
    text.nfc().collect()
}
