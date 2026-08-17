//! Bounded weighted edit-distance candidates over the local lexicon.

use crate::generate::vietnamese::{folded_ascii, is_supported_lexicon_token};
use crate::generate::{Generator, LeftContext};
use crate::lexicon::Lexicon;
use crate::types::{Candidate, CandidateSource, CompositionSnapshot};
use unicode_normalization::UnicodeNormalization;

const MAX_WEIGHTED_DISTANCE: f64 = 1.20;

pub struct FuzzyGenerator<'a> {
    lexicon: &'a Lexicon,
    max_candidates: usize,
}

impl<'a> FuzzyGenerator<'a> {
    #[must_use]
    pub const fn new(lexicon: &'a Lexicon, max_candidates: usize) -> Self {
        Self {
            lexicon,
            max_candidates,
        }
    }
}

impl Generator for FuzzyGenerator<'_> {
    fn source(&self) -> CandidateSource {
        CandidateSource::Fuzzy
    }

    fn generate(
        &self,
        snapshot: &CompositionSnapshot,
        _left_context: &LeftContext,
    ) -> Vec<Candidate> {
        if self.max_candidates == 0
            || snapshot.normalized.chars().any(char::is_whitespace)
            || self.lexicon.contains(&snapshot.normalized)
        {
            return Vec::new();
        }

        let input_nfc: String = snapshot.normalized.nfc().collect();
        let input_folded = letters_only(&input_nfc);
        if input_folded.is_empty() {
            return Vec::new();
        }
        let has_vni_digits = snapshot.raw_keys.chars().any(|ch| ch.is_ascii_digit());
        let plain_unaccented = input_nfc
            .chars()
            .all(|ch| ch.is_ascii_alphabetic() || ch.is_ascii_digit());
        let mut matches = Vec::new();

        for entry in self.lexicon.entries() {
            if !is_supported_lexicon_token(&entry.token_nfc) || entry.token_nfc == input_nfc {
                continue;
            }
            let target_folded = folded_ascii(&entry.token_nfc);
            let length_delta = input_folded.len().abs_diff(target_folded.len());
            if length_delta > 2 {
                continue;
            }
            let mut distance = weighted_distance(&input_folded, &target_folded);
            if distance == 0.0 {
                if plain_unaccented && !has_vni_digits {
                    continue;
                }
                distance = 0.20;
            }
            if distance > MAX_WEIGHTED_DISTANCE {
                continue;
            }
            let frequency_bonus = f64::from(entry.frequency.min(1_000)) / 50_000.0;
            let score = (0.97 - 0.18 * distance + frequency_bonus).clamp(0.0, 1.0);
            matches.push((entry.token_nfc.clone(), score));
        }

        matches.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        matches.truncate(self.max_candidates);
        matches
            .into_iter()
            .enumerate()
            .map(|(index, (text, base_score))| Candidate {
                id: 3_000_000 + u64::try_from(index).unwrap_or(u64::MAX),
                evidence: format!("fuzzy:weighted:{input_nfc}->{text}"),
                text,
                source: CandidateSource::Fuzzy,
                base_score,
                final_score: 0.0,
            })
            .collect()
    }
}

fn letters_only(text: &str) -> String {
    folded_ascii(text)
        .chars()
        .filter(char::is_ascii_alphabetic)
        .collect()
}

fn weighted_distance(input: &str, target: &str) -> f64 {
    let source: Vec<char> = input.chars().collect();
    let target: Vec<char> = target.chars().collect();
    let mut distance: Vec<Vec<f64>> = vec![vec![0.0; target.len() + 1]; source.len() + 1];
    let mut accumulated = 0.0;
    for row in &mut distance {
        row[0] = accumulated;
        accumulated += 0.75;
    }
    accumulated = 0.0;
    for cell in &mut distance[0] {
        *cell = accumulated;
        accumulated += 0.75;
    }

    for source_index in 1..=source.len() {
        for target_index in 1..=target.len() {
            let substitution = if source[source_index - 1] == target[target_index - 1] {
                0.0
            } else if keyboard_adjacent(source[source_index - 1], target[target_index - 1]) {
                0.45
            } else {
                1.0
            };
            let mut best = (distance[source_index - 1][target_index] + 0.75)
                .min(distance[source_index][target_index - 1] + 0.75)
                .min(distance[source_index - 1][target_index - 1] + substitution);
            if source_index > 1
                && target_index > 1
                && source[source_index - 1] == target[target_index - 2]
                && source[source_index - 2] == target[target_index - 1]
            {
                best = best.min(distance[source_index - 2][target_index - 2] + 0.35);
            }
            distance[source_index][target_index] = best;
        }
    }
    distance[source.len()][target.len()]
}

fn keyboard_adjacent(left: char, right: char) -> bool {
    const ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
    let position = |needle| {
        ROWS.iter().enumerate().find_map(|(row, keys)| {
            keys.chars()
                .position(|key| key == needle)
                .map(|column| (row, column))
        })
    };
    let (Some((left_row, left_column)), Some((right_row, right_column))) =
        (position(left), position(right))
    else {
        return false;
    };
    left_row.abs_diff(right_row) <= 1 && left_column.abs_diff(right_column) <= 1
}
