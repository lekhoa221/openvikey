//! Pure raw-key reconstruction for misplaced Telex/VNI tone modifiers.

use crate::engine::backend::transform_raw;
use crate::generate::{Generator, LeftContext};
use crate::types::{Candidate, CandidateSource, CompositionSnapshot, InputMethod, TonePlacement};
use unicode_normalization::UnicodeNormalization;

const VIETNAMESE_ONSETS: &[&str] = &[
    "", "b", "c", "ch", "d", "dd", "g", "gh", "gi", "h", "k", "kh", "l", "m", "n", "ng", "ngh",
    "nh", "p", "ph", "q", "qu", "r", "s", "t", "th", "tr", "v", "x",
];
const VIETNAMESE_CODAS: &[&str] = &["", "c", "ch", "m", "n", "ng", "nh", "p", "t"];

/// Reconstructs a syllable when a tone key was pressed before a later vowel.
pub struct TelexFixGenerator {
    method: InputMethod,
    tone_placement: TonePlacement,
}

impl TelexFixGenerator {
    #[must_use]
    pub fn new(method: InputMethod, tone_placement: TonePlacement) -> Self {
        Self {
            method,
            tone_placement,
        }
    }

    fn reconstruct(&self, raw: &str) -> Option<(String, char)> {
        let mut keys: Vec<char> = raw.chars().collect();
        let misplaced_index = keys.iter().enumerate().find_map(|(index, &key)| {
            if index == 0 || !is_tone_key(key, self.method) {
                return None;
            }
            keys[index + 1..]
                .iter()
                .any(|later| is_ascii_vowel(*later))
                .then_some(index)
        })?;
        let tone_key = keys.remove(misplaced_index);
        if !looks_like_vietnamese_syllable(&keys, self.method) {
            return None;
        }
        keys.push(tone_key);
        Some((
            transform_raw(&keys, self.method, self.tone_placement),
            tone_key,
        ))
    }
}

impl Generator for TelexFixGenerator {
    fn source(&self) -> CandidateSource {
        CandidateSource::TelexFix
    }

    fn generate(
        &self,
        snapshot: &CompositionSnapshot,
        _left_context: &LeftContext,
    ) -> Vec<Candidate> {
        let Some((text, tone_key)) = self.reconstruct(&snapshot.raw_keys) else {
            return Vec::new();
        };
        let text_nfc: String = text.nfc().collect();
        if text_nfc == snapshot.normalized || text_nfc == snapshot.raw_keys || text_nfc.is_empty() {
            return Vec::new();
        }
        let method = match self.method {
            InputMethod::Telex => "telex",
            InputMethod::Vni => "vni",
        };
        vec![Candidate {
            id: 2_000_001,
            text: text_nfc,
            source: CandidateSource::TelexFix,
            evidence: format!("{method}-fix:move-tone-{tone_key}"),
            base_score: 0.92,
            final_score: 0.0,
        }]
    }
}

fn is_tone_key(ch: char, method: InputMethod) -> bool {
    match method {
        InputMethod::Telex => matches!(ch.to_ascii_lowercase(), 's' | 'f' | 'r' | 'x' | 'j'),
        InputMethod::Vni => matches!(ch, '1' | '2' | '3' | '4' | '5'),
    }
}

fn is_ascii_vowel(ch: char) -> bool {
    matches!(ch.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u' | 'y')
}

fn looks_like_vietnamese_syllable(keys: &[char], method: InputMethod) -> bool {
    let letters: String = keys
        .iter()
        .copied()
        .filter(|ch| method != InputMethod::Vni || !ch.is_ascii_digit())
        .map(|ch| ch.to_ascii_lowercase())
        .collect();
    if letters.is_empty() || !letters.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return false;
    }
    let Some(first_vowel) = letters.find(is_ascii_vowel) else {
        return false;
    };
    let last_vowel = letters.rfind(is_ascii_vowel).unwrap_or(first_vowel);
    let onset = &letters[..first_vowel];
    let coda = &letters[last_vowel + 1..];
    VIETNAMESE_ONSETS.contains(&onset) && VIETNAMESE_CODAS.contains(&coda)
}
