//! Lab-owned committed text. The engine forgets tokens after `Commit`.

use openvikey_core::types::{Candidate, InputMethod};
use unicode_segmentation::UnicodeSegmentation;

/// One committed token plus the delimiter that followed it.
#[derive(Debug, Clone, PartialEq)]
pub struct CommittedUnit {
    pub remaining_nfc: String,
    pub full_token_nfc: String,
    pub delimiter: Option<char>,
    pub original_nfc: String,
    /// Ephemeral composition keystrokes used to reopen this exact token after deleting a space.
    pub raw_keys: Option<String>,
    pub left_token_nfc: Option<String>,
    pub input_method: InputMethod,
    pub candidates: Vec<Candidate>,
}

impl CommittedUnit {
    #[must_use]
    pub fn new(
        token_nfc: impl Into<String>,
        delimiter: Option<char>,
        original_nfc: impl Into<String>,
        raw_keys: Option<String>,
        left_token_nfc: Option<String>,
        input_method: InputMethod,
        candidates: Vec<Candidate>,
    ) -> Self {
        let token_nfc = token_nfc.into();
        Self {
            remaining_nfc: token_nfc.clone(),
            full_token_nfc: token_nfc,
            delimiter,
            original_nfc: original_nfc.into(),
            raw_keys,
            left_token_nfc,
            input_method,
            candidates,
        }
    }
}

/// Result of popping one grapheme or delimiter from the document.
#[derive(Debug, Clone, PartialEq)]
pub struct PopOutcome {
    pub started_deleting: Option<CommittedUnit>,
    pub removed_delimiter: Option<char>,
}

/// Committed tokens before the caret.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DocumentBuffer {
    units: Vec<CommittedUnit>,
}

impl DocumentBuffer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_commit(&mut self, unit: CommittedUnit) {
        self.units.push(unit);
    }

    #[must_use]
    pub fn rendered(&self) -> String {
        let mut rendered = String::new();
        for unit in &self.units {
            rendered.push_str(&unit.remaining_nfc);
            if let Some(delimiter) = unit.delimiter {
                rendered.push(delimiter);
            }
        }
        rendered
    }

    #[must_use]
    pub fn last(&self) -> Option<&CommittedUnit> {
        self.units.last()
    }

    #[must_use]
    pub fn last_mut(&mut self) -> Option<&mut CommittedUnit> {
        self.units.last_mut()
    }

    /// Left-context token: the last *complete* committed unit with text.
    #[must_use]
    pub fn context_token(&self) -> Option<String> {
        let tokens: Vec<&CommittedUnit> = self
            .units
            .iter()
            .filter(|unit| !unit.full_token_nfc.is_empty())
            .collect();
        match tokens.as_slice() {
            [.., last] if last.delimiter.is_some() && last.remaining_nfc == last.full_token_nfc => {
                Some(last.full_token_nfc.clone())
            }
            [.., prev, _last] => Some(prev.full_token_nfc.clone()),
            [] | [_] => None,
        }
    }

    pub fn replace_last_token(&mut self, new_token: impl Into<String>) {
        if let Some(unit) = self.units.last_mut() {
            let new_token = new_token.into();
            unit.remaining_nfc.clone_from(&new_token);
            unit.full_token_nfc = new_token;
        }
    }

    pub fn pop_last(&mut self) -> Option<CommittedUnit> {
        self.units.pop()
    }

    pub fn pop_grapheme(&mut self) -> Option<PopOutcome> {
        let removed_delimiter = {
            let unit = self.units.last_mut()?;
            unit.delimiter
                .take()
                .map(|delimiter| (delimiter, unit.remaining_nfc.is_empty()))
        };
        if let Some((delimiter, empty_token)) = removed_delimiter {
            if empty_token {
                self.units.pop();
            }
            return Some(PopOutcome {
                started_deleting: None,
                removed_delimiter: Some(delimiter),
            });
        }
        let unit = self.units.last_mut()?;
        let started = (unit.remaining_nfc == unit.full_token_nfc).then(|| unit.clone());
        let mut graphemes: Vec<&str> = unit.remaining_nfc.graphemes(true).collect();
        graphemes.pop();
        unit.remaining_nfc = graphemes.concat();
        let empty = unit.remaining_nfc.is_empty();
        if empty {
            self.units.pop();
        }
        Some(PopOutcome {
            started_deleting: started,
            removed_delimiter: None,
        })
    }
}
