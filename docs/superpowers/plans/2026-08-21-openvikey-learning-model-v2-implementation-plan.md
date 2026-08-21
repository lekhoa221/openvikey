# OpenViKey Learning Model v2 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor OpenViKey learning so every Gợi ý/Tự sửa goes through one planner, with exact-correction memory, local unigram/bigram ranking, physical forget, undo-loop guards, and a local learning chart — without changing the Telex/VNI engine or adding a neural net.

**Architecture:** Keep generators pure. Move all None/Suggest/Replace decisions into `intervention::plan_intervention`. Persist personal state as payload v2 (`correction_memory` + `user_language_model` + `maintenance_metadata`). Session owns short-lived `RevertGuard` and transactions; OS adapters never decide learning. Compatibility flags keep current Auto policy until Lát 9 flips the locked product defaults.

**Tech Stack:** Rust 1.96 / edition 2024; `openvikey-core` + `openvikey-session` + `openvikey-lab` + `openvikey-win`; serde JSON payloads; Unicode NFC + grapheme clusters (`unicode-segmentation`); Win32 GDI for charts; no network, no extra runtime.

**Spec:** [`../specs/2026-08-20-openvikey-learning-model-v2-design.md`](../specs/2026-08-20-openvikey-learning-model-v2-design.md) — owner locked all 11 §20 defaults as **v1 accepted** on 2026-08-21.

## Global Constraints

- Do not modify `crates/openvikey-core/src/engine/**`.
- Do not change the meaning of existing `types.rs` fields/variants. New `FeedbackKind` / `InputContext` fields require ADR 0011 (or a follow-up ADR) before the code change.
- Generators never receive a model (`generate/mod.rs` stays pure).
- Store still persists opaque bytes; decode stays in core/session.
- No I/O, network, serialize, sleep, or blocking lock on the hook path.
- Caller supplies all time (`at_ms` / `evaluate_at_ms`). Core never reads wall clock.
- Same snapshot + journal + config hash + time → same model hash and same `InterventionPlan`.
- Production diagnostics never log original/candidate/token/left-context.
- `minimum_correction_graphemes = 2` is an invariant from Lát 2 onward; Settings must not expose a control that lowers it to 1.
- Personal and Diacritics remain Suggest-only through v2.
- Do not copy GPL IME code or unclear-license prior-art data.
- Workspace must stay green after every task: `cargo fmt`, `clippy -D warnings`, `cargo test --workspace --all-features`.
- Each lát is independently reviewable. Do not start Lát N+1 until Lát N tests listed in that task are green.
- Lát 1–8 keep compatibility flags so current Auto policy still matches characterization, except the two product bugs Lát 2 is allowed to fix (1-grapheme suggestions; Space→replace→Backspace→Space loop).
- Lát 9 flips locked product defaults: Abbreviation cold-start Auto off; Fuzzy heuristic assist off unless quality gate is on.
- Lát 10 is observation-only. It must not change typed text or ranking.

---

## Locked product decisions (spec §20, accepted)

1. Immediate Backspace restores text + rolls back pending learning; no strong negative until the user recommits original or uses explicit Undo.
2. Abbreviation target policy: no cold-start Auto; single-word Auto only after personal evidence; multi-word always Suggest.
3. Fuzzy heuristic assist sits behind feature flag + quality gate; learned Auto still allowed with enough evidence.
4. TelexFix structural Auto does not need 18 evidence, but it goes through the planner and takes revert cooldown.
5. v2 language model is unigram + one left bigram. No trigram.
6. Ignored suggestions increment impression only. No runtime `-0.2`.
7. Forget physically scrubs explicit correction identity/mapping and prevents replay resurrection; forgetting one correction does not claim to erase raw typing history.
8. Shared parameters change only via versioned config + calibration, never per-user self-tuning.
9. Correction suggestion/intervention starts at two alphabetic Unicode graphemes in `snapshot.normalized`.
10. Semantic revert uses a 3_000 ms window, 3_000 ms reapply cooldown, and a one-shot boundary bypass for the unchanged raw token.
11. Settings → Learning has a local per-rule timeline, score breakdown, and overview. Data is local and bounded.

---

## File map

| File | Responsibility |
|---|---|
| `docs/decisions/0011-learning-model-v2-seams.md` | Opens planner / payload v2 / capture / ModelView query seams; engine + existing `types.rs` meanings stay frozen |
| `crates/openvikey-core/src/learning_config.rs` | `LearningConfigV2` + hash; wraps current `DecisionConfig` / score / source policy |
| `crates/openvikey-core/src/intervention.rs` | The only None/Suggest/Replace decision. Reason codes, guards, `ScoreBreakdown` |
| `crates/openvikey-core/src/correction.rs` | Generate → rank → call planner. Keep `InterventionConfig` delimiters. Stop upgrading action after planner |
| `crates/openvikey-core/src/correction_memory.rs` | Exact-correction identity, global+context blend, evidence summaries, Personal rows |
| `crates/openvikey-core/src/user_language.rs` | Bounded unigram/bigram from settled commits |
| `crates/openvikey-core/src/chart.rs` | Read-only `ChartSnapshot` from memory + recent events |
| `crates/openvikey-core/src/generalized_error.rs` | Observation-only error-pattern stats (Lát 10) |
| `crates/openvikey-core/src/model.rs` | Payload facade, version, migration, `ModelView` |
| `crates/openvikey-core/src/feedback.rs` | Transactions, settlement, rollback; no Auto upgrade |
| `crates/openvikey-core/src/rank.rs` | Dedupe + bounded exact/language extras; emit breakdown inputs |
| `crates/openvikey-session/src/session.rs` | `RevertGuard`, wire planner, notices, forget commands |
| `crates/openvikey-session/src/capture.rs` | Capture v2 records + compaction checkpoint |
| `crates/openvikey-win/src/chart_view.rs` | GDI chart + text alternative from `ChartSnapshot` |
| `crates/openvikey-win/src/control.rs` | Learning page hosts chart + breakdown |
| `crates/openvikey-core/src/engine/**` | **Do not touch** |
| `crates/openvikey-core/src/generate/**` | Pure generators only; Personal still a cloned table |

Do not unilaterally split `model.rs` until Lát 4, when payload v2 lands. `correction.rs` stays the orchestrator; it must not keep a second Auto path.

```text
CompositionSnapshot + LeftContext
        │
        ▼
 generate (pure) → rank (score + breakdown)
        │
        ▼
 correction_memory query + user_language query
        │
        ▼
 plan_intervention  →  InterventionPlan
        │
        ▼
 session apply / RevertGuard / transaction
        │
        ▼
 feedback settle / rollback / persist
```

---

## Public types locked by this plan

Add these in the tasks below. Later tasks must use these exact names.

```rust
// learning_config.rs
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LearningConfigV2 {
    pub version: u32,
    pub minimum_correction_graphemes: usize, // 2 from Lát 2; never user-lowerable
    pub immediate_revert_window_ms: i64,     // 3_000
    pub reapply_cooldown_ms: i64,            // 3_000
    pub abbrev_cold_start_auto: bool,        // true until Lát 9
    pub fuzzy_heuristic_assist: bool,        // true until Lát 9
    pub decision: DecisionConfig,
    pub score: ScoreConfig,
    pub context_shrinkage_k: f64,            // start 2.0
    pub weak_positive_cap: f64,              // 7.2
    pub max_unigrams: usize,                 // 10_000
    pub max_bigrams: usize,                  // 30_000
    pub max_corrections: usize,              // 10_000
    pub max_context_rows: usize,             // 30_000
    pub max_recent_events_per_bucket: usize, // 64
}

impl LearningConfigV2 {
    pub fn compatibility_v1() -> Self { /* Lát 1–8 defaults, min graphemes 2 from Lát 2 */ }
    pub fn product_v2() -> Self { /* Lát 9: abbrev_cold_start_auto=false, fuzzy_heuristic_assist=false */ }
    pub fn hash(&self) -> String { /* stable SHA-256 of canonical JSON */ }
}

// intervention.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterventionAction { None, DisplaySuggestion, Replace }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterventionReason {
    NoCandidate,
    UnsafeContext,
    TokenTooShort,
    RevertGuardBypass,
    SourceSuggestOnly,
    LowScore,
    LowMargin,
    ExplicitlySuppressed,
    RecentRevertCooldown,
    SafeStructuralFix,
    UniqueHeuristicAssist,
    LearnedCorrection,
    ContextSupportedSuggestion,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScoreBreakdown {
    pub generator_base: f64,
    pub exact_correction: f64,
    pub unigram: f64,
    pub bigram: f64,
    pub recent_revert_penalty: f64,
    pub top1_top2_margin: f64,
    pub final_score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UndoContract {
    pub required: bool,
    pub uses_original_rendered: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InterventionPlan {
    pub action: InterventionAction,
    pub reason: InterventionReason,
    pub candidate_id: Option<u64>,
    pub score_breakdown: ScoreBreakdown,
    pub undo_contract: UndoContract,
    /// Persist only when `Some`. `action == None` is not Ignore.
    pub model_transition: Option<DecisionState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevertGuard {
    pub identity: CorrectionIdentity,
    pub raw_token: String,
    pub focus_generation: u64,
    pub composition_revision: u64,
    pub reapply_cooldown_until_ms: i64,
    pub bypass_next_boundary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CorrectionIdentity {
    pub input_method: InputMethod,
    pub source: CandidateSource,
    pub original_nfc: String,
    pub candidate_nfc: String,
    pub source_rule_id: String,
}

pub fn alphabetic_grapheme_count(normalized: &str) -> usize { /* Unicode graphemes with any alphabetic char */ }

pub fn plan_intervention(
    snapshot: &CompositionSnapshot,
    ranked: &[Candidate],
    lexicon: &Lexicon,
    model: &dyn ModelView,
    config: &LearningConfigV2,
    intervention: InterventionConfig,
    context: InputContext,
    delimiter: Option<char>,
    revert_guard: Option<&RevertGuard>,
    evaluate_at_ms: i64,
    auto_edit_valid: bool,
    input_method: InputMethod,
    left_token_nfc: Option<&str>,
) -> InterventionPlan { unimplemented!() }
```

Guard order (spec §9.2) is mandatory inside `plan_intervention`. No caller may turn Suggest into Replace after this function returns.

---

### Task 0: Lock spec v1 and open seams with ADR 0011

**Files:**
- Modify: `docs/superpowers/specs/2026-08-20-openvikey-learning-model-v2-design.md` (status line only)
- Create: `docs/decisions/0011-learning-model-v2-seams.md`
- Create: `docs/superpowers/plans/2026-08-21-openvikey-learning-model-v2-implementation-plan.md` (copy of this plan)

**Interfaces:**
- Consumes: spec §5, §9, §18, §20
- Produces: accepted spec + ADR listing which seams may change

- [x] **Step 1: Update spec status**

Change the header from `v0.1 — bản nháp` to `v1 — accepted (2026-08-21)`. Append a short note under §20: owner accepted all eleven defaults; implementation follows this plan.

- [x] **Step 2: Write ADR 0011**

Must state:

- Engine and existing `types.rs` meanings remain frozen (ADR 0002).
- Allowed: new modules `intervention`, `learning_config`, later `correction_memory` / `user_language` / `chart`.
- Allowed: `AdaptiveModel` payload version 2 (not `#[serde(default)]` silent v1 reinterpretation).
- Allowed: capture record additions behind a capture version bump.
- `ModelView` may grow read-only query methods (`blended_confidence`, `score_breakdown_inputs`) without changing `confidence`/`state` meaning for v1 callers during Lát 1–3.
- `FeedbackKind::SuggestionSettled` remains deserializable; runtime must stop emitting it (Lát 5). Do not add `InputContext` fields unless a later ADR says so.
- Generators stay model-free. Adapters stay decision-free.

- [x] **Step 3: Copy this plan into `docs/superpowers/plans/`**

- [x] **Step 4: Commit**

```powershell
git add docs/superpowers/specs/2026-08-20-openvikey-learning-model-v2-design.md docs/decisions/0011-learning-model-v2-seams.md docs/superpowers/plans/2026-08-21-openvikey-learning-model-v2-implementation-plan.md
git commit -m "docs: accept learning model v2 spec and open ADR 0011 seams"
```

---

### Task 1: Characterization — current Auto policy matrix

**Files:**
- Test: `crates/openvikey-lab/tests/session_capture.rs` (extend; do not weaken existing asserts)
- Test: `crates/openvikey-core/tests/telex_fix.rs`
- Create: `crates/openvikey-core/tests/learning_v2_characterization.rs`

**Interfaces:**
- Consumes: `LabSession`, `boundary_assist_candidate`, current `InterventionConfig::{win32,electron}`
- Produces: frozen tests for Lát 1 to keep green

- [x] **Step 1: Write failing tests only where coverage is missing**

Add to `learning_v2_characterization.rs`:

```rust
//! Lát 0 freeze of current learning behavior. Lát 2/9 may replace named tests
//! listed in those tasks; every other test here must stay green.

use openvikey_core::correction::{
    InterventionConfig, boundary_assist_candidate, unique_telex_fix_candidate,
};
use openvikey_core::generate::telex_fix::TelexFixGenerator;
use openvikey_core::generate::{Generator, LeftContext};
use openvikey_core::lexicon::{Lexicon, LexiconEntry};
use openvikey_core::types::{CandidateSource, CompositionSnapshot, InputMethod, TonePlacement};

fn snap(raw: &str, rendered: &str) -> CompositionSnapshot {
    CompositionSnapshot::new(1, raw.to_string(), rendered.to_string())
}

fn lex(tokens: &[&str]) -> Lexicon {
    Lexicon::from_entries(
        tokens.iter().map(|token| LexiconEntry {
            token_nfc: (*token).to_string(),
            frequency: 10,
        }),
        [],
        Some("char-v2"),
    )
}

#[test]
fn telex_fix_unique_is_boundary_assist_on_win32_space() {
    let generator = TelexFixGenerator::new(InputMethod::Telex, TonePlacement::Modern);
    let snapshot = snap("chfao", "chfao");
    let candidates = generator.generate(&snapshot, &LeftContext::default());
    let lexicon = lex(&["chào"]);
    assert!(unique_telex_fix_candidate(&candidates).is_some());
    assert!(
        boundary_assist_candidate(
            &snapshot,
            &candidates,
            Some(' '),
            InterventionConfig::win32(),
            &lexicon,
            true,
        )
        .is_some()
    );
}

#[test]
fn diacritics_source_never_boundary_assists() {
    // Keep as documentation if session_capture already covers `khong`.
    // This unit test must fail closed: source Diacritics → None from assist.
    let snapshot = snap("khong", "khong");
    let candidates = vec![openvikey_core::types::Candidate {
        id: 1,
        text: "không".into(),
        source: CandidateSource::Diacritics,
        evidence: "diac:khong".into(),
        base_score: 0.9,
        final_score: 0.9,
    }];
    assert!(
        boundary_assist_candidate(
            &snapshot,
            &candidates,
            Some(' '),
            InterventionConfig::win32(),
            &lex(&["không"]),
            true,
        )
        .is_none()
    );
}
```

Confirm these existing tests remain and are named in the commit message:

- `abbrev_boundary_assist_replaces_on_space_without_accept_mass`
- `fuzzy_boundary_assist_replaces_unique_typo_on_space`
- `ntn_space_does_not_boundary_assist_a_guess`
- `diacritics_does_not_boundary_assist_on_space`
- `two_abbrev_assist_undos_stop_further_space_auto` (current loop-after-one-undo behavior)

- [x] **Step 2: Run characterization**

```powershell
cargo test -p openvikey-core --test telex_fix --test learning_v2_characterization
cargo test -p openvikey-lab --test session_capture abbrev_boundary_assist fuzzy_boundary_assist diacritics_does_not two_abbrev_assist ntn_space
```

Expected: new tests pass after they are written against current APIs (these are freeze tests, not new behavior). If a test fails, fix the test to match current code — do not change production behavior in Lát 0.

- [x] **Step 3: Commit**

```powershell
git add crates/openvikey-core/tests/learning_v2_characterization.rs crates/openvikey-lab/tests/session_capture.rs
git commit -m "test: freeze current Auto policy matrix for learning v2"
```

---

### Task 2: Characterization — learned Auto, feedback, rewind, restart

**Files:**
- Test: `crates/openvikey-core/tests/learning_state_machine.rs`
- Test: `crates/openvikey-core/tests/learning_v2_characterization.rs`
- Test: `crates/openvikey-lab/tests/session_capture.rs`

**Interfaces:**
- Consumes: `AdaptiveModel::apply_feedback`, `LearningSession`, `run_learning_correction_slice`
- Produces: named freeze tests for Accept/Reject/Undo/AutoSettled/SuggestionSettled, composition rewind, restart

- [x] **Step 1: Add missing freeze tests**

```rust
#[test]
fn suggestion_settled_still_adds_negative_zero_point_two_in_v1() {
    let mut model = AdaptiveModel::default();
    let key = /* Telex / Abbreviation / ko→không, left_token None */;
    model.apply_feedback(
        &key,
        &FeedbackEvent {
            seq: 1,
            at_ms: 0,
            kind: FeedbackKind::SuggestionSettled { candidate_id: 9 },
        },
        true,
    );
    assert!((model.negative_mass(&key, 0) - 0.2).abs() < 1e-9);
}

#[test]
fn allow_learning_false_is_zero_mutation() {
    let mut model = AdaptiveModel::default();
    let before = model.to_json_payload().unwrap();
    model.apply_feedback(&key, &accept_event(), false);
    assert_eq!(model.to_json_payload().unwrap(), before);
}
```

If `learning_state_machine.rs` already covers a case, do not duplicate — add a `// covered by test_name` comment in `learning_v2_characterization.rs`.

Must be covered before Lát 1 (existing or new):

- 18 accepts → learned Auto via `run_learning_correction_slice` with valid `AutoEditContext`
- Explicit reject
- Undo of recorded auto
- `AutoSettled` after 10 events + 3_000 ms
- composition rewind matched/unmatched (existing session tests)
- model/capture restart round-trip
- English / `allow_transform=false` / `allow_learning=false` zero mutation

- [x] **Step 2: Run**

```powershell
cargo test -p openvikey-core --test learning_state_machine --test learning_v2_characterization
cargo test -p openvikey-lab --test session_capture
```

Expected: PASS. Lát 0 does not change production code except if a test helper is added.

- [x] **Step 3: Commit**

```powershell
git commit -am "test: freeze learned Auto, feedback, rewind, and no-learning paths"
```

---

### Task 3: Characterization — forget is hide-not-scrub; personal cap; context max-merge

**Files:**
- Test: `crates/openvikey-core/tests/learning_v2_characterization.rs`
- Modify if needed: `crates/openvikey-core/src/model.rs` (read-only helpers only if a test cannot reach private fields; prefer payload JSON asserts)

**Interfaces:**
- Consumes: `AdaptiveModel::forget_rule`, `to_json_payload`, `state`
- Produces: tests that Lát 3/4 must flip

- [x] **Step 1: Write tests that document current (undesired) behavior**

```rust
#[test]
fn forget_rule_v1_keeps_original_strings_in_payload() {
    let mut model = AdaptiveModel::default();
    model.apply_feedback(&key, &accept_event(), true);
    assert!(model.forget_rule(&key));
    let payload = String::from_utf8(model.to_json_payload().unwrap()).unwrap();
    assert!(
        payload.contains("khogn") && payload.contains("không"),
        "v1 forget hides evidence but keeps strings; Lát 3 must invert this test"
    );
}

#[test]
fn state_uses_max_across_left_token_buckets() {
    // seed same identity with left=None Suggest and left=Some("Việt") Auto
    // current ModelView::state takes max — this must stay until Lát 4.
}

#[test]
fn personal_store_rejects_new_pair_at_512() {
    // fill 512 promoted rows, 513th promote returns false / is not stored
}
```

Mark each with `#[cfg_attr]` comments: `superseded by Lát 3/4 tests named X`.

- [x] **Step 2: Run and confirm they pass against current code**

```powershell
cargo test -p openvikey-core --test learning_v2_characterization
```

- [x] **Step 3: Commit**

```powershell
git commit -am "test: freeze v1 forget, personal cap, and context max-merge"
```

---

### Task 4: Characterization — current 1-grapheme suggestions and undo loop

**Files:**
- Test: `crates/openvikey-core/tests/learning_v2_characterization.rs`
- Test: `crates/openvikey-lab/tests/session_capture.rs`

**Interfaces:**
- Consumes: current session Space/Backspace path
- Produces: two tests Lát 2 will invert

- [x] **Step 1: Write the current-behavior tests**

```rust
#[test]
fn v1_single_letter_telex_dd_may_still_enter_correction_pipeline() {
    // Document: raw "dd" / rendered "đ" is one alphabetic grapheme.
    // Current pipeline has no minimum_correction_graphemes guard.
    // Lát 2 replaces this with token_too_short_dd_is_engine_only.
    let snapshot = CompositionSnapshot::new(1, "dd".into(), "đ".into());
    assert_eq!(snapshot.normalized, "đ");
    assert_eq!(snapshot.normalized.graphemes(true).count(), 1);
}

#[test]
fn v1_space_backspace_space_repeats_abbrev_replace() {
    // Equivalent to the first iteration of two_abbrev_assist_undos:
    // ko + Space → không; Backspace → ko; Space → không again.
    // Lát 2 inverts this to commit original on the second Space.
}
```

Add the session-level loop test in `session_capture.rs` named `v1_space_backspace_space_repeats_fuzzy_replace` for `khogn` if not already implied by `two_abbrev_assist_undos_stop_further_space_auto`.

- [x] **Step 2: Run**

```powershell
cargo test -p openvikey-lab --test session_capture v1_space_backspace_space two_abbrev_assist
cargo test -p openvikey-core --test learning_v2_characterization v1_single_letter
```

- [x] **Step 3: Commit**

```powershell
git commit -am "test: freeze v1 one-grapheme pipeline and undo replacement loop"
```

Lát 0 is done when Tasks 1–4 are green and no production behavior has changed.

---

### Task 5: Lát 1 — `LearningConfigV2` + `InterventionPlan` types

**Files:**
- Create: `crates/openvikey-core/src/learning_config.rs`
- Create: `crates/openvikey-core/src/intervention.rs`
- Modify: `crates/openvikey-core/src/lib.rs`
- Test: `crates/openvikey-core/tests/intervention_planner.rs`

**Interfaces:**
- Consumes: `DecisionConfig`, `ScoreConfig`, `InterventionConfig`
- Produces: `LearningConfigV2::compatibility_v1()`, `plan_intervention` stub compiling

- [x] **Step 1: Write the failing test**

```rust
use openvikey_core::intervention::{
    InterventionAction, InterventionReason, plan_intervention,
};
use openvikey_core::learning_config::LearningConfigV2;
use openvikey_core::lexicon::Lexicon;
use openvikey_core::model::EmptyModel;
use openvikey_core::types::{CompositionSnapshot, InputContext};

#[test]
fn empty_candidates_are_none_no_candidate() {
    let plan = plan_intervention(
        &CompositionSnapshot::new(1, "khogn".into(), "khogn".into()),
        &[],
        &Lexicon::from_entries([], [], Some("empty")),
        &EmptyModel,
        &LearningConfigV2::compatibility_v1(),
        openvikey_core::correction::InterventionConfig::win32(),
        InputContext::default(),
        Some(' '),
        None,
        0,
        true,
    );
    assert_eq!(plan.action, InterventionAction::None);
    assert_eq!(plan.reason, InterventionReason::NoCandidate);
    assert_eq!(plan.candidate_id, None);
}

#[test]
fn allow_transform_false_is_none_unsafe_even_with_candidates() {
    let candidate = /* fuzzy khogn→không */;
    let mut ctx = InputContext::default();
    ctx.allow_transform = false;
    let plan = plan_intervention(/* ranked = [candidate], ctx */);
    assert_eq!(plan.action, InterventionAction::None);
    assert_eq!(plan.reason, InterventionReason::UnsafeContext);
}
```

- [x] **Step 2: Run to verify it fails**

```powershell
cargo test -p openvikey-core --test intervention_planner empty_candidates allow_transform
```

Expected: compile fail (`plan_intervention` not found) or FAIL.

- [x] **Step 3: Minimal implementation**

Export modules from `lib.rs`. Implement `plan_intervention` with only guards 1 and 3 from spec §9.2 (unsafe / no candidate). Remaining cases may return `None/LowScore` for now.

`LearningConfigV2::compatibility_v1()`:

- `version = 1`
- `minimum_correction_graphemes = 0` (Lát 2 sets 2)
- `immediate_revert_window_ms = 3_000`
- `reapply_cooldown_ms = 3_000`
- `abbrev_cold_start_auto = true`
- `fuzzy_heuristic_assist = true`
- `decision = DecisionConfig::default()`
- `score = ScoreConfig::abbrev_v1()`
- `context_shrinkage_k = 2.0`
- `weak_positive_cap = 7.2`
- storage limits as spec §8.6 / §11.5

- [x] **Step 4: Run to verify pass**

```powershell
cargo test -p openvikey-core --test intervention_planner
cargo test -p openvikey-core --lib
```

- [x] **Step 5: Commit**

```powershell
git add crates/openvikey-core/src/learning_config.rs crates/openvikey-core/src/intervention.rs crates/openvikey-core/src/lib.rs crates/openvikey-core/tests/intervention_planner.rs
git commit -m "feat: add LearningConfigV2 and intervention planner skeleton"
```

---

### Task 6: Lát 1 — fold learned Auto into the planner

**Files:**
- Modify: `crates/openvikey-core/src/intervention.rs`
- Modify: `crates/openvikey-core/src/correction.rs` (`evaluate_correction_slice` / `run_learning_correction_slice`)
- Test: `crates/openvikey-core/tests/intervention_planner.rs`
- Test: `crates/openvikey-core/tests/learning_state_machine.rs` (must stay green)

**Interfaces:**
- Consumes: `ModelView::{state,confidence,positive_mass,auto_allowed}`, `decide`
- Produces: `InterventionReason::LearnedCorrection` + `Replace` when current `decide` would return Auto and `auto_edit_valid`

- [x] **Step 1: Write failing test**

```rust
#[test]
fn learned_auto_with_valid_edit_replaces_and_reasons_learned_correction() {
    let model = /* 18 Accepts on khogn→không Fuzzy */;
    let ranked = /* top candidate không, source Fuzzy, scores matching current rank */;
    let plan = plan_intervention(..., &model, ..., auto_edit_valid: true);
    assert_eq!(plan.action, InterventionAction::Replace);
    assert_eq!(plan.reason, InterventionReason::LearnedCorrection);
    assert_eq!(plan.candidate_id, Some(ranked[0].id));
    assert!(plan.undo_contract.required);
}

#[test]
fn learned_auto_without_valid_edit_degrades_to_suggestion() {
    let plan = plan_intervention(..., auto_edit_valid: false);
    assert_eq!(plan.action, InterventionAction::DisplaySuggestion);
    assert_eq!(plan.reason, InterventionReason::LearnedCorrection);
}
```

- [x] **Step 2: Run — expect FAIL** (skeleton returns None/LowScore)

- [x] **Step 3: Implement**

Inside planner, after safety/candidate guards, compute `decide(...)` on top candidate exactly as `evaluate_correction_slice` does today. If `DecisionState::Auto` and `auto_edit_valid` → Replace/LearnedCorrection. If Auto but invalid edit → DisplaySuggestion. If Suggest → DisplaySuggestion/LowMargin or Learned path still Suggest. Source cap still uses `CandidateSource::max_action`.

Change `evaluate_correction_slice` to:

1. generate + rank (unchanged)
2. `plan = plan_intervention(...)`
3. map `InterventionPlan` → `CorrectionSlice.{decision,action}`
4. stop calling `decide` itself

`run_learning_correction_slice` records decision/auto-edit only when the plan says Replace.

Do **not** remove `boundary_assist_candidate` yet (Task 7). If planner does not Replace, existing session code may still assist — keep that until Task 7/8.

- [x] **Step 4: Run**

```powershell
cargo test -p openvikey-core --test intervention_planner --test learning_state_machine
```

- [x] **Step 5: Commit**

```powershell
git commit -am "feat: route learned Auto through the unified intervention planner"
```

---

### Task 7: Lát 1 — fold `boundary_assist_candidate` into the planner

**Files:**
- Modify: `crates/openvikey-core/src/intervention.rs`
- Modify: `crates/openvikey-core/src/correction.rs` (`boundary_assist_candidate` becomes a thin wrapper or `pub(crate)` helper used only by planner)
- Test: `crates/openvikey-core/tests/telex_fix.rs`
- Test: `crates/openvikey-core/tests/intervention_planner.rs`

**Interfaces:**
- Consumes: current `unique_telex_fix_candidate`, `unique_top_assist_candidate`, `InterventionConfig`
- Produces: `SafeStructuralFix` / `UniqueHeuristicAssist` reasons; public `boundary_assist_candidate` still exists for old tests but must call the planner (or share the same helper)

- [x] **Step 1: Write failing tests**

```rust
#[test]
fn unique_telex_fix_is_replace_safe_structural_fix() {
    // chfao → chào, lexicon has chào not chfao, win32 space, cold model
    assert_eq!(plan.action, InterventionAction::Replace);
    assert_eq!(plan.reason, InterventionReason::SafeStructuralFix);
}

#[test]
fn unique_abbrev_ko_is_replace_unique_heuristic_when_compat_flag_on() {
    assert_eq!(plan.reason, InterventionReason::UniqueHeuristicAssist);
}

#[test]
fn unique_fuzzy_khogn_is_replace_when_compat_flag_on() {
    assert_eq!(plan.reason, InterventionReason::UniqueHeuristicAssist);
}

#[test]
fn diacritics_unique_is_display_suggestion_source_suggest_only() {
    assert_eq!(plan.action, InterventionAction::DisplaySuggestion);
    assert_eq!(plan.reason, InterventionReason::SourceSuggestOnly);
}
```

Keep TelexFix/abbrev/fuzzy unit tests in `telex_fix.rs` green by delegating `boundary_assist_candidate` to planner output (`action == Replace`).

- [x] **Step 2: Run — expect FAIL** on reason codes

- [x] **Step 3: Implement guard steps 8 and 10 of spec §9.2 inside planner**, using the existing helper logic. Compatibility flags:

- `abbrev_cold_start_auto` gates Abbreviation heuristic Replace
- `fuzzy_heuristic_assist` gates Fuzzy heuristic Replace
- TelexFix structural Replace stays on `InterventionConfig.telex_fix_policy_auto` + delimiter policy (unchanged)
- **v1 order through Lát 8:** Learned Auto, then Structural, then heuristic. Spec §9.2 Structural-before-Learned waits until Lát 9 policy flip.

- [x] **Step 4: Run**

```powershell
cargo test -p openvikey-core --test intervention_planner --test telex_fix --test learning_state_machine
```

- [x] **Step 5: Commit**

```powershell
git commit -am "feat: move boundary assist into the intervention planner"
```

---

### Task 8: Lát 1 — session must not upgrade Suggest after planner

**Files:**
- Modify: `crates/openvikey-session/src/session.rs` (`correction_slice`, ~802–891)
- Test: `crates/openvikey-lab/tests/session_capture.rs`
- Test: `crates/openvikey-core/tests/learning_v2_characterization.rs`

**Interfaces:**
- Consumes: `InterventionPlan` from `run_learning_correction_slice` (extend `CorrectionSlice` with `plan: Option<InterventionPlan>`)
- Produces: one decision path; `slice.decision == Auto` iff plan.action == Replace

- [x] **Step 1: Write failing test**

```rust
#[test]
fn session_does_not_replace_when_planner_returned_suggest() {
    // Construct a candidate list that old boundary_assist would replace
    // but force planner Suggest by turning telex_fix_policy_auto off and
    // using a source that is Suggest-only. Assert no ReplaceRange.
}
```

Extend `CorrectionSlice`:

```rust
pub struct CorrectionSlice {
    pub candidates: Vec<Candidate>,
    pub decision: Option<DecisionState>,
    pub action: Option<EngineAction>,
    pub plan: Option<InterventionPlan>,
}
```

- [x] **Step 2: Run — expect FAIL** while session still calls `boundary_assist_candidate` after the slice

- [x] **Step 3: Implement**

Delete the post-slice `boundary_assist_candidate` block in `LabSession::correction_slice`. Session applies Replace only when `slice.plan.action == Replace` (or `slice.action` already set by `run_learning_correction_slice`). Notices:

- `SafeStructuralFix` → keep current silent TelexFix notice behavior (`pending_restore_learn_undo = false`)
- `UniqueHeuristicAssist` / `LearnedCorrection` → current `LearningNoticeKind::Corrected`

All Task 1 characterization Auto tests must still pass.

- [x] **Step 4: Run**

```powershell
cargo test -p openvikey-lab --test session_capture
cargo test -p openvikey-core --test intervention_planner --test telex_fix --test learning_state_machine --test learning_v2_characterization
cargo test --workspace --all-features
```

- [x] **Step 5: Commit**

```powershell
git commit -am "refactor: make session honor planner output as the only Auto path"
```

Lát 1 done when: every Gợi ý/Tự sửa has a stable reason; workspace green; **no product policy change** except that reason codes exist.

---

### Task 9: Lát 2 — minimum two alphabetic graphemes

**Files:**
- Modify: `crates/openvikey-core/src/learning_config.rs` (`minimum_correction_graphemes = 2` in `compatibility_v1`)
- Modify: `crates/openvikey-core/src/intervention.rs` (`alphabetic_grapheme_count`, guard 2)
- Test: `crates/openvikey-core/tests/intervention_planner.rs`
- Test: `crates/openvikey-core/tests/learning_v2_characterization.rs` (invert Task 4 1-grapheme test)
- Test: `crates/openvikey-core/tests/golden_engine.rs` (must stay green)

**Interfaces:**
- Consumes: `snapshot.normalized`
- Produces: `InterventionReason::TokenTooShort`; engine one-letter composition unchanged

- [x] **Step 1: Write failing tests**

```rust
#[test]
fn one_alphabetic_grapheme_is_none_token_too_short() {
    let snapshot = CompositionSnapshot::new(1, "dd".into(), "đ".into());
    let ranked = vec![/* any candidate đ or da */];
    let plan = plan_intervention(&snapshot, &ranked, ..., auto_edit_valid: true);
    assert_eq!(plan.action, InterventionAction::None);
    assert_eq!(plan.reason, InterventionReason::TokenTooShort);
}

#[test]
fn a1_rendered_as_a_acute_is_token_too_short() {
    let snapshot = CompositionSnapshot::new(1, "a1".into(), "á".into());
    assert_eq!(alphabetic_grapheme_count(&snapshot.normalized), 1);
}

#[test]
fn two_graphemes_are_eligible() {
    let snapshot = CompositionSnapshot::new(1, "ko".into(), "ko".into());
    assert_eq!(alphabetic_grapheme_count(&snapshot.normalized), 2);
    // planner may Suggest/Replace depending on candidates, but reason != TokenTooShort
}

#[test]
fn engine_still_composes_dd_to_d_stroke() {
    // use engine API already covered by golden_engine; do not put correction here
}
```

Invert/remove `v1_single_letter_telex_dd_may_still_enter_correction_pipeline`.

- [x] **Step 2: Run — expect FAIL** (`minimum_correction_graphemes` still 0)

- [x] **Step 3: Implement**

```rust
pub fn alphabetic_grapheme_count(normalized: &str) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    normalized
        .graphemes(true)
        .filter(|grapheme| grapheme.chars().any(char::is_alphabetic))
        .count()
}
```

Count **rendered NFC graphemes with a letter**, never raw key length. Settings must not grow a slider for this.

- [x] **Step 4: Run**

```powershell
cargo test -p openvikey-core --test intervention_planner --test golden_engine --test learning_v2_characterization
cargo test -p openvikey-lab --test session_capture
```

- [x] **Step 5: Commit**

```powershell
git commit -am "fix: block correction suggestions below two alphabetic graphemes"
```

---

### Task 10: Lát 2 — `RevertGuard` 3s cooldown + one-shot boundary bypass

**Files:**
- Modify: `crates/openvikey-core/src/intervention.rs`
- Modify: `crates/openvikey-core/src/correction.rs` (planner-owned visible candidate IDs)
- Modify: `crates/openvikey-core/src/feedback.rs` if undo outcome needs to return identity
- Modify: `crates/openvikey-session/src/session.rs` (`try_restore_policy_undo`, `correction_slice`, clear guard on focus/method/reset/raw change)
- Test: `crates/openvikey-lab/tests/session_capture.rs`
- Test: `crates/openvikey-core/tests/intervention_planner.rs`

**Interfaces:**
- Consumes: `LearningConfigV2::{immediate_revert_window_ms,reapply_cooldown_ms}`
- Produces: `RevertGuard`; `InterventionReason::RevertGuardBypass`

- [x] **Step 1: Write failing tests (these invert Task 4 / `two_abbrev` first-Space-after-one-undo)**

```rust
#[test]
fn space_replace_backspace_space_commits_original_once() {
    let mut session = LabSession::new(EngineConfig::default(), khong_lexicon());
    type_keys(&mut session, "khogn", 0);
    let replaced = session.inject(InputKind::Boundary { delimiter: ' ' }, InputContext::default(), 10);
    assert!(matches!(replaced.action, Some(EngineAction::ReplaceRange(_))));
    session.inject(InputKind::Backspace, InputContext::default(), 11);
    assert_eq!(session.composition_text(), "khogn");
    let second = session.inject(InputKind::Boundary { delimiter: ' ' }, InputContext::default(), 12);
    assert!(
        !matches!(second.action, Some(EngineAction::ReplaceRange(_))),
        "first boundary after revert must commit original, got {:?}",
        second.action
    );
    assert!(session.document_text().contains("khogn"));
    assert!(!session.document_text().contains("không"));
}

#[test]
fn bypass_still_applies_after_cooldown_if_raw_token_unchanged() {
    // replace at t=0, backspace at t=1000, wait until t=10_000, Space
    // still commit original once
}

#[test]
fn changing_raw_token_clears_guard_and_allows_new_correction() {
    // khogn → không → BS → khogn → type x → khognx → Space may suggest/replace other rules
}

#[test]
fn planner_hides_reverted_candidate_from_replace_and_overlay_during_cooldown() {
    let guard = RevertGuard { /* identity khogn→không, raw khogn, bypass_next_boundary true, cooldown now+3000 */ };
    let plan = plan_intervention(..., Some(&guard), evaluate_at_ms: 1_500);
    assert_eq!(plan.action, InterventionAction::None);
    assert_eq!(plan.reason, InterventionReason::RevertGuardBypass);
}
```

Keep `two_abbrev_assist_undos_stop_further_space_auto` but update its first-Space-after-one-undo expectation: after one Backspace, the next Space must **not** replace. Two-revert long cooldown remains (already demotes Auto).

- [x] **Step 2: Run — expect FAIL** (current code re-applies)

- [x] **Step 3: Implement**

On successful semantic revert inside `immediate_revert_window_ms`:

1. restore raw composition (already done)
2. cancel pending settlement / do not yet change Lát 5 mass rules
3. set session `revert_guard` with identity + raw token + revision + `bypass_next_boundary = true` + `reapply_cooldown_until_ms = undo_at_ms + 3_000`
4. planner: if guard matches identity+raw, skip that candidate for Replace and overlay; other candidates at most Suggest
5. if `bypass_next_boundary` and raw unchanged, force None/RevertGuardBypass once even after cooldown
6. clear guard after that original commit, or when raw/focus/method/mode/Reset changes

Guard is session-only. Do not persist it. The planner also returns `display_candidate_ids` so the correction slice can hide only the reverted candidate without leaking it through overlay/Accept.

- [x] **Step 4: Run**

```powershell
cargo test -p openvikey-lab --test session_capture space_replace_backspace two_abbrev_assist
cargo test -p openvikey-core --test intervention_planner
cargo test --workspace --all-features
```

- [x] **Step 5: Commit**

```powershell
git commit -am "fix: arm RevertGuard so undo plus Space cannot loop replacements"
```

Lát 2 done when: 1-grapheme tokens never suggest/replace; undo loop tests pass; engine golden tests still pass.

---

### Task 11: Lát 3 — physical forget scrubs semantic identity and blocks resurrection

**Files:**
- Modify: `crates/openvikey-core/src/model.rs` (`forget_rule`, `forget_inspection_row`, `forget_personal_pair`)
- Modify: `crates/openvikey-session/src/capture.rs` + `session.rs` (`forget_last_rule`)
- Test: `crates/openvikey-core/tests/physical_forget.rs`
- Test: invert `forget_rule_v1_keeps_original_strings_in_payload`

**Interfaces:**
- Consumes: `CorrectionIdentity` (may still use `RuleContextKey` in v1 payload)
- Produces: forget APIs that remove rows; capture compaction removes explicit identity strings and a durable tombstone prevents final replay resurrection

- [x] **Step 1: Write failing test**

```rust
#[test]
fn forget_rule_removes_original_and_candidate_from_serialized_model() {
    let mut model = AdaptiveModel::default();
    model.apply_feedback(&key, &accept_event(), true);
    assert!(model.forget_rule(&key));
    let payload = String::from_utf8(model.to_json_payload().unwrap()).unwrap();
    assert!(!payload.contains("khogn"), "{payload}");
    assert!(
        !payload.contains("không") || /* allowed only if independently present in other rows */,
        "{payload}"
    );
}

#[test]
fn forget_all_learning_data_resets_to_cold_start_hash() {
    // forget_all → payload equals AdaptiveModel::default() canonical JSON
}
```

Add a session/lab test: after forget, trimmed capture replay cannot resurrect the rule. Compaction removes semantic correction records for that identity; raw replay commands may remain because forgetting one correction is distinct from deleting typing history, so a durable forget marker must make the final replay state authoritative (spec §11.3 / §12.2). Minimum for Lát 3 on v1 capture is fail-closed whole-journal clearing because v1 lacks identity metadata. Do not add full v2 capture records yet (Lát 5).

- [x] **Step 2: Run — expect FAIL** (v1 forget only clears evidence)

- [x] **Step 3: Implement**

`forget_rule` **removes** the `RuleEntry` from `entries` (do not leave an empty entry with strings). Forget Personal removes count+promoted rows. `forget_all` resets model to default config/empty entries. Session forget rewrites capture with a new checkpoint (even a v1-compatible trim) and coherent-saves.

- [x] **Step 4: Run**

```powershell
cargo test -p openvikey-core --test physical_forget --test learning_v2_characterization
cargo test -p openvikey-lab --test session_capture
cargo test -p openvikey-win --test ui_rules
```

- [x] **Step 5: Commit**

```powershell
git commit -am "fix: physically scrub forgotten correction rows from model payload"
```

---

### Task 12: Lát 3 — global caps and eviction on the v1 store

**Files:**
- Modify: `crates/openvikey-core/src/model.rs`
- Modify: `crates/openvikey-core/src/learning_config.rs` (read limits; v1 model may copy the numbers)
- Test: `crates/openvikey-core/tests/physical_forget.rs` or `learning_v2_characterization.rs`

**Interfaces:**
- Consumes: `max_corrections = 10_000` (use a smaller test override via `ModelConfig` if adding a field needs ADR — prefer new `ModelConfig.max_rules` with default 10_000)
- Produces: deterministic eviction instead of silent growth / personal 512 hard-reject

- [x] **Step 1: Write failing tests**

```rust
#[test]
fn exceeding_max_rules_evicts_oldest_ignore_before_rejecting_new_evidence() {
    let config = ModelConfig { max_events_per_rule: 8, auto_undo_window: 10, /* max_rules: 3 */ };
    // insert 3 rules with evidence, add 4th → weakest Ignore/probation row gone, 4th stored
}

#[test]
fn personal_at_cap_evicts_weak_count_row_instead_of_dropping_new_pair() {
    // invert personal_store_rejects_new_pair_at_512: with max 2 in test config,
    // a stale count=1 row is removed so a new pair can enter probation
}
```

Eviction order (spec §11.5): keep suppression/user-authored, recently used, strong evidence, intervention-eligible, Personal promoted; evict old Ignore/probation, high-impression unused (impression may be 0 until Lát 5), expired.

- [x] **Step 2: Run — expect FAIL**

- [x] **Step 3: Implement bounded insert on `entry_mut` / `promote_personal`. Never scan the whole model on the hook path beyond the already-keyed lookup; eviction runs when inserting a **new** rule, still in-memory.

- [x] **Step 4: Run** `cargo test -p openvikey-core --test physical_forget --test learning_state_machine`

- [x] **Step 5: Commit**

```powershell
git commit -am "feat: bound adaptive and personal stores with deterministic eviction"
```

---

### Task 13: Lát 4 — `CorrectionIdentity` + support-aware context blend

**Files:**
- Create: `crates/openvikey-core/src/correction_memory.rs`
- Modify: `crates/openvikey-core/src/lib.rs`
- Test: `crates/openvikey-core/tests/correction_memory_v2.rs`

**Interfaces:**
- Consumes: `CorrectionIdentity`, `LearningConfigV2.context_shrinkage_k`
- Produces: `CorrectionMemory::{blended_confidence, blended_mass, query_state}` that does **not** take `max(Auto)` across left tokens

- [x] **Step 1: Write failing tests**

```rust
#[test]
fn empty_context_uses_global_confidence() {
    let mut memory = CorrectionMemory::default();
    memory.apply(/* global identity khogn→không, left None, +1.0 at t=0 */);
    let global = memory.blended_confidence(&id, None, 0, 2.0);
    let with_left = memory.blended_confidence(&id, Some("Việt"), 0, 2.0);
    assert!((global - with_left).abs() < 1e-12);
}

#[test]
fn low_support_context_shrinks_toward_global() {
    // global +10, context-left +1 → blended closer to global than to context
}

#[test]
fn high_support_context_can_diverge() {
    // global Suggest-level, context with many negatives → blended Suggest/Ignore,
    // never Auto just because a sibling context is Auto
}

#[test]
fn sibling_context_auto_does_not_force_other_context_auto() {
    // this inverts ModelView::state max-merge once AdaptiveModel delegates to memory
}
```

Formula:

```text
context_weight = context_support / (context_support + shrinkage_k)
confidence = context_weight * context_confidence + (1 - context_weight) * global_confidence
```

`context_support` = decayed positive+negative mass in that left-token bucket.

- [x] **Step 2: Run — expect FAIL**

- [x] **Step 3: Implement `CorrectionMemory` in-memory only** (payload integration is Task 15). Keep applying evidence twice: once global (`left=None` bucket) and once for the actual left token.

- [x] **Step 4: Run** `cargo test -p openvikey-core --test correction_memory_v2`

- [x] **Step 5: Commit**

```powershell
git commit -am "feat: add correction memory with support-aware context blending"
```

---

### Task 14: Lát 4 — evidence summary compaction equivalent to raw events

**Files:**
- Modify: `crates/openvikey-core/src/correction_memory.rs`
- Test: `crates/openvikey-core/tests/correction_memory_v2.rs`

**Interfaces:**
- Consumes: `max_recent_events_per_bucket = 64`
- Produces: `compact_at(evaluate_at_ms)` preserving query results within `1e-9`

- [x] **Step 1: Write failing test**

```rust
#[test]
fn compaction_preserves_decayed_mass_within_1e_9() {
    // push 80 events over 40 days
    let before = memory.blended_confidence(&id, Some("tôi"), t1, 2.0);
    memory.compact_at(t1, 64);
    let after = memory.blended_confidence(&id, Some("tôi"), t1, 2.0);
    assert!((before - after).abs() < 1e-9);
    assert!(memory.recent_event_count(&id, Some("tôi")) <= 64);
}

#[test]
fn compaction_is_idempotent_for_same_checkpoint() {
    memory.compact_at(t1, 64);
    let h1 = memory.stable_hash();
    memory.compact_at(t1, 64);
    assert_eq!(h1, memory.stable_hash());
}
```

- [x] **Step 2: Run — expect FAIL**

- [x] **Step 3: Implement per-bucket**

```text
decayed_positive_at_checkpoint
decayed_negative_at_checkpoint
checkpoint_at_ms
recent_events[]
```

Query decays the summary from checkpoint then adds recent events. Duplicate seq remains idempotent.

- [x] **Step 4: Run** `cargo test -p openvikey-core --test correction_memory_v2`

- [x] **Step 5: Commit**

```powershell
git commit -am "feat: compact correction evidence without changing query results"
```

---

### Task 15: Lát 4 — payload version 2 + deterministic v1 migration

**Files:**
- Modify: `crates/openvikey-core/src/model.rs` (`MODEL_VERSION = 2`, `from_json_payload`)
- Create: `crates/openvikey-core/tests/fixtures/model_v1_personal_and_context.json`
- Test: `crates/openvikey-core/tests/model_migration_v2.rs`
- Modify: `crates/openvikey-session/src/persistence.rs` only if save needs to keep `.bak` of v1 (already does file `.bak`)

**Interfaces:**
- Consumes: v1 `{version, config, entries, personal}`
- Produces: v2 `{version:2, config_hash, correction_memory, user_language_model, maintenance_metadata}`

- [x] **Step 1: Write failing tests**

```rust
#[test]
fn v1_fixture_migrates_deterministically() {
    let bytes = include_bytes!("fixtures/model_v1_personal_and_context.json");
    let a = AdaptiveModel::from_json_payload(bytes).unwrap();
    let b = AdaptiveModel::from_json_payload(bytes).unwrap();
    assert_eq!(a.to_json_payload().unwrap(), b.to_json_payload().unwrap());
    assert_eq!(a.payload_version(), 2);
}

#[test]
fn migration_does_not_promote_suggest_to_auto() {
    // v1 Auto that fails v2 planner Auto guards becomes query-time Suggest
}

#[test]
fn serde_default_cannot_smuggle_v1_as_v2_meanings() {
    AdaptiveModel::from_json_payload(br#"{"version":2}"#).unwrap_err();
}
```

Build the v1 fixture from a real `to_json_payload()` of current code (capture in test setup generator, then freeze bytes).

- [x] **Step 2: Run — expect FAIL** (`version == 1`)

- [x] **Step 3: Implement migration exactly as spec §11.2.** Encrypted envelope unchanged (lab). Windows open JSON (ADR 0008) still stores payload bytes. Backup remains the existing `.bak` path. Do not auto-upgrade intervention rights.

Wire `ModelView` to blended query. Invert `state_uses_max_across_left_token_buckets`.

Empty language model namespace is `{"unigrams":[],"bigrams":[]}` for now.

- [x] **Step 4: Run**

```powershell
cargo test -p openvikey-core --test model_migration_v2 --test learning_state_machine --test physical_forget
cargo test -p openvikey-lab --test session_capture
cargo test -p openvikey-win --test ui_rules
```

- [x] **Step 5: Commit**

```powershell
git commit -am "feat: migrate adaptive model payload to v2 correction memory"
```

---

### Task 16: Lát 4 — merge Personal into correction memory

**Files:**
- Modify: `crates/openvikey-core/src/correction_memory.rs`
- Modify: `crates/openvikey-core/src/generate/personal.rs` (still table-driven; table comes from memory.promoted_personal())
- Modify: `crates/openvikey-core/src/model.rs` (stop owning a separate `PersonalCorrectionStore` after migration)
- Test: `crates/openvikey-core/tests/correction_memory_v2.rs`
- Test: `crates/openvikey-core/tests/learning_state_machine.rs` personal tests

**Interfaces:**
- Consumes: two independent transactions (`seq`/`edit`/`session` anchors)
- Produces: Personal source Suggest-only; never Auto; decay/suppression/forget like others

- [x] **Step 1: Write failing tests**

```rust
#[test]
fn first_personal_observation_is_probation_and_does_not_generate() {
    memory.observe_personal(method, "aaa", "bbb", tx1);
    assert!(memory.promoted_personal(method).is_empty());
}

#[test]
fn second_independent_transaction_promotes_suggest_only() {
    memory.observe_personal(..., tx1);
    memory.observe_personal(..., tx2);
    assert_eq!(memory.promoted_personal(method), vec![("aaa","bbb")]);
    assert!(!memory.allows_auto(&personal_id, 0));
}

#[test]
fn replayed_same_seq_does_not_count_as_second_observation() { /* idempotent */ }
```

- [x] **Step 2: Run — expect FAIL** if still using count-only store

- [x] **Step 3: Implement.** Remove runtime writes to the old `PersonalCorrectionStore` after v2. Keep a deserialize path only inside migration. V1 Personal counts have no timestamp: retain them as transaction support for probation/promotion, but do not fabricate decaying recency evidence at `at_ms = 0`. Keep the intentionally supported pre-transaction v2 checkpoint behind a frozen fixture.

- [x] **Step 4: Run** personal + session_capture personal tests

- [x] **Step 5: Commit**

```powershell
git commit -am "feat: fold personal pairs into correction memory with suggest-only cap"
```

Lát 4 done when: v1 fixtures migrate; blend replaces max-merge; Personal is Suggest-only in the same store.

---

### Task 17: Lát 5 — split immediate revert, explicit Undo, confirmed reject

**Files:**
- Modify: `crates/openvikey-core/src/feedback.rs`
- Modify: `crates/openvikey-session/src/session.rs` (`try_restore_policy_undo`, `undo_last_with_learning`)
- Test: `crates/openvikey-core/tests/learning_state_machine.rs`
- Test: `crates/openvikey-lab/tests/session_capture.rs`

**Interfaces:**
- Consumes: spec §7.1–7.3 weights
- Produces: Backspace-in-window = rollback + guard, **no** `-1.5`; explicit Undo hotkey = `-1.5`; recommit original after rollback = `-1.5` once

- [x] **Step 1: Write failing tests**

```rust
#[test]
fn immediate_backspace_does_not_add_strong_negative() {
    // apply assist replace, backspace within 3s
    let (pos, neg) = model.evidence_totals(&key);
    assert_eq!(neg, 0.0);
}

#[test]
fn explicit_undo_hotkey_adds_one_point_five_negative() {
    session.undo_last(at_ms);
    assert!((model.negative_mass(&key, at_ms) - 1.5).abs() < 1e-9);
}

#[test]
fn recommit_original_after_rollback_adds_one_negative_once() {
    // replace → BS → Space commits original (RevertGuard) → negative 1.5, not twice
}

#[test]
fn one_edit_cannot_feedback_twice() {
    session.undo_last(at_ms);
    session.undo_last(at_ms + 1);
    assert!((model.negative_mass(&key, at_ms) - 1.5).abs() < 1e-9);
}
```

- [x] **Step 2: Run — expect FAIL** (`try_restore_policy_undo` called `learning.undo` and applied `FeedbackKind::Undo` -1.5)

- [x] **Step 3: Implement**

- Immediate revert: rollback settlement + language pending (none yet) **without** `FeedbackKind::Undo`
- `undo_last` (hotkey): inverse text + `FeedbackKind::Undo` -1.5 + cooldown/demotion
- Guard-bypass original commit: strong veto `-1.5` once (confirmed rejection). `ExplicitReject` keeps its frozen `-1.0` meaning; the implementation reuses the existing calibrated `Undo` signal for model evidence after text rollback, and Task 19 records the semantic action as `CorrectionConfirmed`.
- Do not add new `FeedbackKind` variants unless ADR 0011 already allows it; prefer existing kinds

- [x] **Step 4: Run** session_capture + learning_state_machine + revert tests

- [x] **Step 5: Commit**

```powershell
git commit -am "feat: separate immediate revert from explicit undo and confirmed reject"
```

---

### Task 18: Lát 5 — settlement cap, impressions without confidence

**Files:**
- Modify: `crates/openvikey-core/src/model.rs` / `correction_memory.rs` / `feedback.rs`
- Test: `crates/openvikey-core/tests/learning_state_machine.rs`

**Interfaces:**
- Consumes: `weak_positive_cap = 7.2`, settlement 10 events + 3_000 ms
- Produces: no runtime `SuggestionSettled` -0.2; `shown_count` / `selected_count` on the row

- [x] **Step 1: Write failing tests**

```rust
#[test]
fn settlement_alone_cannot_reach_promote_mass_18() {
    // apply many AutoSettled (cap 7.2) → state stays Suggest
    assert!(model.positive_mass(&key, t) <= 7.2 + 1e-9);
    assert_ne!(query_state, DecisionState::Auto);
}

#[test]
fn shown_suggestion_increments_impression_not_negative_mass() {
    memory.record_impression(&id, at_ms);
    assert_eq!(memory.negative_mass(&id, None, at_ms), 0.0);
    assert_eq!(memory.shown_count(&id), 1);
}

#[test]
fn suggestion_settled_from_old_payload_does_not_apply_minus_zero_two_after_migration() {
    // v1 entry with SuggestionSettled event migrates; v2 query ignores that -0.2
}
```

Stop emitting `SuggestionSettled` from session. Keep enum for old JSON.

- [x] **Step 2: Run — expect FAIL**

- [x] **Step 3: Implement cap in settlement application; impression fields; migration maps old `SuggestionSettled` mass to 0 and `shown_count`. The cap is a non-decaying lifetime settlement budget and remains durable across evidence decay, reload, and idempotency-ID trimming below 24 entries.

- [x] **Step 4: Run** learning_state_machine (update any test that expected -0.2 from live `SuggestionSettled`)

- [x] **Step 5: Commit**

```powershell
git commit -am "feat: cap weak settlement and stop scoring ignored suggestions"
```

---

### Task 19: Lát 5 — capture v2 records + replay invariants

**Files:**
- Modify: `crates/openvikey-session/src/capture.rs` (`CAPTURE_VERSION = 2`)
- Modify: `crates/openvikey-session/src/session.rs` (`record_capture`)
- Test: `crates/openvikey-lab/tests/session_capture.rs`
- Modify: `crates/openvikey-session/src/persistence.rs` if header validation needs v1-or-v2 read

**Interfaces:**
- Consumes: planner output, settlement, forget
- Produces: new record kinds; v1 logs still load via migration or explicit version error

Add:

```rust
pub enum CaptureRecord {
    Input { event: InputEvent },
    AcceptTop { seq: u64, at_ms: i64 },
    RejectTop { seq: u64, at_ms: i64 },
    UndoLast { seq: u64, at_ms: i64 },
    // v2:
    CandidateSetEvaluated { seq: u64, at_ms: i64, ids: Vec<u64>, sources: Vec<CandidateSource>, config_hash: String },
    InterventionApplied { seq: u64, at_ms: i64, edit_id: u64, candidate_id: u64, reason: String },
    InterventionReverted { seq: u64, at_ms: i64, edit_id: u64, kind: String },
    InterventionSettled { seq: u64, at_ms: i64, edit_id: u64 },
    CorrectionConfirmed { seq: u64, at_ms: i64, identity: CorrectionIdentity, left_token_nfc: Option<String> },
    LanguageCommitSettled { seq: u64, at_ms: i64, token: String, left_token: Option<String>, transaction_id: u64 },
    DataForgotten { seq: u64, at_ms: i64, identity: String },
}
```

Minimize text: prefer ids/hashes already in the model. Do not log extra surrounding text.

Privacy contract: `CorrectionConfirmed` necessarily stores plaintext original/candidate/source-rule and optional left token inside the capture JSON payload. Lab encrypts that payload at rest; Windows preview persists it as plaintext `.ovkdev.json` under ADR 0008. Selective Forget removes this explicit mapping and prevents replay resurrection, but does not claim raw replay-command erasure.

- [x] **Step 1: Tests**

```rust
#[test]
fn v2_replay_same_snapshot_and_journal_same_model_hash() { /* */ }
#[test]
fn v1_capture_still_loads_or_errors_clearly() { /* pick one and test it */ }
#[test]
fn forget_record_prevents_row_resurrection_after_restart() { /* */ }
#[test]
fn forgetting_one_identity_selectively_compacts_only_its_v2_records() { /* preserve unrelated journal history */ }
```

Capture v1 intentionally clears the whole journal on forget because its records lack correction identity metadata. Capture v2 must replace that fail-closed fallback with selective compaction by identity.

- [x] **Step 2: FAIL → implement versioned serde (`#[serde(tag="kind")]` already) + reducer**

Reducer must ignore unknown future kinds? No: unknown is error. v1 kinds remain. V2 validates parallel candidate metadata and edit cursors, consumes `DataForgotten`, and fails closed when the recorded config hash is unavailable instead of re-deciding history with a new policy. A loaded v1 pair migrates to a clean v2 journal checkpoint because its paired model snapshot is authoritative and v1 records lack selective-compaction identities.

- [x] **Step 3: Run** lab capture + persistence tests

- [x] **Step 4: Commit**

Post-task closure: `MAX_CAPTURE_RECORDS` is a record-count cap, so candidate arrays shorten the retained input horizon. Retention trimming must discard the complete oldest `seq` group when the cutoff intersects a multi-record transaction; tests must reject orphan `CorrectionConfirmed`/intervention metadata.

```powershell
git commit -am "feat: version capture logs for intervention replay and forget"
```

Lát 5 done when: Backspace is rollback; Undo/reject/settlement match §7; capture v2 replays; impressions do not move confidence.

---

### Task 20: Lát 6 — unigram language model, ranking only

**Files:**
- Create: `crates/openvikey-core/src/user_language.rs`
- Modify: `crates/openvikey-core/src/rank.rs`
- Modify: `crates/openvikey-core/src/lib.rs`
- Modify: `crates/openvikey-session/src/session.rs` (update after **settlement** or user commit, never at Auto apply)
- Test: `crates/openvikey-core/tests/user_language.rs`

**Interfaces:**
- Consumes: NFC tokens; `LearningConfigV2.max_unigrams`
- Produces: bounded `unigram` delta in `ScoreBreakdown`; **cannot** create Auto

- [x] **Step 1: Write failing tests**

```rust
#[test]
fn nfc_and_nfd_share_identity() {
    lang.commit("ơ", None, 0);
    lang.commit("ơ\u{0309}", None, 1); // if NFD form of ơ
    assert_eq!(lang.unigram("ơ"), 2);
}

#[test]
fn a_vs_a_breve_are_distinct() {
    lang.commit("a", None, 0);
    lang.commit("ă", None, 1);
    assert_eq!(lang.unigram("a"), 1);
    assert_eq!(lang.unigram("ă"), 1);
}

#[test]
fn d_and_d_stroke_are_distinct() {
    lang.commit("d", None, 0);
    lang.commit("đ", None, 1);
    assert_ne!(lang.unigram("d"), lang.unigram("đ"));
}

#[test]
fn unigram_can_rerank_but_planner_stays_suggest_without_exact_auto() {
    // two Fuzzy candidates; user language prefers the second; plan.action != Replace
    // unless exact correction already allows Auto
}

#[test]
fn private_mode_does_not_write_unigrams() {
    // allow_learning=false commit → counts unchanged
}
```

Do not update on paste-many, URL/secret-like, composition-only, or unsettled Auto (spec §8.3–8.4). Spec choice: delay language writes until settlement (option 1).

- [x] **Step 2: FAIL → implement `UserLanguageModel` + rank extra `unigram` term clamped**, include in `ScoreBreakdown.unigram`. Planner still requires exact evidence for Replace.

- [x] **Step 3: Run** `cargo test -p openvikey-core --test user_language --test intervention_planner`

- [x] **Step 4: Commit**

```powershell
git commit -am "feat: learn settled unigrams and use them only to rerank suggestions"
```

---

### Task 21: Lát 6 — unigram pruning and inspect/forget word

**Files:**
- Modify: `crates/openvikey-core/src/user_language.rs`
- Modify: `crates/openvikey-core/src/model.rs` (forget-word API)
- Modify: `crates/openvikey-session/src/session.rs` if a public command is needed
- Test: `crates/openvikey-core/tests/user_language.rs`

**Interfaces:**
- Produces: `forget_token("Nam")` vs `forget_correction(X→Y)` as distinct operations

- [x] **Step 1: Tests**

```rust
#[test]
fn prune_when_over_cap_drops_stale_low_count_first() { /* max_unigrams=3 in test */ }
#[test]
fn forget_token_does_not_delete_correction_row() { /* */ }
#[test]
fn forget_correction_does_not_delete_unigram_if_user_typed_the_word() { /* */ }
```

- [x] **Step 2: Implement eviction order spec §8.6. Forget-word API on `AdaptiveModel`.

- [x] **Step 3: Commit**

```powershell
git commit -am "feat: prune unigrams and separate forget-word from forget-correction"
```

Lát 6 done when: unigrams improve ranking only; private mode zero-write; prune deterministic.

---

### Task 22: Lát 7 — one-token bigram + context margin guard

**Files:**
- Modify: `crates/openvikey-core/src/user_language.rs`
- Modify: `crates/openvikey-core/src/rank.rs`
- Modify: `crates/openvikey-core/src/intervention.rs` (margin uses language-adjusted scores; still no Auto from language alone)
- Test: `crates/openvikey-core/tests/user_language.rs`
- Test: `crates/openvikey-core/tests/intervention_planner.rs`

**Interfaces:**
- Consumes: `max_bigrams = 30_000`; left token only
- Produces: `ScoreBreakdown.bigram`; `LowMargin` when top1–top2 too close for Auto

- [x] **Step 1: Write failing tests**

```rust
#[test]
fn after_viet_nam_beats_nam_unrelated() {
    lang.commit("Nam", Some("Việt"), 0);
    lang.commit("Nam", Some("Việt"), 1);
    // rank "Việt" + composition "nam" prefers Nam
}

#[test]
fn bigram_cannot_grant_auto_without_exact_correction() {
    let plan = plan_intervention(/* strong bigram, cold exact memory */);
    assert_ne!(plan.action, InterventionAction::Replace);
}

#[test]
fn small_margin_blocks_learned_auto() {
    // two close scores → Replace forbidden even if exact Auto thresholds pass
    assert_eq!(plan.reason, InterventionReason::LowMargin);
}
```

- [x] **Step 2: Implement bigram counts, backoff (unigram if bigram missing), clamp, prune like unigrams. Planner Auto requires margin from `LearningConfigV2` (add `auto_margin` default matching current implicit uniqueness: treat `top1_top2_margin` below a versioned threshold as LowMargin). Until calibrated, set compatibility threshold so current unique-candidate Auto tests still pass (margin vacuously large when `ranked.len()==1`).

- [x] **Step 3: Run** user_language + intervention_planner + session_capture

- [x] **Step 4: Commit**

```powershell
git commit -am "feat: add left-token bigrams and use margin as an Auto guard"
```

Lát 7 done when: bigrams rerank; they never sole-source Auto; trigram absent.

---

### Task 23: Lát 8 — `ChartSnapshot` golden JSON

**Files:**
- Create: `crates/openvikey-core/src/chart.rs`
- Test: `crates/openvikey-core/tests/chart_snapshot.rs`
- Modify: `crates/openvikey-core/src/lib.rs`

**Interfaces:**
- Consumes: read-only `&CorrectionMemory` + `&LearningConfigV2` + `evaluate_at_ms`
- Produces: `ChartSnapshot` serde JSON byte-identical for same inputs

```rust
pub struct ChartPoint {
    pub at_ms: i64,
    pub confidence: f64,
    pub blended_confidence: f64,
    pub stored_state: DecisionState, // persisted hysteresis/metadata state
    pub state_band: ChartStateBand,  // effective Observe / Suggest / Auto / Cooldown after guards
    pub marker: Option<ChartMarker>, // Accept, Reject, Revert, WeakSettle
}

pub struct ChartSnapshot {
    pub identity: CorrectionIdentity,
    pub points: Vec<ChartPoint>,
    pub compaction_marker_at_ms: Option<i64>,
    pub breakdown: ScoreBreakdown,
    pub conclusion: String, // Vietnamese UI string from planner state, not a formula
    pub config_hash: String,
}
```

- [x] **Step 1: Tests**

```rust
#[test]
fn chart_snapshot_is_byte_identical_for_same_model_config_time() {
    let a = ChartSnapshot::from_memory(&memory, &id, &config, t);
    let b = ChartSnapshot::from_memory(&memory, &id, &config, t);
    assert_eq!(serde_json::to_vec(&a).unwrap(), serde_json::to_vec(&b).unwrap());
}

#[test]
fn confidence_line_matches_model_query_at_each_recent_event() { /* */ }
#[test]
fn migrated_stored_auto_is_presented_as_effective_suggest_when_guards_fail() { /* */ }
#[test]
fn compaction_inserts_checkpoint_marker_without_changing_final_confidence() { /* */ }
#[test]
fn physical_forget_makes_chart_none() { /* */ }
#[test]
fn snapshot_contains_no_left_context_strings_by_default() { /* privacy */ }
```

- [x] **Step 2: Implement from summary + ≤64 recent events. No capture-log read. No hook-path build: this is a Settings/idle API.

- [x] **Step 3: Commit**

```powershell
git commit -am "feat: build local ChartSnapshot from correction memory"
```

---

### Task 24: Lát 8 — Settings Learning chart (GDI + text alternative)

**Files:**
- Create: `crates/openvikey-win/src/chart_view.rs`
- Modify: `crates/openvikey-win/src/control.rs` (Learning page: timeline, breakdown bars, overview counts)
- Modify: `crates/openvikey-win/src/host.rs` (`ControlSnapshot` grows `chart: Option<ChartSnapshot>` + overview counters, filled off hook path)
- Test: `crates/openvikey-win/tests/learning_chart.rs`
- Test: `crates/openvikey-win/tests/ui_rules.rs` (learned rules still hide surrounding context)

**Interfaces:**
- Consumes: `ChartSnapshot`
- Produces: native chart + accessible text table; no web runtime

- [x] **Step 1: Tests**

```rust
#[test]
fn selecting_a_rule_exposes_chart_snapshot_and_text_alternative() {
    // bind_runtime / control_snapshot; select row; assert text alt contains
    // "Gợi ý" or "Đang quan sát" and breakdown labels in Vietnamese
}

#[test]
fn overview_counts_match_effective_chart_states_not_raw_stored_states() { /* Observe/Suggest/Auto/Cooldown */ }

#[test]
fn chart_build_is_not_invoked_from_inject_path() {
    // typing inject must not call ChartSnapshot::from_memory; assert via a test
    // double or by keeping chart build only in control_snapshot refresh
}
```

UI copy (spec §15.1 / §15.4): Vietnamese state names, markers `+` `−` `↩`, breakdown bars. “Chi tiết kỹ thuật” can show mass/half-life/hash. No default surrounding context.

- [x] **Step 2: Implement GDI owner-draw (existing `control.rs` patterns). Text alternative is a read-only static or list for accessibility/tests.

- [x] **Step 3: Run** `cargo test -p openvikey-win --test learning_chart --test ui_rules`

Cannot fully click-paint in this environment; tests cover snapshot + text alt + “not on hook path”. Note that live GDI paint needs a manual Settings look after merge.

- [x] **Step 4: Commit**

```powershell
git commit -am "feat: show local learning charts on the Settings Learning page"
```

Lát 8 done when: golden JSON stable; Settings shows timeline + breakdown + overview; hook path unchanged.

---

### Task 25: Lát 9 — flip locked product policy defaults

**Files:**
- Modify: `crates/openvikey-core/src/learning_config.rs` (`product_v2()`, make it Default after reports)
- Modify: session default config wiring (`LabSession` / win host)
- Test: invert `abbrev_boundary_assist_replaces_on_space_without_accept_mass` to Suggest-only at cold start
- Test: invert `fuzzy_boundary_assist_replaces_unique_typo_on_space` to Suggest unless `fuzzy_heuristic_assist`
- Keep TelexFix structural Auto tests green
- Add: `crates/openvikey-lab` report path using existing `metrics.rs` (separate Structural / Heuristic / Learned Auto / Suggest)

**Interfaces:**
- Consumes: owner-locked §20.2–20.3
- Produces: `LearningConfigV2::product_v2()` with `abbrev_cold_start_auto=false`, `fuzzy_heuristic_assist=false`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn product_v2_abbrev_ko_space_does_not_replace_without_evidence() {
    let mut session = LabSession::new_with_learning_config(
        EngineConfig::default(),
        khong_lexicon(),
        LearningConfigV2::product_v2(),
    );
    type_keys(&mut session, "ko", 0);
    let last = session.inject(InputKind::Boundary { delimiter: ' ' }, InputContext::default(), 10);
    assert!(!matches!(last.action, Some(EngineAction::ReplaceRange(_))));
}

#[test]
fn product_v2_fuzzy_khogn_is_suggest_without_heuristic_flag() { /* */ }

#[test]
fn product_v2_telex_fix_chfao_still_replaces() { /* SafeStructuralFix */ }

#[test]
fn product_v2_abbrev_replaces_after_personal_evidence() {
    // enough exact accepts on ko→không then Space may Replace LearnedCorrection
}
```

- [ ] **Step 2: FAIL → switch host/session default to `product_v2()`. Keep `compatibility_v1()` for replay of old characterization if needed.

- [ ] **Step 3: Add lab report command or test that writes config hash + per-kind counts** into `target/evaluation-learning-v2.json`. Do not claim G3 corpus floors if they are still unmet; default high-risk Fuzzy heuristic stays **off**.

- [ ] **Step 4: Run workspace tests; update any remaining Assist tests to opt into `compatibility_v1()` explicitly.

- [ ] **Step 5: Commit**

```powershell
git commit -am "feat: ship learning v2 product policy with safer Auto defaults"
```

Lát 9 done when: Abbreviation/Fuzzy cold Auto match locked policy; TelexFix still structural Auto; reports include config hash.

---

### Task 26: Lát 10 — generalized error model, observation only

**Files:**
- Create: `crates/openvikey-core/src/generalized_error.rs`
- Modify: `crates/openvikey-core/src/lib.rs`
- Test: `crates/openvikey-core/tests/generalized_error.rs`
- Modify: session to record observations from **strong intent only** (explicit accept, confirmed delete/retype), never from ignored suggestion, weak settle, or immediate Backspace

**Interfaces:**
- Consumes: spec §14
- Produces: `GeneralizedErrorModel` stats that **do not** enter `plan_intervention` or `rank`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn transposition_pattern_is_recorded_from_explicit_accept_only() {
    // khogn→không accept with unique alignment → count transposition op
}

#[test]
fn observation_does_not_change_ranked_order_or_plan_action() {
    let before = plan_intervention(...);
    model.observe_error_pattern(...);
    let after = plan_intervention(...);
    assert_eq!(before, after);
}

#[test]
fn ignored_suggestion_does_not_train_error_model() { /* */ }

#[test]
fn immediate_backspace_does_not_train_error_model() { /* */ }
```

- [ ] **Step 2: Implement counters keyed by operation class (transpose, adjacent, extra key, early tone). Cap influence at 0. Shadow/offline inspect via lab dump. **No Auto, no Suggest from this model in v2.**

- [ ] **Step 3: Commit**

```powershell
git commit -am "feat: record generalized typing-error patterns in observe-only mode"
```

Lát 10 done when: patterns record from strong intent; planner/rank unchanged; no bandit exploration.

---

### Task 27: Workspace verification against spec definition of done

**Files:** none new unless a DoD test is missing

- [ ] **Step 1: Run the full gate**

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test -p openvikey-core --test golden_engine
cargo test -p openvikey-core --test learning_state_machine
cargo test -p openvikey-core --test intervention_planner
cargo test -p openvikey-core --test correction_memory_v2
cargo test -p openvikey-core --test model_migration_v2
cargo test -p openvikey-core --test user_language
cargo test -p openvikey-core --test chart_snapshot
cargo test -p openvikey-core --test physical_forget
cargo test -p openvikey-core --test generalized_error
cargo test -p openvikey-lab --test session_capture
cargo deny check
```

- [ ] **Step 2: Check spec §19 point-by-point** (see coverage table below). If a box is empty, add the missing test in the matching lát, do not skip.

- [ ] **Step 3: Commit only if verification produced doc/test-name index updates**

```powershell
git commit -am "test: confirm learning v2 definition of done"
```

---

## Spec coverage (self-review)

| Spec | Task |
|---|---|
| §0 / §20 eleven defaults | Task 0, 9, 10, 17, 18, 20–22, 25 |
| §4.3 safety outside model | Task 5 UnsafeContext, Task 8 |
| §4.6 / §9.2 min 2 graphemes | Task 9 |
| §5 four-layer pipeline | Tasks 5–8, 13, 20–22 |
| §6 identity + blend + compact + Personal | Tasks 13–16 |
| §7 feedback table, RevertGuard, settlement, impression | Tasks 10, 17, 18 |
| §8 unigram/bigram ranking-only | Tasks 20–22 |
| §9 planner + reasons + source policy | Tasks 5–8, 25 |
| §10 versioned config + calibration | Tasks 5, 25 |
| §11 payload v2, migration, physical forget, caps | Tasks 11, 12, 15 |
| §12 capture v2 + replay | Task 19 |
| §13 privacy / no token logs | Tasks 11, 19, 23 |
| §14 generalized error observe-only | Task 26 |
| §15 UI copy + chart | Tasks 23–24 |
| §16 tests listed | Tasks 1–4, 9–10, 13–26 |
| §17 lát order | Task order 0 → 26 |
| §18 ADR 0002 engine freeze | Task 0 ADR 0011; engine never in file map |
| §19 DoD | Task 27 |
| No NN / no cloud / no GPL copy | Global constraints |

## Type consistency

- `plan_intervention` signature is defined in the file map and used in Tasks 5–10, 18, 20, 22, 25, 26.
- `LearningConfigV2::compatibility_v1` is default through Lát 8; `product_v2` from Task 25.
- `RevertGuard` is session-only (Task 10); not in payload v2.
- `CorrectionIdentity` is introduced conceptually in the file map, implemented in Task 13, used by forget/capture/chart.
- `FeedbackKind::SuggestionSettled` is never removed; runtime stops emitting it in Task 18.
- `CorrectionSlice.plan` is added in Task 8.

## Execution notes

- Work in a git worktree if executing with subagents (`superpowers:using-git-worktrees`).
- TDD is mandatory: failing test first for every task that changes code.
- Do not implement Lát 9 policy flips early “because they are accepted” — characterization and planner compatibility depend on the old flags until Task 25.
- Do not implement language-model Auto or generalized-error Suggest. Those are spec non-goals.

**Plan complete.** Two execution options after this plan is saved into `docs/superpowers/plans/2026-08-21-openvikey-learning-model-v2-implementation-plan.md`:

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks
2. **Inline Execution** — execute in this session with checkpoints
