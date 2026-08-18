# OpenViKey GĐ2 — Master plan (ba plan nhỏ)

> **For agentic workers:** This file is the **index**, not an executable TDD plan. Execute **GĐ2a** via [`2026-08-18-openvikey-gd2a-implementation-plan.md`](./2026-08-18-openvikey-gd2a-implementation-plan.md) (subagent-driven-development or executing-plans). Write GĐ2b/GĐ2c specs+plans only when that pha starts.

**Goal:** Ship a Windows host in three independently testable slices so the user can type Telex in daily apps (including Cursor Agent prompts) without waiting for a full TSF IME.

**Architecture:** Hybrid host — hook is the typing path; TSF is added later for context then for per-app primary input. Brain stays `openvikey-core` + `openvikey-session`.

**Tech Stack:** Rust 1.96, `windows` crate (2a), later a TSF COM DLL (2b/2c). MIT only.

**Spec:** [`../specs/2026-08-18-openvikey-gd2-windows-host-design.md`](../specs/2026-08-18-openvikey-gd2-windows-host-design.md)

## Global Constraints

- Do not copy GPL/AGPL IME code (VKey, OpenKey, PHTV, espanso).
- Do not modify `crates/openvikey-core/src/engine/**`, `types.rs`, or `model.rs`.
- Tab/Esc always pass through to the application.
- G3 corpus and PII export stay out of GĐ2.
- `unsafe` only in `openvikey-win` (and later TSF DLL), never in core/session.
- Two IMEs at once (UniKey + OpenViKey) is unsupported.

---

## How the three plans relate

```text
Plan 2a  ──implements──►  spec gd2a-hook-electron
Plan 2b  ──implements──►  spec gd2b-tsf-context     (not written yet)
Plan 2c  ──implements──►  spec gd2c-tsf-primary     (not written yet)

2b must not start until 2a manual smoke (Notepad + Cursor) is accepted.
2c must not start until 2b InputScope tests are green.
```

Each plan produces a **runnable** `openvikey-win` (2b/2c add a DLL next to it). No “big bang” merge of hook+TSF+UWP.

| Plan | Spec | Implementation plan | Runnable outcome |
|---|---|---|---|
| **2a** | [`../specs/2026-08-18-openvikey-gd2a-hook-electron-design.md`](../specs/2026-08-18-openvikey-gd2a-hook-electron-design.md) | **This folder:** `2026-08-18-openvikey-gd2a-implementation-plan.md` (full TDD) | `openvikey-win` types into Notepad + Cursor via hook |
| **2b** | *write at start of pha* `YYYY-MM-DD-openvikey-gd2b-tsf-context-design.md` | *write after that spec* `...-gd2b-implementation-plan.md` | Same exe; TSF DLL **read-only**; hook still types |
| **2c** | *write at start of pha* `YYYY-MM-DD-openvikey-gd2c-tsf-primary-design.md` | *write after that spec* `...-gd2c-implementation-plan.md` | Per-exe TSF primary input; hook skipped for those processes |

---

## Plan 2a — looks like (locked, execute now)

**Done when:** unit tests in spec §10 green; lab `session_capture` still green; README has run + keymap + UniKey-off; manual Notepad + Cursor composer Telex `xin chao`.

**File map:** extract `openvikey-session`; add `openvikey-win` (`policy`, `sync`, `inject`, `classify`, `hook`, `overlay`, `tray`, `host`).

**Task shape (detail in the 2a plan v3):** (1) extract session crate, (2) policy including OVK Pass / Backspace / Ctrl+C / Caps / Enter `CommitAndPass`, (3) sync from `obs.action` + punctuation delimiter, (4) `SendingGuard` + `Arc<AtomicBool>` + partial SendInput, (5) classify, (6) host + AcceptVisual/Undo + `SessionSaveSnapshot` + HWND, (7) hook return semantics, no key queue, (8) mouse + focus cache, (9) overlay + tray, (10) passphrase + persist + main, (11) deny + `no_key_log` includes `host.rs` + manual Notepad/Cursor gate. P95 is `#[ignore]`, not CI.

**Not in 2a:** TSF, InputScope, bait char, autostart, Authenticode.

---

## Plan 2b — will look like (do not execute yet)

**Goal:** Password/PIN fields do not get Telex; `left_context` can use surrounding text when TSF exposes it.

**Architecture:** Register a TSF text service DLL that **does not** commit composition as the default path. It publishes InputScope + a small surrounding-text snapshot over IPC/shared memory to `openvikey-win`. Hook remains the typer. Credit VietType/VKey for InputScope *idea* only.

**Likely files:** `crates/openvikey-win-tsf/` (C++ or `windows` COM), `src/context_bridge.rs` in win, tests with mock `ITfContext`.

**Likely tasks:** (1) InputScope mock → flags, (2) DLL register/unregister documented, (3) wire flags into `InputContext` before `inject`, (4) surrounding token → `LeftContext`, (5) manual Chrome password, (6) ADR 0008.

**Done when:** denylist 2a still works; password InputScope test forces `allow_transform=false`; no double typing.

**Out:** TSF as the key source; UWP primary; changing Tab/Esc.

---

## Plan 2c — will look like (do not execute yet)

**Goal:** A user-managed list of executables uses TSF as **primary** input (UWP / anti-cheat), including Windows composition underline.

**Architecture:** Coordinator: if foreground exe is on the TSF-primary list, hook `Pass`es all keys; TSF KeyEventSink drives `LabSession`; `ReplaceRange` maps to UTF-16 `ITfRange`. Off-list stays 2a hook.

**Likely files:** extend TSF DLL (`CompositionManager`, `KeyEventSink`), `src/coordinator.rs`, per-app list file under `%LOCALAPPDATA%\OpenViKey\tsf-apps.txt`.

**Likely tasks:** (1) coordinator tests (hook vs TSF by exe), (2) UTF-16 range mapper tests, (3) prevent double SendInput, (4) Store app smoke, (5) README underline caveat.

**Done when:** listed app types Telex without hook inject; unlisted app unchanged from 2a.

**Out:** making TSF the default for Cursor (Electron stays hook unless the user lists it).

---

## Order of docs to write later

1. After 2a ships: GĐ2b design spec (same template as 2a: locks, file map, tests, non-goals) → then writing-plans for 2b.
2. After 2b ships: GĐ2c design spec → writing-plans for 2c.
3. Do not pre-write 2b/2c TDD plans now — APIs will follow 2a host seams.

---

## Pointers

- ADR: [`docs/decisions/0007-gd2-windows-hybrid-host.md`](../../decisions/0007-gd2-windows-hybrid-host.md)
- Part 2 lab session remains the non-OS harness; README keeps that sentence.
