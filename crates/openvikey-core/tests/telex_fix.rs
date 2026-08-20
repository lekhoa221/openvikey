//! Milestone 7A: raw-key Telex/VNI modifier reconstruction.

use openvikey_core::correction::{
    InterventionConfig, boundary_assist_candidate, candidate_rule_key,
};
use openvikey_core::generate::telex_fix::TelexFixGenerator;
use openvikey_core::generate::{Generator, LeftContext};
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use openvikey_core::types::{
    Candidate, CandidateSource, CompositionSnapshot, InputMethod, TonePlacement,
};

fn snapshot(raw: &str, rendered: &str) -> CompositionSnapshot {
    CompositionSnapshot::new(1, raw.to_string(), rendered.to_string())
}

fn assist_applies(
    snapshot: &CompositionSnapshot,
    candidates: &[Candidate],
    delimiter: Option<char>,
    config: InterventionConfig,
    lexicon: &Lexicon,
    allow_transform: bool,
) -> bool {
    boundary_assist_candidate(
        snapshot,
        candidates,
        delimiter,
        config,
        lexicon,
        allow_transform,
    )
    .is_some()
}

#[test]
fn vni_tone_key_before_vowel_reconstructs_chao() {
    let generator = TelexFixGenerator::new(InputMethod::Vni, TonePlacement::Modern);
    let candidates = generator.generate(&snapshot("ch2ao", "ch2ao"), &LeftContext::default());
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].text, "chào");
    assert_eq!(candidates[0].source, CandidateSource::TelexFix);
    assert_eq!(candidates[0].evidence, "vni-fix:move-tone-2");
}

#[test]
fn telex_tone_key_before_vowel_reconstructs_chao() {
    let generator = TelexFixGenerator::new(InputMethod::Telex, TonePlacement::Modern);
    let candidates = generator.generate(&snapshot("chfao", "chfao"), &LeftContext::default());
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].text, "chào");
    assert_eq!(candidates[0].evidence, "telex-fix:move-tone-f");
}

#[test]
fn methods_are_separate_and_valid_input_is_unchanged() {
    let telex = TelexFixGenerator::new(InputMethod::Telex, TonePlacement::Modern);
    let vni = TelexFixGenerator::new(InputMethod::Vni, TonePlacement::Modern);

    assert!(
        telex
            .generate(&snapshot("ch2ao", "ch2ao"), &LeftContext::default())
            .is_empty()
    );
    assert!(
        vni.generate(&snapshot("chfao", "chfao"), &LeftContext::default())
            .is_empty()
    );
    assert!(
        telex
            .generate(&snapshot("chaof", "chào"), &LeftContext::default())
            .is_empty()
    );
    assert!(
        telex
            .generate(&snapshot("craft", "craft"), &LeftContext::default())
            .is_empty()
    );
    assert!(
        vni.generate(&snapshot("chao2", "chào"), &LeftContext::default())
            .is_empty()
    );
}

#[test]
fn generator_respects_classic_tone_placement() {
    let generator = TelexFixGenerator::new(InputMethod::Vni, TonePlacement::Classic);
    let candidates = generator.generate(&snapshot("ho2a", "ho2a"), &LeftContext::default());
    assert_eq!(candidates[0].text, "hòa");
}

fn chao_lexicon() -> Lexicon {
    Lexicon::from_entries(
        [LexiconEntry {
            token_nfc: "chào".to_string(),
            frequency: 10,
        }],
        [],
        Some("telex-fix-policy"),
    )
}

fn telex_candidate(text: &str) -> Candidate {
    Candidate {
        id: 2_000_001,
        text: text.to_string(),
        source: CandidateSource::TelexFix,
        evidence: "vni-fix:move-tone-2".to_string(),
        base_score: 0.92,
        final_score: 0.92,
    }
}

fn fuzzy_candidate(id: u64, text: &str) -> Candidate {
    Candidate {
        id,
        text: text.to_string(),
        source: CandidateSource::Fuzzy,
        evidence: "fuzzy:weighted".to_string(),
        base_score: 0.8,
        final_score: 0.95,
    }
}

#[test]
fn telex_fix_policy_allows_win32_space_and_rejects_electron_punct() {
    let snapshot = snapshot("ch2ao", "ch2ao");
    let candidates = vec![telex_candidate("chào")];
    let lexicon = chao_lexicon();
    assert!(assist_applies(
        &snapshot,
        &candidates,
        Some(' '),
        InterventionConfig::electron(),
        &lexicon,
        true,
    ));
    assert!(!assist_applies(
        &snapshot,
        &candidates,
        Some('.'),
        InterventionConfig::electron(),
        &lexicon,
        true,
    ));
    assert!(assist_applies(
        &snapshot,
        &candidates,
        Some('.'),
        InterventionConfig::win32(),
        &lexicon,
        true,
    ));
    assert!(!assist_applies(
        &snapshot,
        &candidates,
        Some(' '),
        InterventionConfig::default(),
        &lexicon,
        true,
    ));
}

#[test]
fn telex_fix_policy_autos_when_fuzzy_agrees_on_the_same_word() {
    let snapshot = snapshot("ch2ao", "ch2ao");
    let candidates = vec![telex_candidate("chào"), fuzzy_candidate(1, "chào")];
    assert!(assist_applies(
        &snapshot,
        &candidates,
        Some(' '),
        InterventionConfig::win32(),
        &chao_lexicon(),
        true,
    ));
}

fn abbrev_candidate(text: &str) -> Candidate {
    Candidate {
        id: 3_000_001,
        text: text.to_string(),
        source: CandidateSource::Abbreviation,
        evidence: "seed:ko".to_string(),
        base_score: 0.8,
        final_score: 0.8,
    }
}

fn diacritics_candidate(id: u64, text: &str) -> Candidate {
    Candidate {
        id,
        text: text.to_string(),
        source: CandidateSource::Diacritics,
        evidence: "diacritics:unigram".to_string(),
        base_score: 0.7,
        final_score: 0.75,
    }
}

fn khong_lexicon() -> Lexicon {
    Lexicon::from_entries(
        [LexiconEntry {
            token_nfc: "không".to_string(),
            frequency: 100,
        }],
        [],
        Some("boundary-assist"),
    )
}

#[test]
fn boundary_assist_picks_unique_abbrev_on_space() {
    let snapshot = snapshot("ko", "ko");
    let candidates = vec![abbrev_candidate("không")];
    let picked = boundary_assist_candidate(
        &snapshot,
        &candidates,
        Some(' '),
        InterventionConfig::win32(),
        &khong_lexicon(),
        true,
    );
    assert_eq!(
        picked.map(|candidate| candidate.text.as_str()),
        Some("không")
    );
}

#[test]
fn boundary_assist_rejects_diacritics_even_when_unique() {
    let snapshot = snapshot("khong", "khong");
    let candidates = vec![diacritics_candidate(1, "không")];
    assert!(
        boundary_assist_candidate(
            &snapshot,
            &candidates,
            Some(' '),
            InterventionConfig::win32(),
            &khong_lexicon(),
            true,
        )
        .is_none()
    );
}

#[test]
fn boundary_assist_picks_unique_fuzzy_on_space() {
    let snapshot = snapshot("khogn", "khogn");
    let candidates = vec![fuzzy_candidate(1, "không")];
    assert_eq!(
        boundary_assist_candidate(
            &snapshot,
            &candidates,
            Some(' '),
            InterventionConfig::win32(),
            &khong_lexicon(),
            true,
        )
        .map(|candidate| candidate.text.as_str()),
        Some("không")
    );
}

#[test]
fn boundary_assist_rejects_fuzzy_when_other_sources_disagree() {
    let snapshot = snapshot("ntn", "ntn");
    let lexicon = Lexicon::from_entries(
        [LexiconEntry {
            token_nfc: "nên".to_string(),
            frequency: 50,
        }],
        [],
        Some("fuzzy-guess"),
    );
    let candidates = vec![fuzzy_candidate(1, "nên"), abbrev_candidate("như thế nào")];
    assert!(
        boundary_assist_candidate(
            &snapshot,
            &candidates,
            Some(' '),
            InterventionConfig::win32(),
            &lexicon,
            true,
        )
        .is_none()
    );
}

#[test]
fn boundary_assist_rejects_short_unique_fuzzy() {
    let snapshot = snapshot("ntn", "ntn");
    let lexicon = Lexicon::from_entries(
        [LexiconEntry {
            token_nfc: "nên".to_string(),
            frequency: 50,
        }],
        [],
        Some("short-fuzzy"),
    );
    let candidates = vec![fuzzy_candidate(1, "nên")];
    assert!(
        boundary_assist_candidate(
            &snapshot,
            &candidates,
            Some(' '),
            InterventionConfig::win32(),
            &lexicon,
            true,
        )
        .is_none()
    );
}

#[test]
fn boundary_assist_rejects_multiword_abbrev() {
    let snapshot = snapshot("ntn", "ntn");
    let lexicon = Lexicon::from_entries(
        [LexiconEntry {
            token_nfc: "như thế nào".to_string(),
            frequency: 10,
        }],
        [],
        Some("phrase-abbrev"),
    );
    let candidates = vec![abbrev_candidate("như thế nào")];
    assert!(
        boundary_assist_candidate(
            &snapshot,
            &candidates,
            Some(' '),
            InterventionConfig::win32(),
            &lexicon,
            true,
        )
        .is_none()
    );
}

#[test]
fn telex_fix_policy_autos_when_unique_vni_fix_is_not_ranked_first() {
    let snapshot = snapshot("ch2ao", "ch2ao");
    let candidates = vec![fuzzy_candidate(1, "cho"), telex_candidate("chào")];
    assert!(assist_applies(
        &snapshot,
        &candidates,
        Some(' '),
        InterventionConfig::win32(),
        &chao_lexicon(),
        true,
    ));
}

#[test]
fn telex_fix_rule_keeps_telex_source_after_nfc_merge() {
    let snapshot = snapshot("ch2ao", "ch2ao");
    let merged = Candidate {
        id: 1,
        text: "chào".to_string(),
        source: CandidateSource::Diacritics,
        evidence: "diacritics:unigram+vni-fix:move-tone-2".to_string(),
        base_score: 0.9,
        final_score: 0.9,
    };
    let key = candidate_rule_key(
        &snapshot,
        &LeftContext::default(),
        InputMethod::Vni,
        &merged,
    );
    assert_eq!(key.source, CandidateSource::TelexFix);
    assert_eq!(key.source_rule_id, "vni-fix:move-tone-2");
    assert_eq!(key.original_nfc, "ch2ao");
    assert_eq!(key.candidate_nfc, "chào");
}
