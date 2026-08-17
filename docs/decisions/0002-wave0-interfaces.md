# ADR 0002: Wave 0 locked core seams

- **Date:** 2026-08-17
- **Status:** Accepted
- **Gate:** Wave 0 (implementation plan §4.1)

---

## 1. Context

Milestones 4–10 will be built by separate groups. They share `types.rs` and `engine/` and must not reshape those modules “khi làm M5”. Wave 0 locks the remaining seams as small interfaces; later milestones fill implementations behind them.

## 2. Frozen after this ADR

- `crates/openvikey-core/src/types.rs` — `InputEvent`, `CompositionSnapshot`, `EngineAction`, `Candidate`, `FeedbackEvent`, `UndoTracker`. Add fields only with a new ADR; do not change meaning.
- `crates/openvikey-core/src/engine/` — Telex/VNI composition via `vi-rs` (ADR 0001). Correction, model, and store stay out of the engine.

Engine policy locked here:

- Punctuation and whitespace are word boundaries (`backend::is_boundary_char`) and emit `Commit`. A URL such as `https://example.com` is **not** one composing token.
- English / URL heuristics are **not** an engine concern in v1. `allow_transform=false` is the passthrough switch.
- NFC is the matching form (`CompositionSnapshot.normalized`). `rendered` and `raw_keys` stay as produced; undo uses `original`.

## 3. Locked seams (signatures only)

| Seam | Module | Interface | Later fill |
|---|---|---|---|
| Lexicon | `lexicon.rs` | `from_entries` / `lookup` / `bigram` / `source_manifest_hash` | M4 artifact from manifest |
| Generator | `generate/mod.rs` | `generate(&CompositionSnapshot, &LeftContext) -> Vec<Candidate>` | M5 abbrev, M7 others |
| Decision | `decision.rs` | `DecisionState`, `ActionCap`, `DecisionConfig` defaults | M5/M6 state machine |
| Model view | `model.rs` | `ModelView::{confidence,state}`; `EmptyModel` JSON `{version,entries}` | M6 event-backed Beta |
| Store | `store/mod.rs` | `SecretProvider`, `ModelStore` on `&[u8]`; `StoreError` kinds | M8 envelope crypto |
| Metrics | `openvikey-lab/src/metrics.rs` | `auto_precision`, `correct_token_fpr`, `wilson_interval` | M4/M9 corpus evaluate |

Invariants callers may rely on:

1. **Generators never receive a model.** Rank/decision read `ModelView`.
2. **Diacritics `max_action` is `Suggest`.** Other sources may reach `Auto`.
3. **Empty model** is Beta(1,1) prior (`confidence = 0.5`) and `DecisionState::Ignore`. `apply_feedback` is a no-op until M6.
4. **Store persists bytes**, never `EmptyModel` / Beta types. Decode is the caller's job.
5. **Precision denominator** is `TP+FP`. **Correct-token FPR denominator** is `total_correct_tokens`. Wilson 95% uses z = 1.96.

`DecisionConfig` defaults match spec §6.3: `suggest_on=0.70`, `suggest_off=0.60`, `auto_score=0.90`, `auto_confidence=0.95`, `promote_positive_mass=18`.

## 4. Out of scope

No production lexicon file, no abbrev ranking, no Argon2/XChaCha, no lab `corpus evaluate` CLI. Those remain M4–M10.

## 5. Consequences

Wave 1 may start: group A implements M4 behind `Lexicon` + metrics; group B implements M5 behind `Generator` + `DecisionConfig` + `EmptyModel`.
