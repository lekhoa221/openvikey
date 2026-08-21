//! Evaluation formulas locked at Wave 0.
//!
//! Auto precision and correct-token FPR must never share a denominator.
//! Wilson uses the score interval from Wikipedia (z typically 1.96 for 95%).
//!
//! Counts are cast to `f64`. v1 sample sizes stay far below the 53-bit mantissa.

#![allow(clippy::cast_precision_loss)]

use openvikey_core::intervention::{InterventionAction, InterventionReason};
use serde::Serialize;

/// Counts planner outcomes without merging their different risk profiles.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct InterventionPathCounts {
    pub structural_auto: u64,
    pub heuristic_auto: u64,
    pub learned_auto: u64,
    pub suggest: u64,
}

impl InterventionPathCounts {
    pub fn observe(&mut self, action: InterventionAction, reason: InterventionReason) {
        match (action, reason) {
            (InterventionAction::Replace, InterventionReason::SafeStructuralFix) => {
                self.structural_auto = self.structural_auto.saturating_add(1);
            }
            (InterventionAction::Replace, InterventionReason::UniqueHeuristicAssist) => {
                self.heuristic_auto = self.heuristic_auto.saturating_add(1);
            }
            (InterventionAction::Replace, InterventionReason::LearnedCorrection) => {
                self.learned_auto = self.learned_auto.saturating_add(1);
            }
            (InterventionAction::DisplaySuggestion, _) => {
                self.suggest = self.suggest.saturating_add(1);
            }
            (InterventionAction::None | InterventionAction::Replace, _) => {}
        }
    }
}

/// `TP / (TP + FP)`. Undefined when there are no auto decisions.
#[must_use]
pub fn auto_precision(true_auto: u64, false_auto: u64) -> Option<f64> {
    let denom = true_auto.checked_add(false_auto)?;
    if denom == 0 {
        return None;
    }
    Some(true_auto as f64 / denom as f64)
}

/// `false_auto_replacements / total_correct_tokens`. Distinct from precision.
#[must_use]
pub fn correct_token_fpr(false_auto: u64, correct_tokens: u64) -> Option<f64> {
    if correct_tokens == 0 {
        return None;
    }
    Some(false_auto as f64 / correct_tokens as f64)
}

/// Wilson score interval for `successes / trials`.
///
/// Returns `None` when `trials == 0` or `successes > trials`.
#[must_use]
pub fn wilson_interval(successes: u64, trials: u64, z: f64) -> Option<(f64, f64)> {
    if trials == 0 || successes > trials {
        return None;
    }
    let n = trials as f64;
    let p = successes as f64 / n;
    let z2 = z * z;
    let denom = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / denom;
    let margin = z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / denom;
    Some((center - margin, center + margin))
}
