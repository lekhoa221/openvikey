//! Milestone 7B: bounded weighted fuzzy correction over the authored lexicon.

use openvikey_core::generate::fuzzy::FuzzyGenerator;
use openvikey_core::generate::{Generator, LeftContext};
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use openvikey_core::types::{CandidateSource, CompositionSnapshot};

fn lexicon(words: &[(&str, u32)]) -> Lexicon {
    Lexicon::from_entries(
        words.iter().map(|(token, frequency)| LexiconEntry {
            token_nfc: (*token).to_string(),
            frequency: *frequency,
        }),
        [],
        Some("fuzzy-test"),
    )
}

fn generated<'a>(generator: &'a FuzzyGenerator<'a>, input: &str) -> Vec<String> {
    generator
        .generate(
            &CompositionSnapshot::new(1, input.to_string(), input.to_string()),
            &LeftContext::default(),
        )
        .into_iter()
        .map(|candidate| candidate.text)
        .collect()
}

#[test]
fn weighted_transposition_adjacency_and_duplicate_key_find_words() {
    let lexicon = lexicon(&[("không", 100), ("thật", 90), ("bạn", 80)]);
    let generator = FuzzyGenerator::new(&lexicon, 5);

    assert_eq!(generated(&generator, "khọgn")[0], "không");
    assert_eq!(generated(&generator, "taht")[0], "thật");
    assert_eq!(generated(&generator, "bam")[0], "bạn");
    assert_eq!(generated(&generator, "khôong")[0], "không");
}

#[test]
fn mixed_vni_digits_and_typos_from_real_input_are_recovered() {
    let lexicon = lexicon(&[
        ("mẫu", 100),
        ("phát", 90),
        ("dạng", 80),
        ("hợp", 70),
        ("thống", 60),
        ("được", 50),
        ("prompt", 40),
        ("bạn", 30),
        ("biết", 20),
        ("nó", 10),
        ("không", 10),
        ("này", 10),
    ]);
    let generator = FuzzyGenerator::new(&lexicon, 5);

    for (input, expected) in [
        ("ma674u", "mẫu"),
        ("paht1", "phát"),
        ("dnag5", "dạng"),
        ("hiop75", "hợp"),
        ("htong61", "thống"),
        ("đưcọ", "được"),
        ("proimtp", "prompt"),
        ("bab5", "bạn"),
        ("nbiet61", "biết"),
        ("n1o", "nó"),
        ("kh6oing", "không"),
        ("nah2y", "này"),
    ] {
        assert!(
            generated(&generator, input)
                .iter()
                .any(|candidate| candidate == expected),
            "missing {input} -> {expected}"
        );
    }
}

#[test]
fn candidates_are_valid_lexicon_syllables_stable_and_bounded() {
    let lexicon = lexicon(&[
        ("bàn", 100),
        ("bán", 90),
        ("bạn", 80),
        ("ban", 70),
        ("xyz", 1_000),
    ]);
    let generator = FuzzyGenerator::new(&lexicon, 2);
    let first = generator.generate(
        &CompositionSnapshot::new(1, "bqn5".to_string(), "bqn5".to_string()),
        &LeftContext::default(),
    );
    let second = generator.generate(
        &CompositionSnapshot::new(1, "bqn5".to_string(), "bqn5".to_string()),
        &LeftContext::default(),
    );

    assert_eq!(first, second);
    assert_eq!(first.len(), 2);
    assert!(
        first
            .iter()
            .all(|candidate| candidate.source == CandidateSource::Fuzzy)
    );
    assert!(first.iter().all(|candidate| candidate.text != "xyz"));
}

#[test]
fn valid_lexicon_input_and_plain_unaccented_ambiguity_are_unchanged() {
    let lexicon = lexicon(&[("không", 100), ("bàn", 90), ("bạn", 80)]);
    let generator = FuzzyGenerator::new(&lexicon, 5);

    assert!(generated(&generator, "không").is_empty());
    assert!(generated(&generator, "ban").is_empty());
}
