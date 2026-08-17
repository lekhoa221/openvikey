//! Backend wrapper around vi-rs transform routines.

use crate::types::{InputMethod, TonePlacement};
use vi::methods::{TELEX, VNI, transform_buffer_incremental_with_style};
use vi::processor::AccentStyle;

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
