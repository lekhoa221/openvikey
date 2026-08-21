//! Observe-only aggregate statistics for trusted typing-error corrections.

use crate::generate::vietnamese::folded_ascii;
use crate::types::InputMethod;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

/// Bounded operation vocabulary recorded for offline inspection only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorOperationClass {
    Transpose,
    AdjacentKey,
    ExtraKey,
    EarlyTone,
}

/// Aggregate operation counts. These values never enter ranking or intervention planning.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneralizedErrorModel {
    counts: BTreeMap<ErrorOperationClass, u64>,
}

impl GeneralizedErrorModel {
    /// Records one trusted correction when it has one recognized operation class.
    pub fn observe(
        &mut self,
        original: &str,
        replacement: &str,
        input_method: InputMethod,
    ) -> Option<ErrorOperationClass> {
        let operation = classify_operation(original, replacement, input_method)?;
        let count = self.counts.entry(operation).or_default();
        *count = count.saturating_add(1);
        Some(operation)
    }

    #[must_use]
    pub fn count(&self, operation: ErrorOperationClass) -> u64 {
        self.counts.get(&operation).copied().unwrap_or(0)
    }

    #[must_use]
    pub fn total_observations(&self) -> u64 {
        self.counts
            .values()
            .copied()
            .fold(0_u64, u64::saturating_add)
    }
}

fn classify_operation(
    original: &str,
    replacement: &str,
    input_method: InputMethod,
) -> Option<ErrorOperationClass> {
    if is_early_tone(original, replacement, input_method) {
        return Some(ErrorOperationClass::EarlyTone);
    }
    let original = folded_ascii(original);
    let replacement = folded_ascii(replacement);
    if is_single_transposition(&original, &replacement) {
        Some(ErrorOperationClass::Transpose)
    } else if is_single_adjacent_substitution(&original, &replacement) {
        Some(ErrorOperationClass::AdjacentKey)
    } else if has_single_extra_key(&original, &replacement) {
        Some(ErrorOperationClass::ExtraKey)
    } else {
        None
    }
}

fn is_single_transposition(original: &str, replacement: &str) -> bool {
    let original: Vec<char> = original.chars().collect();
    let replacement: Vec<char> = replacement.chars().collect();
    if original.len() != replacement.len() || original.len() < 2 {
        return false;
    }
    let differences: Vec<usize> = original
        .iter()
        .zip(&replacement)
        .enumerate()
        .filter_map(|(index, (left, right))| (left != right).then_some(index))
        .collect();
    matches!(differences.as_slice(), [left, right]
        if *right == left.saturating_add(1)
            && original[*left] == replacement[*right]
            && original[*right] == replacement[*left])
}

fn is_single_adjacent_substitution(original: &str, replacement: &str) -> bool {
    let mut differences = original
        .chars()
        .zip(replacement.chars())
        .filter(|(left, right)| left != right);
    let Some((left, right)) = differences.next() else {
        return false;
    };
    original.chars().count() == replacement.chars().count()
        && differences.next().is_none()
        && keyboard_adjacent(left, right)
}

fn has_single_extra_key(original: &str, replacement: &str) -> bool {
    let original: Vec<char> = original.chars().collect();
    let replacement: Vec<char> = replacement.chars().collect();
    original.len() == replacement.len().saturating_add(1)
        && (0..original.len()).any(|skip| {
            original
                .iter()
                .enumerate()
                .filter_map(|(index, ch)| (index != skip).then_some(*ch))
                .eq(replacement.iter().copied())
        })
}

fn is_early_tone(original: &str, replacement: &str, input_method: InputMethod) -> bool {
    let keys: Vec<char> = original.chars().collect();
    let replacement_folded = folded_ascii(replacement);
    let mut matching_positions = 0_u8;
    for (index, key) in keys.iter().copied().enumerate().skip(1) {
        let Some(mark) = tone_mark(key, input_method) else {
            continue;
        };
        if !replacement.nfd().any(|ch| ch == mark)
            || !keys[index.saturating_add(1)..]
                .iter()
                .copied()
                .any(is_ascii_vowel)
        {
            continue;
        }
        let without_tone: String = keys
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(candidate_index, ch)| (candidate_index != index).then_some(ch))
            .collect();
        if folded_ascii(&without_tone) == replacement_folded {
            matching_positions = matching_positions.saturating_add(1);
        }
    }
    matching_positions == 1
}

fn tone_mark(key: char, input_method: InputMethod) -> Option<char> {
    match (input_method, key.to_ascii_lowercase()) {
        (InputMethod::Telex, 's') | (InputMethod::Vni, '1') => Some('\u{0301}'),
        (InputMethod::Telex, 'f') | (InputMethod::Vni, '2') => Some('\u{0300}'),
        (InputMethod::Telex, 'r') | (InputMethod::Vni, '3') => Some('\u{0309}'),
        (InputMethod::Telex, 'x') | (InputMethod::Vni, '4') => Some('\u{0303}'),
        (InputMethod::Telex, 'j') | (InputMethod::Vni, '5') => Some('\u{0323}'),
        _ => None,
    }
}

fn is_ascii_vowel(ch: char) -> bool {
    matches!(ch.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u' | 'y')
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
