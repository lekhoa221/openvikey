//! Backend wrapper around vi-rs transform routines.

use crate::types::{InputMethod, TonePlacement};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;
use unicode_segmentation::UnicodeSegmentation;
use vi::methods::{TELEX, VNI, transform_buffer_incremental_with_style};
use vi::processor::AccentStyle;

const MAX_RAW_KEYS_PER_VISIBLE_GRAPHEME: usize = 4;

/// Transforms an unparsed slice of characters into rendered Vietnamese text.
#[must_use]
pub fn transform_raw(raw_keys: &[char], method: InputMethod, tone: TonePlacement) -> String {
    if raw_keys.is_empty() {
        return String::new();
    }

    let definition = match method {
        InputMethod::Telex => &TELEX,
        InputMethod::Vni => &VNI,
    };

    let accent_style = match tone {
        TonePlacement::Modern => AccentStyle::New,
        TonePlacement::Classic => AccentStyle::Old,
    };

    let mut buffer = transform_buffer_incremental_with_style(definition, accent_style);
    for &ch in raw_keys {
        buffer.push(ch);
    }
    buffer.view().to_string()
}

/// Remove one rendered extended grapheme and retain the best replayable raw history.
pub(super) fn backspace_visible_grapheme(
    raw_keys: &[char],
    rendered: &str,
    method: InputMethod,
    tone: TonePlacement,
) -> (Vec<char>, String) {
    let target_end = rendered
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(index, _)| index);
    let target = rendered[..target_end].to_owned();
    if target.is_empty() {
        return (Vec::new(), target);
    }

    let max_removed = raw_keys.len().min(MAX_RAW_KEYS_PER_VISIBLE_GRAPHEME);
    let mut best_folded = None;
    for remove_count in 1..=max_removed {
        let mut removed = Vec::with_capacity(remove_count);
        let mut exact = None;
        let mut folded = None;
        search_removed_key_sets(
            raw_keys,
            raw_keys.len(),
            remove_count,
            &mut removed,
            method,
            tone,
            &target,
            &mut exact,
            &mut folded,
        );
        if let Some(candidate) = exact {
            return (candidate, target);
        }
        if best_folded.is_none() {
            best_folded = folded;
        }
    }
    if let Some(candidate) = best_folded {
        return (candidate, target);
    }

    // Defensive fallback for malformed or unusually long raw input. Preserve the
    // visible contract even when vi-rs has no replayable representation.
    let mut candidate = raw_keys.to_vec();
    let target_graphemes = target.graphemes(true).count();
    while !candidate.is_empty() {
        candidate.pop();
        let replayed = transform_raw(&candidate, method, tone);
        if replayed.graphemes(true).count() <= target_graphemes {
            break;
        }
    }
    (candidate, target)
}

#[allow(clippy::too_many_arguments)]
fn search_removed_key_sets(
    raw_keys: &[char],
    next_exclusive: usize,
    remaining: usize,
    removed: &mut Vec<usize>,
    method: InputMethod,
    tone: TonePlacement,
    target: &str,
    exact: &mut Option<Vec<char>>,
    folded: &mut Option<Vec<char>>,
) {
    if exact.is_some() {
        return;
    }
    if remaining == 0 {
        let candidate: Vec<char> = raw_keys
            .iter()
            .enumerate()
            .filter_map(|(index, ch)| (!removed.contains(&index)).then_some(*ch))
            .collect();
        let replayed = transform_raw(&candidate, method, tone);
        if canonically_equal(&replayed, target) {
            *exact = Some(candidate);
        } else if folded.is_none()
            && replayed.graphemes(true).count() == target.graphemes(true).count()
            && fold_visible(&replayed) == fold_visible(target)
        {
            *folded = Some(candidate);
        }
        return;
    }
    if next_exclusive < remaining {
        return;
    }

    for index in ((remaining - 1)..next_exclusive).rev() {
        removed.push(index);
        search_removed_key_sets(
            raw_keys,
            index,
            remaining - 1,
            removed,
            method,
            tone,
            target,
            exact,
            folded,
        );
        removed.pop();
        if exact.is_some() {
            return;
        }
    }
}

fn canonically_equal(left: &str, right: &str) -> bool {
    left.nfc().eq(right.nfc())
}

fn fold_visible(text: &str) -> String {
    text.nfd()
        .filter(|ch| !is_combining_mark(*ch))
        .flat_map(char::to_lowercase)
        .map(|ch| if ch == 'đ' { 'd' } else { ch })
        .collect()
}

/// Checks if a character acts as a word boundary / delimiter.
#[must_use]
pub fn is_boundary_char(ch: char) -> bool {
    ch.is_whitespace() || is_punctuation_or_symbol(ch)
}

fn is_punctuation_or_symbol(ch: char) -> bool {
    matches!(
        ch,
        ' ' | '\t'
            | '\n'
            | '\r'
            | '.'
            | ','
            | '!'
            | '?'
            | ';'
            | ':'
            | '"'
            | '\''
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '<'
            | '>'
            | '/'
            | '\\'
            | '|'
            | '@'
            | '#'
            | '$'
            | '%'
            | '^'
            | '&'
            | '*'
            | '-'
            | '_'
            | '+'
            | '='
            | '~'
            | '`'
    )
}
