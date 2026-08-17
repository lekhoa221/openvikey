//! Compact authored lexicon + left-context bigram lookup.
//!
//! Wave 0 locks the seam only. Loading a production artifact from a
//! verified manifest is Milestone 4.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One lexicon token, keyed by NFC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LexiconEntry {
    pub token_nfc: String,
    pub frequency: u32,
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
            .map(|entry| (entry.token_nfc.clone(), entry))
            .collect();
        Self {
            entries,
            bigrams: bigrams.into_iter().collect(),
            source_manifest_hash: source_manifest_hash.map(str::to_string),
        }
    }

    #[must_use]
    pub fn lookup(&self, token_nfc: &str) -> Option<&LexiconEntry> {
        self.entries.get(token_nfc)
    }

    #[must_use]
    pub fn contains(&self, token_nfc: &str) -> bool {
        self.entries.contains_key(token_nfc)
    }

    #[must_use]
    pub fn bigram(&self, left_nfc: &str, token_nfc: &str) -> Option<f64> {
        self.bigrams
            .get(&(left_nfc.to_string(), token_nfc.to_string()))
            .copied()
    }

    #[must_use]
    pub fn source_manifest_hash(&self) -> Option<&str> {
        self.source_manifest_hash.as_deref()
    }
}

impl Default for Lexicon {
    fn default() -> Self {
        Self::empty()
    }
}
