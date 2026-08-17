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
    // Modern (AccentStyle::New): hoá, oà
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

    // Classic (AccentStyle::Old): hóa, òa
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
    // In vi-rs Telex, typing a tone key on a syllable with existing tone:
    let mut buf = transform_buffer_incremental(&TELEX);
    for ch in "vieetj".chars() {
        buf.push(ch);
    }
    assert_eq!(buf.view(), "việt");

    // Typing 'j' again removes the tone and leaves the raw key at the end: "viêtj"
    buf.push('j');
    assert_eq!(buf.view(), "viêtj");

    // Typing 's' on "toanf" (toàn) -> "toans" (toán)
    let mut buf_switch = transform_buffer_incremental(&TELEX);
    for ch in "toanf".chars() {
        buf_switch.push(ch);
    }
    assert_eq!(buf_switch.view(), "toàn");
    buf_switch.push('s');
    assert_eq!(buf_switch.view(), "toán");
}

#[test]
fn test_passthrough_urls_code_and_english() {
    let cases = [
        ("https", "https"),
        ("simple", "simple"),
        ("facebook", "facebook"),
    ];

    for (raw, expected) in cases {
        let mut buf = transform_buffer_incremental(&TELEX);
        for ch in raw.chars() {
            buf.push(ch);
        }
        assert_eq!(
            buf.view(),
            expected,
            "Mismatch for passthrough input '{raw}'"
        );
    }
}

#[test]
fn test_raw_keys_pop_and_replay_backspace_matches_scratch() {
    let raw_sequence = "dduowngf";
    let mut raw_keys = Vec::new();

    for ch in raw_sequence.chars() {
        raw_keys.push(ch);
    }

    for _ in 0..4 {
        raw_keys.pop();

        // Buffer replayed from popped keys
        let mut replay_buf = transform_buffer_incremental(&TELEX);
        for &k in &raw_keys {
            replay_buf.push(k);
        }

        // Fresh buffer typed from scratch with remaining keys
        let mut scratch_buf = transform_buffer_incremental(&TELEX);
        let remaining_str: String = raw_keys.iter().collect();
        for ch in remaining_str.chars() {
            scratch_buf.push(ch);
        }

        assert_eq!(
            replay_buf.view(),
            scratch_buf.view(),
            "Pop-replay must match fresh typing for keys '{remaining_str}'"
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
