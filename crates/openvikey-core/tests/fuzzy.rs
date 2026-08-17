//! Milestone 7B: bounded weighted fuzzy correction over the authored lexicon.

use openvikey_core::generate::fuzzy::FuzzyGenerator;
use openvikey_core::generate::{Generator, LeftContext};
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use openvikey_core::types::{CandidateSource, CompositionSnapshot};

fn lexicon(words: &[(&str, u32)]) -> Lexicon {
    lexicon_with_bigrams(words, &[])
}

fn lexicon_with_bigrams(words: &[(&str, u32)], bigrams: &[(&str, &str, f64)]) -> Lexicon {
    Lexicon::from_entries(
        words.iter().map(|(token, frequency)| LexiconEntry {
            token_nfc: (*token).to_string(),
            frequency: *frequency,
        }),
        bigrams
            .iter()
            .map(|(left, token, score)| (((*left).to_string(), (*token).to_string()), *score)),
        Some("fuzzy-test"),
    )
}

fn generated<'a>(generator: &'a FuzzyGenerator<'a>, input: &str) -> Vec<String> {
    generated_with_left(generator, input, None)
}

fn generated_with_left<'a>(
    generator: &'a FuzzyGenerator<'a>,
    input: &str,
    left: Option<&str>,
) -> Vec<String> {
    generator
        .generate(
            &CompositionSnapshot::new(1, input.to_string(), input.to_string()),
            &LeftContext {
                prev_token_nfc: left.map(str::to_string),
            },
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

    let duplicate = generator.generate(
        &CompositionSnapshot::new(1, "khôong".to_string(), "khôong".to_string()),
        &LeftContext::default(),
    )[0]
    .base_score;
    let ordinary_insertion = generator.generate(
        &CompositionSnapshot::new(1, "khôg".to_string(), "khôg".to_string()),
        &LeftContext::default(),
    )[0]
    .base_score;
    assert!(
        duplicate > ordinary_insertion,
        "duplicate-key deletion must cost less than an ordinary insertion"
    );
}

#[test]
fn mixed_vni_digits_and_typos_from_real_input_are_recovered() {
    let lexicon = lexicon_with_bigrams(
        &[
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
            ("chữ", 10),
            ("hẳn", 10),
            ("là", 10),
            ("gõ", 10),
        ],
        &[("hẳn", "là", 1.0)],
    );
    let generator = FuzzyGenerator::new(&lexicon, 5);

    for (input, expected, left) in [
        ("ma674u", "mẫu", None),
        ("paht1", "phát", None),
        ("dnag5", "dạng", None),
        ("hiop75", "hợp", None),
        ("htong61", "thống", None),
        ("đưcọ", "được", None),
        ("proimtp", "prompt", None),
        ("bab5", "bạn", None),
        ("nbiet61", "biết", None),
        ("n1o", "nó", None),
        ("kh6oing", "không", None),
        ("nah2y", "này", None),
        ("chũ", "chữ", None),
        ("hẵ", "hẳn", None),
        ("nal2", "là", Some("hẳn")),
        ("go4", "gõ", None),
    ] {
        assert_eq!(
            generated_with_left(&generator, input, left)
                .first()
                .map(String::as_str),
            Some(expected),
            "wrong top candidate for {input}"
        );
    }
}

#[test]
fn vni_tone_and_modifier_digits_outrank_unigram_frequency() {
    let lexicon = lexicon(&[("bàn", 1_000), ("bạn", 1), ("hộp", 1_000), ("hợp", 1)]);
    let generator = FuzzyGenerator::new(&lexicon, 5);

    assert_eq!(generated(&generator, "ban5")[0], "bạn");
    assert_eq!(generated(&generator, "hiop75")[0], "hợp");
}

#[test]
fn candidates_are_valid_lexicon_syllables_stable_and_bounded() {
    let lexicon = lexicon(&[
        ("bàn", 100),
        ("bán", 90),
        ("bạn", 80),
        ("ban", 70),
        ("xyz", 1_000),
        ("káe", 2_000),
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
    assert_eq!(
        first
            .iter()
            .map(|candidate| (
                candidate.id,
                candidate.text.as_str(),
                candidate.evidence.as_str()
            ))
            .collect::<Vec<_>>(),
        vec![
            (3_000_000, "bạn", "fuzzy:weighted:bqn5->bạn"),
            (3_000_001, "bàn", "fuzzy:weighted:bqn5->bàn"),
        ]
    );
    assert!(
        first
            .iter()
            .all(|candidate| candidate.source == CandidateSource::Fuzzy)
    );
    assert!(
        first
            .iter()
            .all(|candidate| candidate.text != "xyz" && candidate.text != "káe")
    );
    assert!(
        generated(&generator, "kae1")
            .iter()
            .all(|candidate| candidate != "káe"),
        "accented outputs must satisfy Vietnamese syllable grammar"
    );
}

#[test]
fn valid_lexicon_input_and_plain_unaccented_ambiguity_are_unchanged() {
    let lexicon = lexicon(&[("không", 100), ("bàn", 90), ("bạn", 80)]);
    let generator = FuzzyGenerator::new(&lexicon, 5);

    assert!(generated(&generator, "không").is_empty());
    assert!(generated(&generator, "ban").is_empty());
}
