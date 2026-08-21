//! Read-only local chart snapshots built from correction memory.
//!
//! A snapshot is a pure function of (`CorrectionMemory`, `CorrectionIdentity`,
//! `LearningConfigV2`, `evaluate_at_ms`). Same inputs serialize to byte-identical
//! JSON. Building is a Settings/idle operation: it never reads capture logs and
//! never runs on the hook/inject path. Context-free identity only — left-token
//! strings are never included, so the default export leaks no surrounding text.

use crate::correction_memory::CorrectionMemory;
use crate::decision::{ActionCap, DecisionState};
use crate::intervention::{CorrectionIdentity, ScoreBreakdown};
use crate::learning_config::LearningConfigV2;
use serde::{Deserialize, Serialize};

/// Maximum number of recent events rendered into one chart (spec §15.4C).
pub const MAX_CHART_EVENTS: usize = 64;

const MARKER_EPSILON: f64 = 1e-6;
/// Undo feedback adds exactly this much negative mass.
const REVERT_NEGATIVE_MASS: f64 = 1.5;
/// Accept/implicit-correction feedback adds at least this much positive mass.
const ACCEPT_POSITIVE_MASS: f64 = 1.0;

/// Effective display band after planner/source guards (spec §15.4A).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartStateBand {
    Observe,
    Suggest,
    Auto,
    Cooldown,
}

impl ChartStateBand {
    /// Vietnamese state name shared by the chart, overview counts, and JSON.
    #[must_use]
    pub fn vietnamese_label(self) -> &'static str {
        match self {
            Self::Observe => "Đang quan sát",
            Self::Suggest => "Gợi ý",
            Self::Auto => "Có thể tự sửa",
            Self::Cooldown => "Tạm dừng tự sửa",
        }
    }
}

/// Event marker rendered as `+` `−` `↩` or the small weak-settlement dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartMarker {
    Accept,
    Reject,
    Revert,
    WeakSettle,
}

impl ChartMarker {
    #[must_use]
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Accept => "+",
            Self::Reject => "\u{2212}",
            Self::Revert => "\u{21a9}",
            Self::WeakSettle => "\u{00b7}",
        }
    }
}

/// One plotted event: confidence lines plus the effective band at that moment.
///
/// `confidence` is the global-bucket line; `blended_confidence` is the
/// context-blended line. Both coincide for the context-free identity used here.
/// `stored_state` is the persisted hysteresis state (constant per snapshot);
/// `state_band` is re-evaluated at each point's timestamp so cooldown regions
/// appear where guards were active.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartPoint {
    pub at_ms: i64,
    pub confidence: f64,
    pub blended_confidence: f64,
    pub stored_state: DecisionState,
    pub state_band: ChartStateBand,
    pub marker: Option<ChartMarker>,
}

/// Golden, serializable projection of one learned rule (spec §15.4A).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartSnapshot {
    pub identity: CorrectionIdentity,
    pub points: Vec<ChartPoint>,
    pub compaction_marker_at_ms: Option<i64>,
    pub breakdown: ScoreBreakdown,
    pub conclusion: String,
    pub config_hash: String,
}

impl ChartSnapshot {
    /// Builds the snapshot from read-only correction memory.
    ///
    /// Returns `None` after physical forget removes the identity.
    #[must_use]
    pub fn from_memory(
        memory: &CorrectionMemory,
        identity: &CorrectionIdentity,
        config: &LearningConfigV2,
        evaluate_at_ms: i64,
    ) -> Option<Self> {
        if !memory.contains(identity) {
            return None;
        }
        let (events, checkpoint_at_ms) = memory
            .chart_source(identity)
            .unwrap_or_else(|| (Vec::new(), None));
        let stored_state = memory.query_state(identity, None);
        let shrinkage_k = config.context_shrinkage_k;
        let mut points = Vec::new();
        for event in events.iter().rev().take(MAX_CHART_EVENTS).rev() {
            let confidence = memory.blended_confidence(identity, None, event.at_ms, shrinkage_k);
            points.push(ChartPoint {
                at_ms: event.at_ms,
                confidence,
                blended_confidence: confidence,
                stored_state,
                state_band: effective_band(memory, identity, config, event.at_ms),
                marker: marker_for(event.positive, event.negative),
            });
        }
        let breakdown_confidence =
            memory.blended_confidence(identity, None, evaluate_at_ms, shrinkage_k);
        Some(Self {
            identity: identity.clone(),
            points,
            compaction_marker_at_ms: checkpoint_at_ms,
            breakdown: ScoreBreakdown {
                generator_base: 0.0,
                exact_correction: config.score.personal_weight * (breakdown_confidence - 0.5),
                unigram: 0.0,
                bigram: 0.0,
                recent_revert_penalty: 0.0,
                top1_top2_margin: 1.0,
                final_score: breakdown_confidence,
            },
            conclusion: effective_band(memory, identity, config, evaluate_at_ms)
                .vietnamese_label()
                .to_string(),
            config_hash: config.hash(),
        })
    }

    /// Effective band of the rule at the evaluation time.
    #[must_use]
    pub fn state_band(&self) -> ChartStateBand {
        match self.conclusion.as_str() {
            "Gợi ý" => ChartStateBand::Suggest,
            "Có thể tự sửa" => ChartStateBand::Auto,
            "Tạm dừng tự sửa" => ChartStateBand::Cooldown,
            _ => ChartStateBand::Observe,
        }
    }
}

/// Effective band after source policy, revert veto, and decayed confidence.
///
/// Mirrors the planner's hysteresis exit check: a stored Auto survives only
/// while confidence stays at or above `auto_off_confidence`; otherwise the rule
/// presents as Suggest (spec §15.2). An active revert demotion veto presents as
/// Cooldown regardless of the stored state.
#[must_use]
pub fn effective_band(
    memory: &CorrectionMemory,
    identity: &CorrectionIdentity,
    config: &LearningConfigV2,
    evaluate_at_ms: i64,
) -> ChartStateBand {
    let stored_state = memory.query_state(identity, None);
    match stored_state {
        DecisionState::Ignore => ChartStateBand::Observe,
        DecisionState::Suggest => {
            if auto_capable(identity) && !memory.auto_allowed(identity, None, evaluate_at_ms) {
                ChartStateBand::Cooldown
            } else {
                ChartStateBand::Suggest
            }
        }
        DecisionState::Auto => {
            if !auto_capable(identity) {
                return ChartStateBand::Suggest;
            }
            if !memory.auto_allowed(identity, None, evaluate_at_ms) {
                return ChartStateBand::Cooldown;
            }
            let confidence = memory.blended_confidence(
                identity,
                None,
                evaluate_at_ms,
                config.context_shrinkage_k,
            );
            if confidence < config.decision.auto_off_confidence {
                ChartStateBand::Suggest
            } else {
                ChartStateBand::Auto
            }
        }
    }
}

fn auto_capable(identity: &CorrectionIdentity) -> bool {
    identity.source.max_action() == ActionCap::Auto
}

/// Infers the marker from retained evidence deltas (spec §15.4A).
fn marker_for(positive: f64, negative: f64) -> Option<ChartMarker> {
    if negative > MARKER_EPSILON {
        if (negative - REVERT_NEGATIVE_MASS).abs() < MARKER_EPSILON {
            Some(ChartMarker::Revert)
        } else {
            Some(ChartMarker::Reject)
        }
    } else if positive > MARKER_EPSILON {
        if positive >= ACCEPT_POSITIVE_MASS - MARKER_EPSILON {
            Some(ChartMarker::Accept)
        } else {
            Some(ChartMarker::WeakSettle)
        }
    } else {
        None
    }
}
