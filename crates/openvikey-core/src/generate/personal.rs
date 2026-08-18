//! Table-driven personal corrections. No model access at generate time.

use crate::generate::{Generator, LeftContext};
use crate::types::{Candidate, CandidateSource, CompositionSnapshot, InputMethod};
use unicode_normalization::UnicodeNormalization;

const PERSONAL_ID_BASE: u64 = 5_000_000;

/// Promoted user-taught original → replacement rows, injected as a pure table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersonalGenerator {
    rows: Vec<(String, String)>,
}

impl PersonalGenerator {
    #[must_use]
    pub fn for_method(method: InputMethod, promoted: &[(InputMethod, String, String)]) -> Self {
        Self {
            rows: promoted
                .iter()
                .filter(|(row_method, _, _)| *row_method == method)
                .map(|(_, original, replacement)| (nfc(original), replacement.clone()))
                .collect(),
        }
    }
}

impl Generator for PersonalGenerator {
    fn source(&self) -> CandidateSource {
        CandidateSource::Personal
    }

    fn generate(
        &self,
        snapshot: &CompositionSnapshot,
        _left_context: &LeftContext,
    ) -> Vec<Candidate> {
        let key = nfc(&snapshot.normalized);
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, (original, _))| original == &key)
            .map(|(idx, (_, replacement))| Candidate {
                id: PERSONAL_ID_BASE.saturating_add(u64::try_from(idx).unwrap_or(u64::MAX)),
                text: replacement.clone(),
                source: CandidateSource::Personal,
                evidence: format!("personal:{key}"),
                base_score: 0.88,
                final_score: 0.0,
            })
            .collect()
    }
}

fn nfc(text: &str) -> String {
    text.nfc().collect()
}
