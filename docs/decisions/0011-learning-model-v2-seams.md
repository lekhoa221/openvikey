# ADR 0011: Learning model v2 seams

- **Date:** 2026-08-21
- **Status:** Accepted
- **Gate:** Learning model v2 (spec `2026-08-20-openvikey-learning-model-v2-design.md` v1)
- **Plan:** [`../superpowers/plans/2026-08-21-openvikey-learning-model-v2-implementation-plan.md`](../superpowers/plans/2026-08-21-openvikey-learning-model-v2-implementation-plan.md)

## Context

ADR 0002 freezes `types.rs` meanings and the Telex/VNI engine. ADR 0003 locked v1 adaptive evidence, `LearningSession` undo, and encrypted persistence. The v2 spec needs a single intervention planner, payload version 2, capture additions, and read-only model query growth without silently reinterpreting v1 JSON or putting a model into generators.

## Decisions

1. **Engine and existing `types.rs` meanings stay frozen.** Do not change `InputEvent`, `CompositionSnapshot`, `EngineAction`, or existing `FeedbackKind` variant meanings. New `InputContext` fields need a later ADR.

2. **New core modules are allowed:** `learning_config`, `intervention`, and later `correction_memory`, `user_language`, `chart`, `generalized_error`. `correction.rs` remains the generate → rank → planner orchestrator.

3. **`AdaptiveModel` payload version 2 is a real schema bump.** Do not use `#[serde(default)]` to give v1 bytes v2 meaning. Migration is deterministic, writes a v1 `.bak` via the existing store, and must not raise intervention rights.

4. **Capture may add record kinds behind a capture version bump.** Unknown kinds are errors. v1 kinds remain readable.

5. **`ModelView` may grow read-only query methods** (`blended_confidence`, score-breakdown inputs) without changing v1 `confidence` / `positive_mass` / `state` meaning for Lát 1–3 callers.

6. **`FeedbackKind::SuggestionSettled` stays deserializable.** Runtime stops emitting it (Lát 5). Prefer existing kinds for immediate revert / explicit undo / confirmed reject rather than adding variants.

7. **Generators stay model-free. OS adapters stay decision-free.** Only `plan_intervention` may choose None / Suggest / Replace. `InterventionPlan.model_transition` is the only signal that may be persisted; `action == None` is not `DecisionState::Ignore`.

8. **`minimum_correction_graphemes = 2` is a product invariant** from Lát 2. Settings must not expose a control that lowers it.

9. **`plan_intervention` query identity.** Besides the ranked list, the planner takes `input_method: InputMethod` and `left_token_nfc: Option<&str>` so it can build `RuleContextKey` without changing frozen `CompositionSnapshot`.

## Consequences

- Lát 1 can land a planner without changing payload version.
- Lát 4 owns the schema migration.
- Tests prove explicit correction identity strings are absent after Forget and replay cannot resurrect the row. Forget one correction remains distinct from deleting raw typing history.
- A follow-up ADR is required before adding `FeedbackKind` variants or `InputContext` capability bits.

## Out of scope

Neural nets, cloud accounts, telemetry, trigrams, app/domain learning in the first v2 schema, and making the generalized error model change typed text.
