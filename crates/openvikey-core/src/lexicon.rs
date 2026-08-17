//! Compact authored lexicon + left-context bigram lookup.
//!
//! Milestone 4: artifacts record the source corpus-manifest SHA-256.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

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
#[derive(Debug, Clone, PartialEq)]
pub struct Lexicon {
    entries: BTreeMap<String, LexiconEntry>,
    bigrams: BTreeMap<(String, String), f64>,
    source_manifest_hash: Option<String>,
    folded_by_token: BTreeMap<String, String>,
    tokens_by_folded_length: BTreeMap<usize, Vec<String>>,
    tokens_by_folded_form: BTreeMap<String, Vec<String>>,
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
        let entries: BTreeMap<String, LexiconEntry> = entries
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
        let mut folded_by_token = BTreeMap::new();
        let mut tokens_by_folded_length: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        let mut tokens_by_folded_form: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for token in entries.keys() {
            let folded = fold_for_index(token);
            folded_by_token.insert(token.clone(), folded.clone());
            tokens_by_folded_length
                .entry(folded.len())
                .or_default()
                .push(token.clone());
            tokens_by_folded_form
                .entry(folded)
                .or_default()
                .push(token.clone());
        }
        Self {
            entries,
            bigrams,
            source_manifest_hash: source_manifest_hash.map(str::to_string),
            folded_by_token,
            tokens_by_folded_length,
            tokens_by_folded_form,
        }
    }

    #[must_use]
    pub fn lookup(&self, token_nfc: &str) -> Option<&LexiconEntry> {
        self.entries.get(&nfc(token_nfc))
    }

    pub fn entries(&self) -> impl ExactSizeIterator<Item = &LexiconEntry> {
        self.entries.values()
    }

    pub(crate) fn entries_near_folded_length(
        &self,
        folded_length: usize,
        max_delta: usize,
    ) -> impl Iterator<Item = (&LexiconEntry, &str)> {
        let minimum = folded_length.saturating_sub(max_delta);
        let maximum = folded_length.saturating_add(max_delta);
        self.tokens_by_folded_length
            .range(minimum..=maximum)
            .flat_map(|(_, tokens)| tokens)
            .filter_map(|token| {
                self.entries
                    .get(token)
                    .zip(self.folded_by_token.get(token).map(String::as_str))
            })
    }

    pub(crate) fn entries_with_folded_form(
        &self,
        folded: &str,
    ) -> impl Iterator<Item = &LexiconEntry> {
        self.tokens_by_folded_form
            .get(folded)
            .into_iter()
            .flatten()
            .filter_map(|token| self.entries.get(token))
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

fn fold_for_index(text: &str) -> String {
    text.nfd()
        .filter(|ch| !is_combining_mark(*ch))
        .flat_map(char::to_lowercase)
        .map(|ch| if ch == 'đ' { 'd' } else { ch })
        .collect()
}
