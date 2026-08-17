# OpenViKey Part 2 — Personal Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Add a headless personal capture-and-learn reducer plus `openvikey-lab session` raw-mode REPL so a user model learns from real typing, persists encrypted across restart, and replays deterministically.

**Architecture:** All new logic lives in `openvikey-lab`. `LabSession` owns a `DocumentBuffer` (engine forgets committed tokens). Implicit learning uses candidate snapshots stored at commit. Capture logs command records and replays them through the same reducer. Crossterm is a thin adapter.

**Tech Stack:** Rust 1.96, `openvikey-core` APIs as-is, `crossterm` (lab only), existing `FileModelStore` + `DebouncedSaver`.

**Spec:** [`../specs/2026-08-17-openvikey-part2-personal-capture-design.md`](../specs/2026-08-17-openvikey-part2-personal-capture-design.md)

## Global Constraints

- Do not modify `crates/openvikey-core/**`.
- `openvikey-lab type` stdin JSONL stays; `session` is additive.
- G3 stays open; capture is not release evidence.
- No PII filter, no plaintext export, no network crate.
- Crossterm: `deny.toml` only — do not add `data/provenance.toml`.
- Core never reads wall-clock; lab stamps `at_ms` and stores it.
- No UniKey GUI, tray, TSF, or full-screen TUI.

## Locked design

1. **Undo revision:** `AutoEditContext.range.revision` = engine `snapshot.revision` at Auto. `undo_last` passes that stored value, not a second document counter.
2. **Candidate snapshot at commit:** `CommittedUnit` stores `{ token_nfc, delimiter, original_nfc, left_token_nfc, input_method, candidates }`. Implicit `RuleContextKey` rebuilt from those fields + `source_rule_id = evidence.split('+').next()`.
3. **Capture commands:** `Input | AcceptTop | RejectTop | UndoLast`. Replay those through the reducer. `AutoSettled` is regenerated, not re-applied.
4. **Auto only at commit:** `AutoEditContext` only when this event produced a non-empty `Commit`. Mid-composition `auto_edit: None`.
5. Space and Enter both `Boundary { delimiter: ' ' }`.
6. T1 fixture: seed abbrev `ko` → `không`. Do not use `teh→the`.
7. Auto tests (T2/T2b): VNI `paht1` → `phát` (cold `final_score >= 0.90`) with 18 seeded accepts. Abbrev `ko` maxes at ~0.89 after rerank and cannot Auto.
8. Missing model file: `exists()` then default. Wrong passphrase: hard error. Model present + log missing: hard error.
9. SuggestionSettled and in-composition retype are out of Part 2. Undo log is not persisted.

## File map

Create: `crates/openvikey-lab/src/document.rs`, `capture.rs`, `repl.rs`; `tests/session_capture.rs`, `tests/repl_keymap.rs`; `docs/decisions/0006-wave5-lab-session-capture.md`.

Modify: `session.rs`, `cli.rs`, `lib.rs`, lab + workspace `Cargo.toml`, `cli_smoke.rs`, `deny.toml` only if needed, `README.md`.

---

### Task 1: DocumentBuffer grapheme pop

**Files:**
- Create: `crates/openvikey-lab/src/document.rs`
- Test: `crates/openvikey-lab/tests/session_capture.rs` (document unit tests can live at top of this file via `openvikey_lab::document`)
- Modify: `crates/openvikey-lab/src/lib.rs`

**Produces:** `DocumentBuffer`, `CommittedUnit`, `pop_grapheme() -> Option<PopOutcome>`

- [x] Write failing tests: push `ko` + space; first pop removes delimiter; subsequent pops shrink token; first token shrink reports `started_deleting: "ko"` once; empty pop is None.
- [x] Run `cargo test -p openvikey-lab --test session_capture -- document` — fail (module missing).
- [x] Implement `DocumentBuffer` using grapheme clusters (`unicode-segmentation` workspace dep on lab).
- [x] Tests pass. `session_live` still compiles.

### Task 2: LabSession owns document + empty-composition Backspace

**Files:** Modify `session.rs`. Test: `session_capture.rs`.

**Produces:** `LabSession::document_text()`, `inject(kind, context, at_ms)`, document updates on Commit and Backspace-when-empty.

- [x] Test: type `ko` + space → `document_text()` is `"ko "`; three backspaces on empty composition clear it; mid-composition backspace does not pop document.
- [x] Fail, then wire Commit → `document.push` and empty Backspace → `document.pop_grapheme`. Keep `type_text` using `Key` for space (engine still Commits). `left_context.prev_token_nfc` = last remaining document token (or None).
- [x] `cargo test -p openvikey-lab --test session_live` stays green.

### Task 3: Implicit mining T1 (B2/B5)

**Produces:** miner + commit-time candidate snapshot + `apply_feedback` with rebuilt `rule_key`.

- [x] Test T1: `ko` + space, pop token, `InsertText("không")` (avoids Telex ambiguity), space/commit → `positive_mass` on Abbreviation `ko→không` / `seed:ko` >= 1.0.
- [x] Test: after `ko` commit, pop, `InsertText("xyz")` commit → mass stays 0 (Y not in snapshot).
- [x] Test: after `ko` commit, `CursorMoved`, pop + retype `không` → mass 0.
- [x] Implement: snapshot candidates/`original_nfc`/`left_token`/`input_method` onto `CommittedUnit` at commit; `record_deleted_token(full X)` once on first token shrink; `finish_replacement(Y)` on next non-empty Commit; match `candidate.text`; `model_mut().apply_feedback`. Caret/Reset invalidate miner + `learning.invalidate_due_to_caret_break()`.

### Task 4: Capture Input + T5 + T4 replay

**Files:** Create `capture.rs`. Modify `session.rs`.

**Produces:** `CaptureRecord`, `CaptureLog`, `drain_capture`, `replay`.

- [x] T5: `allow_transform=false, allow_learning=false` → no candidates, model payload unchanged, capture empty.
- [x] T4: replay Input records of T1 through a fresh `LabSession::new` → `to_json_payload()` SHA-256 equal.
- [x] Sensitive events are not captured. `observe_input_or_edit` not required until Wave 6.

### Task 5: Auto at commit + accept/reject/undo (T2, T2b)

**Produces:** `AutoEditContext` on non-empty Commit; `accept_top` / `reject_top` / `undo_last`; `observe_input_or_edit` every allowed event.

- [x] Seed 18 Accepts on the live fuzzy key for VNI `paht1`→`phát`, then Boundary-commit → decision Auto, document token is replacement, `undo_last(snapshot.revision)` restores original and adds undo mass.
- [x] `accept_top` on composing `ko` writes `không` into document, clears composition (`Reset`), Accept mass on abbrev key.
- [x] `reject_top` adds ExplicitReject mass, document unchanged.
- [x] T2b: after Auto, 10 subsequent injects emit one AutoSettled (+0.3 once); `CursorMoved` cancels pending (no settle).

### Task 6: Persist + cursor restore (T3) + full T4

**Produces:** `SessionCursors`, `new_with_model`, `CaptureHeader { v, next_seq, next_edit_id }`, two `DebouncedSaver` helpers used by CLI later; tests flush immediately via `FileModelStore` + `PassphraseProvider::new_for_testing`.

- [x] T3: session A implicit-learns, save model+log, `new_with_model` with header cursors, another implicit or accept with new seq increases mass (not deduped).
- [x] Missing log while model exists → error type, no guessed cursor.
- [x] T4 with AcceptTop/UndoLast commands matches live payload hash.

### Task 7: Keymap T6

**Files:** Create `repl.rs`, `tests/repl_keymap.rs`. No TTY.

- [x] Map printable → `Key`, Backspace, Enter/Space → `Boundary(' ')`, Tab AcceptTop, Esc RejectTop, Ctrl+Z UndoLast, Ctrl+C/D Quit.
- [x] Fail then implement pure `key_event_to_action`.

### Task 8: CLI session

**Files:** `cli.rs`, `cli_smoke.rs`, workspace + lab `Cargo.toml` (`crossterm`).

**Produces:** `openvikey-lab session --method --lexicon --model --capture --context`.

- [x] Help lists `session`. Piped/non-TTY exits ≠ 0 with message containing `terminal`.
- [x] Prompt passphrase before entering the key loop; two encrypted savers; flush on quit. `type` command unchanged.

### Task 9: Deny, ADR 0006, README, T7

- [x] `cargo deny check` (add allowlist only if a new transitive license appears). No provenance.toml row for crossterm.
- [x] ADR 0006: session REPL + capture envelope + document buffer; does not replace ADR 0005.
- [x] README: how to run `session`; not a system IME / no tray.
- [x] Verify: `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace --all-features`; `cargo deny check`.

## Wave mapping

| Wave | Tasks | Done when |
|------|-------|-----------|
| 5 | 1–4 | T1, T4 (input), T5; `session_live` green |
| 6 | 5–6 | T2, T2b, T3 |
| 7 | 7–8 | T6; CLI session |
| 8 | 9 | T7; ADR 0006 |

## Out of scope

OS hook / TSF / tray / UniKey window, full-screen TUI, PII sanitize/export, G3 corpus, `types.rs`/`engine` changes, SuggestionSettled, plaintext leaving the machine.
