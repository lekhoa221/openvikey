use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

const ONSETS: &[&str] = &[
    "ngh", "ch", "gh", "gi", "kh", "ng", "nh", "ph", "qu", "th", "tr", "b", "c", "d", "g", "h",
    "k", "l", "m", "n", "p", "q", "r", "s", "t", "v", "x", "",
];
const CODAS: &[&str] = &["", "c", "ch", "m", "n", "ng", "nh", "p", "t"];

pub(crate) fn folded_ascii(text: &str) -> String {
    text.nfd()
        .filter(|ch| !is_combining_mark(*ch))
        .flat_map(char::to_lowercase)
        .map(|ch| match ch {
            'đ' => 'd',
            other => other,
        })
        .collect()
}

pub(crate) fn vni_features(text: &str) -> [bool; 10] {
    let mut features = [false; 10];
    for ch in text.nfd() {
        match ch {
            '\u{0301}' => features[1] = true, // acute
            '\u{0300}' => features[2] = true, // grave
            '\u{0309}' => features[3] = true, // hook above
            '\u{0303}' => features[4] = true, // tilde
            '\u{0323}' => features[5] = true, // dot below
            '\u{0302}' => features[6] = true, // circumflex
            '\u{031b}' => features[7] = true, // horn
            '\u{0306}' => features[8] = true, // breve
            'đ' | 'Đ' => features[9] = true,
            _ => {}
        }
    }
    features
}

pub(crate) fn is_supported_lexicon_token(token: &str) -> bool {
    let folded = folded_ascii(token);
    if folded.is_empty() || !folded.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return false;
    }
    if is_vietnamese_syllable(&folded) {
        return true;
    }
    // Explicit lexicon entries may be common Latin words in mixed Vietnamese text.
    folded
        .chars()
        .any(|ch| matches!(ch, 'a' | 'e' | 'i' | 'o' | 'u'))
}

fn is_vietnamese_syllable(folded: &str) -> bool {
    ONSETS.iter().any(|onset| {
        let Some(remainder) = folded.strip_prefix(onset) else {
            return false;
        };
        let chars: Vec<char> = remainder.chars().collect();
        let Some(first_vowel) = chars.iter().position(|ch| is_vowel(*ch)) else {
            return false;
        };
        if first_vowel != 0 {
            return false;
        }
        let last_vowel = chars
            .iter()
            .rposition(|ch| is_vowel(*ch))
            .unwrap_or(first_vowel);
        if !chars[first_vowel..=last_vowel]
            .iter()
            .all(|ch| is_vowel(*ch))
        {
            return false;
        }
        let coda: String = chars[last_vowel + 1..].iter().collect();
        CODAS.contains(&coda.as_str())
    })
}

fn is_vowel(ch: char) -> bool {
    matches!(ch, 'a' | 'e' | 'i' | 'o' | 'u' | 'y')
}
