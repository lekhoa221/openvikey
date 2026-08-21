//! Bounded, local token statistics used only as ranking signals.

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

const MAX_TOKEN_BYTES: usize = 128;
const DEFAULT_MAX_UNIGRAMS: usize = 10_000;
const DEFAULT_MAX_BIGRAMS: usize = 30_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnigramRow {
    token_nfc: String,
    count: u64,
    last_used_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyBigramRow {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnigramInspectionRow {
    pub token_nfc: String,
    pub count: u64,
    pub last_used_at_ms: i64,
}

/// Persisted unigram/bigram namespace in adaptive-model payload v2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct UserLanguageModel {
    unigrams: Vec<UnigramRow>,
    bigrams: Vec<EmptyBigramRow>,
    #[serde(default)]
    last_transaction_id: Option<u64>,
}

impl UserLanguageModel {
    /// Records one safe, single-token commit.
    pub fn commit(&mut self, token: &str, left_token: Option<&str>, at_ms: i64) -> bool {
        self.commit_bounded(
            token,
            left_token,
            at_ms,
            DEFAULT_MAX_UNIGRAMS,
            DEFAULT_MAX_BIGRAMS,
        )
    }

    /// Records one commit and deterministically enforces configured storage caps.
    pub fn commit_bounded(
        &mut self,
        token: &str,
        _left_token: Option<&str>,
        at_ms: i64,
        max_unigrams: usize,
        _max_bigrams: usize,
    ) -> bool {
        let Some(token_nfc) = learnable_token(token) else {
            return false;
        };
        let inserted = match self
            .unigrams
            .binary_search_by(|row| row.token_nfc.cmp(&token_nfc))
        {
            Ok(index) => {
                let row = &mut self.unigrams[index];
                row.count = row.count.saturating_add(1);
                row.last_used_at_ms = row.last_used_at_ms.max(at_ms);
                false
            }
            Err(index) => {
                self.unigrams.insert(
                    index,
                    UnigramRow {
                        token_nfc: token_nfc.clone(),
                        count: 1,
                        last_used_at_ms: at_ms,
                    },
                );
                true
            }
        };
        let max_unigrams = max_unigrams.max(1);
        while self.unigrams.len() > max_unigrams {
            let victim = self
                .unigrams
                .iter()
                .enumerate()
                .filter(|(_, row)| !inserted || row.token_nfc != token_nfc)
                .min_by(|(_, left), (_, right)| {
                    (left.count, left.last_used_at_ms, &left.token_nfc).cmp(&(
                        right.count,
                        right.last_used_at_ms,
                        &right.token_nfc,
                    ))
                })
                .map_or(0, |(index, _)| index);
            self.unigrams.remove(victim);
        }
        true
    }

    /// Records a monotonically identified commit exactly once.
    pub fn commit_transaction(
        &mut self,
        token: &str,
        left_token: Option<&str>,
        at_ms: i64,
        transaction_id: u64,
    ) -> bool {
        self.commit_transaction_bounded(
            token,
            left_token,
            at_ms,
            transaction_id,
            DEFAULT_MAX_UNIGRAMS,
            DEFAULT_MAX_BIGRAMS,
        )
    }

    pub fn commit_transaction_bounded(
        &mut self,
        token: &str,
        left_token: Option<&str>,
        at_ms: i64,
        transaction_id: u64,
        max_unigrams: usize,
        max_bigrams: usize,
    ) -> bool {
        if self
            .last_transaction_id
            .is_some_and(|last| transaction_id <= last)
        {
            return false;
        }
        if !self.commit_bounded(token, left_token, at_ms, max_unigrams, max_bigrams) {
            return false;
        }
        self.last_transaction_id = Some(transaction_id);
        true
    }

    pub fn enforce_limits(&mut self, max_unigrams: usize, _max_bigrams: usize) {
        let max_unigrams = max_unigrams.max(1);
        while self.unigrams.len() > max_unigrams {
            let victim = self
                .unigrams
                .iter()
                .enumerate()
                .min_by(|(_, left), (_, right)| {
                    (left.count, left.last_used_at_ms, &left.token_nfc).cmp(&(
                        right.count,
                        right.last_used_at_ms,
                        &right.token_nfc,
                    ))
                })
                .map_or(0, |(index, _)| index);
            self.unigrams.remove(victim);
        }
    }

    #[must_use]
    pub fn unigram(&self, token: &str) -> u64 {
        let token_nfc = nfc(token);
        self.unigrams
            .binary_search_by(|row| row.token_nfc.cmp(&token_nfc))
            .ok()
            .map_or(0, |index| self.unigrams[index].count)
    }

    #[must_use]
    pub fn unigram_count(&self) -> usize {
        self.unigrams.len()
    }

    #[must_use]
    pub fn unigram_rows(&self) -> Vec<UnigramInspectionRow> {
        self.unigrams
            .iter()
            .map(|row| UnigramInspectionRow {
                token_nfc: row.token_nfc.clone(),
                count: row.count,
                last_used_at_ms: row.last_used_at_ms,
            })
            .collect()
    }

    pub fn forget_token(&mut self, token: &str) -> bool {
        let token_nfc = nfc(token);
        let Ok(index) = self
            .unigrams
            .binary_search_by(|row| row.token_nfc.cmp(&token_nfc))
        else {
            return false;
        };
        self.unigrams.remove(index);
        true
    }

    /// Returns a bounded ranking signal in `[0, 1]`.
    #[must_use]
    pub fn unigram_signal(&self, token: &str) -> f64 {
        let count = self.unigram(token);
        if count == 0 {
            return 0.0;
        }
        let bounded = u32::try_from(count).unwrap_or(u32::MAX);
        (f64::from(bounded).ln_1p() / 17_f64.ln()).min(1.0)
    }
}

fn learnable_token(token: &str) -> Option<String> {
    let token_nfc = nfc(token);
    if token_nfc.is_empty()
        || token_nfc.len() > MAX_TOKEN_BYTES
        || token_nfc.chars().any(char::is_whitespace)
        || !token_nfc.chars().any(char::is_alphabetic)
        || token_nfc
            .chars()
            .any(|ch| matches!(ch, '/' | '\\' | '@' | ':' | '='))
    {
        return None;
    }
    Some(token_nfc)
}

fn nfc(text: &str) -> String {
    text.nfc().collect()
}
