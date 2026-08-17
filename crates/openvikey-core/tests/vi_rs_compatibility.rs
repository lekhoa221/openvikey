//! Milestone 2: vi-rs compatibility gate.

use std::time::Instant;
use vi::methods::{
    TELEX, VNI, transform_buffer, transform_buffer_incremental,
    transform_buffer_incremental_with_style,
};
use vi::processor::AccentStyle;

fn telex(raw: &str) -> String {
    let mut out = String::new();
    transform_buffer(&TELEX, raw.chars(), &mut out);
    out
}

fn vni(raw: &str) -> String {
    let mut out = String::new();
    transform_buffer(&VNI, raw.chars(), &mut out);
    out
}

fn telex_incremental(raw: &str) -> vi::methods::IncrementalBuffer<'static> {
    let mut buf = transform_buffer_incremental(&TELEX);
    for ch in raw.chars() {
        buf.push(ch);
    }
    buf
}

#[test]
fn test_telex_and_vni_basic_transformations() {
    let telex_cases = [
        ("vieetj", "việt"),
        ("nam", "nam"),
        ("dduowngf", "đường"),
        ("hoas", "hoá"),
        ("ddaf", "đà"),
        ("tieengs", "tiếng"),
    ];
    for (raw, expected) in telex_cases {
        assert_eq!(telex(raw), expected, "Telex mismatch for '{raw}'");
    }

    let vni_cases = [
        ("vie6t5", "việt"),
        ("nam", "nam"),
        ("d9u7o7ng2", "đường"),
        ("hoa1", "hoá"),
        ("d9a2", "đà"),
        ("tie6ng1", "tiếng"),
    ];
    for (raw, expected) in vni_cases {
        assert_eq!(vni(raw), expected, "VNI mismatch for '{raw}'");
    }
}

#[test]
fn test_tone_placement_modern_vs_classic() {
    let mut buf_new = transform_buffer_incremental_with_style(&TELEX, AccentStyle::New);
    for ch in "hoas".chars() {
        buf_new.push(ch);
    }
    assert_eq!(buf_new.view(), "hoá");

    let mut buf_new_oa = transform_buffer_incremental_with_style(&TELEX, AccentStyle::New);
    for ch in "oaf".chars() {
        buf_new_oa.push(ch);
    }
    assert_eq!(buf_new_oa.view(), "oà");

    let mut buf_old = transform_buffer_incremental_with_style(&TELEX, AccentStyle::Old);
    for ch in "hoas".chars() {
        buf_old.push(ch);
    }
    assert_eq!(buf_old.view(), "hóa");

    let mut buf_old_oa = transform_buffer_incremental_with_style(&TELEX, AccentStyle::Old);
    for ch in "oaf".chars() {
        buf_old_oa.push(ch);
    }
    assert_eq!(buf_old_oa.view(), "òa");
}

#[test]
fn test_casing_behavior() {
    let cases = [
        ("Vieetj", "Việt"),
        ("VIEETJ", "VIỆT"),
        ("Dd", "Đ"),
        ("DD", "Đ"),
    ];
    for (raw, expected) in cases {
        assert_eq!(telex(raw), expected, "Casing mismatch for '{raw}'");
    }
}

#[test]
fn test_escape_reset_and_restore() {
    // Duplicate modifier restores the raw pair (vi-rs testdata/simple_telex).
    assert_eq!(telex("ass"), "as");
    assert_eq!(telex("aaa"), "aa");
    assert_eq!(telex("ww"), "w");
    assert_eq!(telex("w"), "ư");
    // z removes the tone mark.
    assert_eq!(telex("hafz"), "ha");
    // Switching tone keys replaces the mark.
    assert_eq!(telex("toanfs"), "toán");
    // Repeating the same tone key strips the mark and keeps the extra letter.
    assert_eq!(telex("vieetjj"), "viêtj");
}

#[test]
fn test_invalid_syllable_passthrough() {
    // From vi-rs testdata: a cluster with no valid Vietnamese syllable stays raw.
    assert_eq!(telex("zzzjjjjhhhkkk"), "zzzjjjjhhhkkk");
    assert_eq!(telex("kkk"), "kkk");
}

#[test]
fn test_english_and_url_actual_vi_rs_behavior() {
    // vi-rs is a syllable transformer, not an IME policy layer.
    // English-like tokens with tone keys are transformed (`case` + s).
    assert_eq!(telex("case"), "cáe");
    assert_eq!(telex("casse"), "case");
    assert_eq!(telex("simple"), "simple");
    assert_eq!(telex("facebook"), "facebook");

    // Full URL / code tokens — pin observed vi-rs output (wrapper owns passthrough).
    assert_eq!(telex("https://example.com"), "https://example.com");
    assert_eq!(telex("github.com"), "github.com");
    assert_eq!(telex("foo->bar"), "foo->bar");
}

#[test]
fn test_raw_keys_preserved_alongside_rendered() {
    let raw = "dduowngf";
    let buf = telex_incremental(raw);
    assert_eq!(buf.view(), "đường");
    let stored: String = buf.input().iter().collect();
    assert_eq!(stored, raw);
}

#[test]
fn test_pop_replay_matches_oneshot_prefix_and_expected() {
    // Hand-derived remaining prefixes of dduowngf → đường.
    let cases = [
        (0, "dduowngf", "đường"),
        (1, "dduowng", "đương"),
        (2, "dduown", "đươn"),
        (3, "dduow", "đuơ"),
        (4, "dduo", "đuo"),
    ];

    for (pop_count, remaining, expected) in cases {
        let mut raw_keys: Vec<char> = "dduowngf".chars().collect();
        for _ in 0..pop_count {
            raw_keys.pop();
        }
        let remaining_from_pop: String = raw_keys.iter().collect();
        assert_eq!(remaining_from_pop, remaining);

        let mut replay = transform_buffer_incremental(&TELEX);
        for &ch in &raw_keys {
            replay.push(ch);
        }

        let oneshot = telex(remaining);
        assert_eq!(
            replay.view(),
            expected,
            "incremental replay for '{remaining}'"
        );
        assert_eq!(
            oneshot, expected,
            "oneshot transform_buffer for '{remaining}'"
        );
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
#[test]
fn test_replay_backspace_performance_p95_on_max_composing_token() {
    let max_composing_tokens = ["nghieengs", "nghieeux", "khuys", "thuyeesn"];
    let iterations_per_token = 500;
    let mut durations = Vec::new();

    for &token in &max_composing_tokens {
        for _ in 0..iterations_per_token {
            let mut keys: Vec<char> = token.chars().collect();
            keys.pop();

            let start = Instant::now();
            let mut buf = transform_buffer_incremental(&TELEX);
            for &k in &keys {
                buf.push(k);
            }
            let _ = buf.view();
            durations.push(start.elapsed());
        }
    }

    durations.sort();
    let p95_idx = (durations.len() as f64 * 0.95) as usize;
    let p95 = durations[p95_idx];

    println!("Replay-backspace P95 latency across max tokens: {p95:?}");
    assert!(
        p95.as_micros() < 5000,
        "P95 latency {p95:?} exceeds 5ms threshold"
    );
}
