//! Bounded, local token statistics used only as ranking signals.

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

const MAX_TOKEN_BYTES: usize = 128;

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
    pub fn commit(&mut self, token: &str, _left_token: Option<&str>, at_ms: i64) -> bool {
        let Some(token_nfc) = learnable_token(token) else {
            return false;
        };
        match self
            .unigrams
            .binary_search_by(|row| row.token_nfc.cmp(&token_nfc))
        {
            Ok(index) => {
                let row = &mut self.unigrams[index];
                row.count = row.count.saturating_add(1);
                row.last_used_at_ms = row.last_used_at_ms.max(at_ms);
            }
            Err(index) => self.unigrams.insert(
                index,
                UnigramRow {
                    token_nfc,
                    count: 1,
                    last_used_at_ms: at_ms,
                },
            ),
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
        if self
            .last_transaction_id
            .is_some_and(|last| transaction_id <= last)
        {
            return false;
        }
        if !self.commit(token, left_token, at_ms) {
            return false;
        }
        self.last_transaction_id = Some(transaction_id);
        true
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
