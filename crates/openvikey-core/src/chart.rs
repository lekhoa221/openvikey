//! Read-only local chart snapshots built from correction memory.
//!
//! A snapshot is a pure function of (`CorrectionMemory`, `CorrectionIdentity`,
//! `LearningConfigV2`, `evaluate_at_ms`). Same inputs serialize to byte-identical
//! JSON. Building is a Settings/idle operation: it never reads capture logs and
//! never runs on the hook/inject path. Context-free identity only — left-token
//! strings are never included, so the default export leaks no surrounding text.

use crate::correction_memory::CorrectionMemory;
use crate::decision::{ActionCap, DecisionState};
use crate::intervention::{
    CorrectionIdentity, InterventionAction, InterventionPlan, InterventionReason, ScoreBreakdown,
};
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
    Unavailable,
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
            Self::Unavailable => "Chưa có đánh giá",
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

/// One plotted event with causal confidence lines.
///
/// `confidence` is the global-bucket line; `blended_confidence` is the
/// context-blended line. Historical state provenance is unavailable in the v2
/// store, so state fields remain `None`/`Unavailable` rather than projecting
/// the current state backward.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartPoint {
    pub at_ms: i64,
    pub confidence: f64,
    pub blended_confidence: f64,
    pub stored_state: Option<DecisionState>,
    pub state_band: ChartStateBand,
    pub marker: Option<ChartMarker>,
}

/// Golden, serializable projection of one learned rule (spec §15.4A).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartSnapshot {
    pub identity: CorrectionIdentity,
    pub points: Vec<ChartPoint>,
    pub compaction_marker_at_ms: Option<i64>,
    pub breakdown: Option<ScoreBreakdown>,
    pub assessment_at_ms: Option<i64>,
    pub current_state_band: ChartStateBand,
    pub conclusion: String,
    pub config_hash: String,
}

/// Authentic current assessment captured from the intervention planner.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartAssessment {
    pub breakdown: ScoreBreakdown,
    pub state_band: ChartStateBand,
    pub evaluated_at_ms: i64,
}

impl ChartAssessment {
    #[must_use]
    pub fn from_plan(plan: &InterventionPlan, evaluated_at_ms: i64) -> Self {
        let state_band = if matches!(
            plan.reason,
            InterventionReason::RecentRevertCooldown
                | InterventionReason::RevertGuardBypass
                | InterventionReason::ExplicitlySuppressed
        ) {
            ChartStateBand::Cooldown
        } else {
            match plan.action {
                InterventionAction::Replace => ChartStateBand::Auto,
                InterventionAction::DisplaySuggestion => ChartStateBand::Suggest,
                InterventionAction::None => ChartStateBand::Observe,
            }
        };
        Self {
            breakdown: plan.score_breakdown.clone(),
            state_band,
            evaluated_at_ms,
        }
    }
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
        Self::from_memory_with_context(memory, identity, config, evaluate_at_ms, None, None)
    }

    /// Builds a snapshot with private context and an optional authentic planner assessment.
    /// The left token influences calculations but is never serialized into the snapshot.
    #[must_use]
    pub fn from_memory_with_context(
        memory: &CorrectionMemory,
        identity: &CorrectionIdentity,
        config: &LearningConfigV2,
        evaluate_at_ms: i64,
        left_token_nfc: Option<&str>,
        assessment: Option<&ChartAssessment>,
    ) -> Option<Self> {
        if !memory.contains(identity) {
            return None;
        }
        let (events, checkpoint_at_ms) = memory
            .chart_source(identity)
            .unwrap_or_else(|| (Vec::new(), None));
        let shrinkage_k = config.context_shrinkage_k;
        let mut points = Vec::new();
        for event in events.iter().rev().take(MAX_CHART_EVENTS).rev() {
            let (confidence, blended_confidence) = memory.chart_confidence_through(
                identity,
                left_token_nfc,
                event.at_ms,
                event.seq,
                shrinkage_k,
            );
            points.push(ChartPoint {
                at_ms: event.at_ms,
                confidence,
                blended_confidence,
                stored_state: None,
                state_band: ChartStateBand::Unavailable,
                marker: marker_for(event.positive, event.negative),
            });
        }
        let current_state_band = assessment.map_or_else(
            || guarded_state_band(memory, identity, evaluate_at_ms, left_token_nfc),
            |assessment| assessment.state_band,
        );
        Some(Self {
            identity: identity.clone(),
            points,
            compaction_marker_at_ms: checkpoint_at_ms,
            breakdown: assessment.map(|assessment| assessment.breakdown.clone()),
            assessment_at_ms: assessment.map(|assessment| assessment.evaluated_at_ms),
            current_state_band,
            conclusion: current_state_band.vietnamese_label().to_string(),
            config_hash: config.hash(),
        })
    }

    /// Effective band of the rule at the evaluation time.
    #[must_use]
    pub fn state_band(&self) -> ChartStateBand {
        self.current_state_band
    }
}

/// Conservative display of persisted state after guards available from memory alone.
///
/// This deliberately does not apply decision thresholds or claim to replace a
/// planner assessment. An active revert veto presents as Cooldown and source
/// policy caps a persisted Auto state to Suggest.
#[must_use]
pub fn guarded_state_band(
    memory: &CorrectionMemory,
    identity: &CorrectionIdentity,
    evaluate_at_ms: i64,
    left_token_nfc: Option<&str>,
) -> ChartStateBand {
    let stored_state = memory.query_state(identity, left_token_nfc);
    if auto_capable(identity) && !memory.auto_allowed(identity, left_token_nfc, evaluate_at_ms) {
        return ChartStateBand::Cooldown;
    }
    match stored_state {
        DecisionState::Ignore => ChartStateBand::Observe,
        DecisionState::Auto if auto_capable(identity) => ChartStateBand::Auto,
        DecisionState::Suggest | DecisionState::Auto => ChartStateBand::Suggest,
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
