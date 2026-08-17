//! Compact authored lexicon + left-context bigram lookup.
//!
//! Milestone 4: artifacts record the source corpus-manifest SHA-256.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

fn nfc(text: &str) -> String {
    text.nfc().collect()
}

/// One lexicon token, keyed by NFC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LexiconEntry {
    pub token_nfc: String,
    pub frequency: u32,
}

/// Left-context bigram stored in a lexicon artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LexiconBigram {
    pub left_nfc: String,
    pub token_nfc: String,
    pub score: f64,
}

/// Versioned compact lexicon written by the lab pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LexiconArtifact {
    pub source_manifest_hash: String,
    pub entries: Vec<LexiconEntry>,
    pub bigrams: Vec<LexiconBigram>,
}

/// Deterministic in-memory lexicon. Iteration order is sorted NFC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lexicon {
    entries: BTreeMap<String, LexiconEntry>,
    bigrams: BTreeMap<(String, String), f64>,
    source_manifest_hash: Option<String>,
}

impl Lexicon {
    #[must_use]
    pub fn empty() -> Self {
        Self::from_entries([], [], None)
    }

    #[must_use]
    pub fn from_entries(
        entries: impl IntoIterator<Item = LexiconEntry>,
        bigrams: impl IntoIterator<Item = ((String, String), f64)>,
        source_manifest_hash: Option<&str>,
    ) -> Self {
        let entries = entries
            .into_iter()
            .map(|mut entry| {
                entry.token_nfc = nfc(&entry.token_nfc);
                (entry.token_nfc.clone(), entry)
            })
            .collect();
        let bigrams = bigrams
            .into_iter()
            .map(|((left, token), score)| ((nfc(&left), nfc(&token)), score))
            .collect();
        Self {
            entries,
            bigrams,
            source_manifest_hash: source_manifest_hash.map(str::to_string),
        }
    }

    #[must_use]
    pub fn lookup(&self, token_nfc: &str) -> Option<&LexiconEntry> {
        self.entries.get(&nfc(token_nfc))
    }

    pub fn entries(&self) -> impl ExactSizeIterator<Item = &LexiconEntry> {
        self.entries.values()
    }

    #[must_use]
    pub fn contains(&self, token_nfc: &str) -> bool {
        self.entries.contains_key(&nfc(token_nfc))
    }

    #[must_use]
    pub fn bigram(&self, left_nfc: &str, token_nfc: &str) -> Option<f64> {
        self.bigrams.get(&(nfc(left_nfc), nfc(token_nfc))).copied()
    }

    #[must_use]
    pub fn source_manifest_hash(&self) -> Option<&str> {
        self.source_manifest_hash.as_deref()
    }

    #[must_use]
    pub fn from_artifact(artifact: LexiconArtifact) -> Self {
        let hash = artifact.source_manifest_hash.clone();
        let bigrams = artifact
            .bigrams
            .into_iter()
            .map(|row| ((row.left_nfc, row.token_nfc), row.score));
        Self::from_entries(artifact.entries, bigrams, Some(&hash))
    }

    #[must_use]
    pub fn to_artifact(&self) -> LexiconArtifact {
        LexiconArtifact {
            source_manifest_hash: self.source_manifest_hash.clone().unwrap_or_default(),
            entries: self.entries.values().cloned().collect(),
            bigrams: self
                .bigrams
                .iter()
                .map(|((left, token), score)| LexiconBigram {
                    left_nfc: left.clone(),
                    token_nfc: token.clone(),
                    score: *score,
                })
                .collect(),
        }
    }
}

impl Default for Lexicon {
    fn default() -> Self {
        Self::empty()
    }
}
