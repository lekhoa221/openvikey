//! Per-token diacritics suggestions ranked by unigram and left bigram evidence.

use crate::generate::vietnamese::{folded_ascii, is_supported_lexicon_token};
use crate::generate::{Generator, LeftContext};
use crate::lexicon::Lexicon;
use crate::types::{Candidate, CandidateSource, CompositionSnapshot};
use unicode_normalization::UnicodeNormalization;

pub struct DiacriticsGenerator<'a> {
    lexicon: &'a Lexicon,
    top_k: usize,
}

impl<'a> DiacriticsGenerator<'a> {
    #[must_use]
    pub const fn new(lexicon: &'a Lexicon, top_k: usize) -> Self {
        Self { lexicon, top_k }
    }
}

impl Generator for DiacriticsGenerator<'_> {
    fn source(&self) -> CandidateSource {
        CandidateSource::Diacritics
    }

    fn generate(
        &self,
        snapshot: &CompositionSnapshot,
        left_context: &LeftContext,
    ) -> Vec<Candidate> {
        if self.top_k == 0
            || snapshot.normalized.is_empty()
            || !snapshot
                .normalized
                .chars()
                .all(|ch| ch.is_ascii_alphabetic())
        {
            return Vec::new();
        }
        let input_nfc: String = snapshot.normalized.nfc().collect();
        let folded_input = folded_ascii(&input_nfc);
        let mut matching: Vec<_> = self
            .lexicon
            .entries()
            .filter(|entry| {
                entry.token_nfc != input_nfc
                    && is_supported_lexicon_token(&entry.token_nfc)
                    && folded_ascii(&entry.token_nfc) == folded_input
            })
            .collect();
        let Some(max_frequency) = matching.iter().map(|entry| entry.frequency).max() else {
            return Vec::new();
        };
        let max_frequency = f64::from(max_frequency.max(1));

        let mut scored = matching
            .drain(..)
            .map(|entry| {
                let unigram = f64::from(entry.frequency) / max_frequency;
                let bigram = left_context
                    .prev_token_nfc
                    .as_deref()
                    .and_then(|left| self.lexicon.bigram(left, &entry.token_nfc))
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
                let score = (0.70 + 0.20 * unigram + 0.10 * bigram).clamp(0.0, 1.0);
                (entry.token_nfc.clone(), score)
            })
            .collect::<Vec<_>>();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        scored.truncate(self.top_k);
        scored
            .into_iter()
            .enumerate()
            .map(|(index, (text, base_score))| Candidate {
                id: 4_000_000 + u64::try_from(index).unwrap_or(u64::MAX),
                evidence: format!("diacritics:{input_nfc}->{text}"),
                text,
                source: CandidateSource::Diacritics,
                base_score,
                final_score: 0.0,
            })
            .collect()
    }
}
