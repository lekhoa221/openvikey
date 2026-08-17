# ADR 0003: Wave 2 deterministic learning and encrypted persistence

- **Date:** 2026-08-17
- **Status:** Accepted
- **Gate:** Wave 2 (M6, 7A, M8)

## Context

Wave 2 must add adaptive evidence, undo, raw-key correction, and restart-safe encrypted persistence without introducing wall-clock reads, network access, OS keyring coupling, or model knowledge into generators/store.

## Decisions

1. `AdaptiveModel` stores non-negative evidence events by `RuleContextKey`. Beta confidence and 30-day half-life decay are computed at caller-provided `evaluate_at_ms`; negative age clamps to zero.
2. Explicit accept and implicit correction add `+1.0`; explicit reject adds `-1.0` as negative Beta mass; auto settle adds `+0.3`; suggestion settle adds negative `0.2`; undo adds negative `1.5`. Settled IDs and feedback sequence IDs are idempotent per rule.
3. A rule starts `Ignore`, acceptance makes it `Suggest`, 18 canonical accepts satisfy promotion, and two undone edits in the latest ten auto emissions force `Suggest`.
4. `LearningSession` owns the bounded semantic undo log. It emits an exact inverse `ReplaceRangeAction` plus `FeedbackEvent::Undo`; caret/selection breaks invalidate both undo and implicit correction mining.
5. Personal rerank is neutral at Beta(1,1) and applies a versioned bounded delta around confidence `0.5`.
6. `TelexFixGenerator` is pure and explicitly configured for Telex or VNI. It moves misplaced tone modifiers only when the remaining raw letters match a bounded Vietnamese syllable shape.
7. Model files use a random 32-byte DEK with XChaCha20-Poly1305. The immutable payload header is AAD. Passphrase slots wrap the DEK independently with Argon2id + XChaCha20-Poly1305, so rewrap leaves payload bytes unchanged.
8. Production Argon2id defaults are RFC 9106 low-memory (`64 MiB, t=3, p=4`). A deliberately weaker profile is public only for unit tests.
9. `FileModelStore` writes and syncs a sibling temp file, preserves the prior envelope as `.bak`, replaces the primary, and falls back to the backup only for corruption/I/O—not for a wrong passphrase.
10. Passphrases, KEKs, and DEKs are zeroized where owned; debug output redacts secret material.

## Consequences

- M9 can persist `AdaptiveModel::to_json_payload()` without the store importing model types.
- OS keyring wrappers remain deferred; `SecretProvider` can add them without changing the envelope payload.
- Lab remains responsible for debounce/background scheduling; core file APIs are synchronous and must stay off the typing path.

## Review closure amendment

Wave 2 closure review tightened the operational contract:

- Read-only correction degrades an otherwise-auto decision to `Suggest` when no semantic edit payload exists. Adaptive correction accepts caller-owned `edit_id`/`EditRange`, validates the snapshot revision and grapheme length, emits `ReplaceRange`, and records the same action in `LearningSession` for exact inverse undo.
- `InputContext.allow_learning` gates decision persistence, auto-emission history, settlement scheduling, and feedback mutation. It does not disable the semantic undo log for an allowed transform.
- Two recent auto undos create an auto-promotion block independent of aggregate confidence. Re-promotion requires 18 decayed positive mass accumulated after demotion.
- `LearningSession` emits `AutoSettled` exactly once after ten subsequent allowed input/edit events; undo or caret invalidation cancels the pending settlement.
- Diacritics source caps are enforced both by decision and model mutation seams. Operational decision transitions are persisted to preserve hysteresis across correction calls.
- Normal passphrase providers reject persisted or configured Argon2id parameters below the OWASP floor (`19 MiB / t=2 / p=1`). Weak parameters require the explicit test provider constructor.
- Wrapped-key debug output is redacted and buffers zeroize on drop. File replacement never performs a Windows delete-before-rename step.
