//! Milestone 7A: raw-key Telex/VNI modifier reconstruction.

use openvikey_core::generate::telex_fix::TelexFixGenerator;
use openvikey_core::generate::{Generator, LeftContext};
use openvikey_core::types::{CandidateSource, CompositionSnapshot, InputMethod, TonePlacement};

fn snapshot(raw: &str, rendered: &str) -> CompositionSnapshot {
    CompositionSnapshot::new(1, raw.to_string(), rendered.to_string())
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
