# ADR 0009: Personal candidate source and additive engine restore

- **Date:** 2026-08-18
- **Status:** Accepted
- **Gate:** Frictionless learning lát 1+2

## Context

ADR 0002 froze `types.rs` and `engine/` against casual reshapes. Composition-rewind learning needs a candidate source for user-taught pairs that are not TelexFix/Fuzzy/Abbreviation/Diacritics, and TelexFix policy undo needs to put original `raw_keys` back into the engine without inventing a new `InputEvent`.

## Decision

1. Add `CandidateSource::Personal` to `types.rs`. `max_action` is `Suggest` (same cap as Diacritics). Personal pairs never auto-apply in this milestone.
2. Add `Engine::restore_raw_keys(&str)`: replace the composition buffer, recompute `rendered` via existing `transform_raw`, bump `revision`. No new `InputEvent` variant. Generators, model, and store stay out of the engine.
3. Do not add fields to `InputContext`. Intervention policy (`telex_fix_policy_auto`, delimiter set) lives on the session.

## Consequences

- Exhaustive `match` on `CandidateSource` must handle `Personal`.
- Old model JSON has no Personal entries; serde of existing keys is unchanged.
- Undo-to-composition uses `restore_raw_keys` instead of leaving a committed leftover token.
