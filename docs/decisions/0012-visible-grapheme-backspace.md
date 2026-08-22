# ADR 0012: Backspace deletes one visible grapheme

- **Date:** 2026-08-22
- **Status:** Accepted
- **Supersedes:** ADR 0001 raw-key-pop Backspace semantics only

## Context

ADR 0001 used `raw_keys.pop()` because `vi-rs` has no native Backspace API. That is deterministic and fast, but it exposes input-method internals to the user. For example, Telex `keer` and VNI `ke63` both render `kể`; popping the final `r`/`3` renders `kê`, so the first Backspace removes only the tone instead of the visible grapheme `ể`.

The Windows host already synchronizes text by Unicode extended grapheme clusters. Engine Backspace must therefore have the same user-visible unit.

## Decision

1. One `InputKind::Backspace` on an active composition removes exactly one rendered Unicode extended grapheme cluster.
2. The engine removes the smallest deterministic subset of raw input keys whose replay matches the visible prefix. A canonically equal replay wins; an accent-folded replay is accepted for incomplete Vietnamese prefixes that `vi-rs` cannot represent exactly (for example transient `đườ`).
3. The visible prefix is authoritative after Backspace. Raw keys remain the best replayable input history and normal `vi-rs` recomputation resumes on the next typed key.
4. Empty-composition Backspace, policy Auto rollback, committed-token reopening, and host pass-through behavior are unchanged.
5. No public `InputKind`, `EngineAction`, or snapshot field changes.

## Consequences

- `kể` becomes `k` on the first Backspace in both Telex and VNI.
- `đường` deletes as `đườn → đườ → đư → đ → ""`, preserving tone and vowel modifiers that belong to the visible prefix.
- Backspace replay now evaluates a bounded number of small raw-key subsets. The existing sub-5 ms engine budget remains a required test gate.
- During an incomplete Vietnamese prefix, `snapshot.rendered` may intentionally be more faithful than replaying `snapshot.raw_keys` through `vi-rs`; the next ordinary key recomputes from the retained raw history.
