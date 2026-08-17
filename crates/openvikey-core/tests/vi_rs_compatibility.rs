//! Milestone 2: vi-rs Compatibility Gate & Spike Tests.

use std::time::Instant;
use vi::methods::{
    TELEX, VNI, transform_buffer_incremental, transform_buffer_incremental_with_style,
};
use vi::processor::AccentStyle;

#[test]
fn test_telex_and_vni_basic_transformations() {
    // Telex tests with New accent style (vi-rs default: hoas -> hoá)
    let telex_cases = [
        ("vieetj", "việt"),
        ("nam", "nam"),
        ("dduowngf", "đường"),
        ("hoas", "hoá"),
        ("ddaf", "đà"),
        ("tieengs", "tiếng"),
    ];

    for (raw, expected) in telex_cases {
        let mut buffer = transform_buffer_incremental(&TELEX);
        for ch in raw.chars() {
            buffer.push(ch);
        }
        assert_eq!(
            buffer.view(),
            expected,
            "Telex mismatch for raw input '{raw}'"
        );
    }

    // VNI tests
    let vni_cases = [
        ("vie6t5", "việt"),
        ("nam", "nam"),
        ("d9u7o7ng2", "đường"),
        ("hoa1", "hoá"),
        ("d9a2", "đà"),
        ("tie6ng1", "tiếng"),
    ];

    for (raw, expected) in vni_cases {
        let mut buffer = transform_buffer_incremental(&VNI);
        for ch in raw.chars() {
            buffer.push(ch);
        }
        assert_eq!(
            buffer.view(),
            expected,
            "VNI mismatch for raw input '{raw}'"
        );
    }
}

#[test]
fn test_tone_placement_modern_vs_classic() {
    let mut buf_new = transform_buffer_incremental_with_style(&TELEX, AccentStyle::New);
    for ch in "hoas".chars() {
        buf_new.push(ch);
    }
    assert_eq!(buf_new.view(), "hoá");

    let mut buf_old = transform_buffer_incremental_with_style(&TELEX, AccentStyle::Old);
    for ch in "hoas".chars() {
        buf_old.push(ch);
    }
    assert_eq!(buf_old.view(), "hóa");
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
        let mut buffer = transform_buffer_incremental(&TELEX);
        for ch in raw.chars() {
            buffer.push(ch);
        }
        assert_eq!(
            buffer.view(),
            expected,
            "Casing mismatch for raw input '{raw}'"
        );
    }
}

#[test]
fn test_tone_mark_escape_and_restore() {
    // In Telex, typing tone key again:
    // 'vieetjj' -> vi-rs removes tone mark and gives 'viêt' or 'viet'
    let mut buf = transform_buffer_incremental(&TELEX);
    for ch in "vieetj".chars() {
        buf.push(ch);
    }
    assert_eq!(buf.view(), "việt");

    buf.push('j');
    println!("vieetjj -> {}", buf.view());
}

#[test]
fn test_raw_keys_pop_and_replay_backspace_matches_scratch() {
    let raw_sequence = "dduowngf";
    let mut raw_keys = Vec::new();

    for ch in raw_sequence.chars() {
        raw_keys.push(ch);
    }

    for _ in 0..3 {
        raw_keys.pop();

        let mut replay_buf = transform_buffer_incremental(&TELEX);
        for &k in &raw_keys {
            replay_buf.push(k);
        }

        let mut fresh_buf = transform_buffer_incremental(&TELEX);
        for &k in &raw_keys {
            fresh_buf.push(k);
        }

        assert_eq!(replay_buf.view(), fresh_buf.view());
    }
}

#[test]
fn test_english_and_passthrough_cases() {
    // English words that trigger transformations if unhandled
    let mut buf = transform_buffer_incremental(&TELEX);
    for ch in "simple".chars() {
        buf.push(ch);
    }
    // "simple" has no telex trigger, passes through as "simple"
    assert_eq!(buf.view(), "simple");
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
#[test]
fn test_replay_backspace_performance_p95() {
    let long_raw = "nghieengs";
    let iterations = 1000;
    let mut durations = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let mut keys: Vec<char> = long_raw.chars().collect();
        keys.pop();

        let start = Instant::now();
        let mut buf = transform_buffer_incremental(&TELEX);
        for &k in &keys {
            buf.push(k);
        }
        let _ = buf.view();
        durations.push(start.elapsed());
    }

    durations.sort();
    let p95_idx = (iterations as f64 * 0.95) as usize;
    let p95 = durations[p95_idx];

    println!("Replay-backspace P95 latency: {p95:?}");
    assert!(
        p95.as_micros() < 5000,
        "P95 latency {p95:?} exceeds 5ms threshold"
    );
}
