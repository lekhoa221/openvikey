//! Wave 0: lock evaluation formulas before corpus/evaluate exists.
//!
//! Precision and correct-token FPR must use different denominators.
//! Wilson 95% expected bounds are frozen literals from an independent
//! calculation of the Wikipedia Wilson score interval (z = 1.96).

use openvikey_core::intervention::{InterventionAction, InterventionReason};
use openvikey_lab::metrics::{
    InterventionPathCounts, auto_precision, correct_token_fpr, wilson_interval,
};

#[test]
fn auto_precision_uses_tp_plus_fp_denominator() {
    let precision = auto_precision(10, 1).expect("defined when TP+FP > 0");
    assert!((precision - 10.0 / 11.0).abs() < 1e-12);
}

#[test]
fn correct_token_fpr_uses_correct_token_denominator() {
    let fpr = correct_token_fpr(1, 1000).expect("defined when correct tokens > 0");
    assert!((fpr - 0.001).abs() < 1e-12);
}

#[test]
fn precision_and_fpr_do_not_share_a_denominator() {
    // Same raw counts would collide if someone reused TP+FP for FPR.
    let precision = auto_precision(10, 1).unwrap();
    let fpr = correct_token_fpr(1, 1000).unwrap();
    assert!(
        (precision - fpr).abs() > 0.5,
        "precision ({precision}) and FPR ({fpr}) must not share a denominator"
    );
    assert!(precision > 0.9);
    assert!(fpr <= 0.001);
}

#[test]
fn empty_denominators_are_undefined() {
    assert_eq!(auto_precision(0, 0), None);
    assert_eq!(correct_token_fpr(0, 0), None);
    assert_eq!(wilson_interval(0, 0, 1.96), None);
    assert_eq!(wilson_interval(2, 1, 1.96), None);
}

#[test]
fn wilson_95_matches_hand_calculated_cases() {
    let cases = [
        (0_u64, 1_u64, 0.0, 0.793_456_708_5),
        (0, 20, 0.0, 0.161_130_125_5),
        (81, 87, 0.857_601_338_6, 0.968_011_596_4),
        (10, 11, 0.622_635_374_5, 0.983_768_247_7),
        (1, 1000, 0.000_176_541_8, 0.005_642_703_0),
    ];

    for (successes, trials, expected_lo, expected_hi) in cases {
        let (lo, hi) = wilson_interval(successes, trials, 1.96)
            .unwrap_or_else(|| panic!("wilson({successes}/{trials}) must be defined"));
        assert!(
            (lo - expected_lo).abs() < 1e-9,
            "wilson lower {successes}/{trials}: got {lo}, expected {expected_lo}"
        );
        assert!(
            (hi - expected_hi).abs() < 1e-9,
            "wilson upper {successes}/{trials}: got {hi}, expected {expected_hi}"
        );
        assert!((0.0..=1.0).contains(&lo.max(0.0)));
        assert!((0.0..=1.0).contains(&hi.min(1.0)));
    }
}

#[test]
fn intervention_paths_count_structural_heuristic_learned_and_suggest_separately() {
    let mut counts = InterventionPathCounts::default();
    counts.observe(
        InterventionAction::Replace,
        InterventionReason::SafeStructuralFix,
    );
    counts.observe(
        InterventionAction::Replace,
        InterventionReason::UniqueHeuristicAssist,
    );
    counts.observe(
        InterventionAction::Replace,
        InterventionReason::LearnedCorrection,
    );
    counts.observe(
        InterventionAction::DisplaySuggestion,
        InterventionReason::LowScore,
    );
    counts.observe(InterventionAction::None, InterventionReason::NoCandidate);

    assert_eq!(counts.structural_auto, 1);
    assert_eq!(counts.heuristic_auto, 1);
    assert_eq!(counts.learned_auto, 1);
    assert_eq!(counts.suggest, 1);
}
