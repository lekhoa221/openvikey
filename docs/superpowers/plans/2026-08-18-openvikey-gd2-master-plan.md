# OpenViKey GĐ2 — Master plan (historical hybrid roadmap)

> **Superseded:** Do not execute GĐ2c/GĐ2d in this order. The owner selected the standalone product direction in [ADR 0010](../../decisions/0010-windows-standalone-default.md). Use the [Windows standalone spec](../specs/2026-08-19-openvikey-windows-standalone-design.md). GĐ2a/GĐ2b remain implementation history.

**Goal:** Ship a Windows host in four independently testable slices: daily hook typing, read-only TSF context, per-app TSF primary input, then a stable Windows product UI.

**Architecture:** Hybrid host — hook is the typing path; TSF is added later for context then for per-app primary input. Brain stays `openvikey-core` + `openvikey-session`.

**Tech Stack:** Rust 1.96, `windows` crate (2a), later a TSF COM DLL (2b/2c). GĐ2d UI technology is selected only after the 2c host/config contracts stabilize. MIT only.

**Spec:** [`../specs/2026-08-18-openvikey-gd2-windows-host-design.md`](../specs/2026-08-18-openvikey-gd2-windows-host-design.md)

## Global Constraints

- Do not copy GPL/AGPL IME code (VKey, OpenKey, PHTV, espanso).
- Do not modify `crates/openvikey-core/src/engine/**`, `types.rs`, or `model.rs`.
- Tab/Esc always pass through to the application.
- G3 corpus and PII export stay out of GĐ2.
- `unsafe` only in `openvikey-win` (and later TSF DLL), never in core/session.
- Two IMEs at once (UniKey + OpenViKey) is unsupported.

---

## How the four plans relate

```text
Plan 2a  ──implements──►  spec gd2a-hook-electron
Plan 2b  ──implements──►  spec gd2b-tsf-context     (implementation in progress)
Plan 2c  ──implements──►  spec gd2c-tsf-primary     (not written yet)
Plan 2d  ──implements──►  spec gd2d-windows-ui      (not written yet)

2b must not start until 2a manual smoke (Notepad + Cursor) is accepted.
2c must not start until 2b InputScope tests are green.
2d must not start until 2c app-routing/config contracts are stable.
```

Each plan produces a **runnable** `openvikey-win` (2b/2c add a DLL next to it). No “big bang” merge of hook+TSF+UWP.

| Plan | Spec | Implementation plan | Runnable outcome |
|---|---|---|---|
| **2a** | [`../specs/2026-08-18-openvikey-gd2a-hook-electron-design.md`](../specs/2026-08-18-openvikey-gd2a-hook-electron-design.md) | **This folder:** `2026-08-18-openvikey-gd2a-implementation-plan.md` (full TDD) | `openvikey-win` types into Notepad + Cursor via hook |
| **2b** | [`../specs/2026-08-19-openvikey-gd2b-tsf-context-design.md`](../specs/2026-08-19-openvikey-gd2b-tsf-context-design.md) | [`2026-08-19-openvikey-gd2b-implementation-plan.md`](./2026-08-19-openvikey-gd2b-implementation-plan.md) | Same exe; TSF DLL **read-only**; hook still types |
| **2c** | *write at start of pha* `YYYY-MM-DD-openvikey-gd2c-tsf-primary-design.md` | *write after that spec* `...-gd2c-implementation-plan.md` | Per-exe TSF primary input; hook skipped for those processes |
| **2d** | *write at start of pha* `YYYY-MM-DD-openvikey-gd2d-windows-ui-design.md` | *write after that spec* `...-gd2d-implementation-plan.md` | UniKey-style settings/control window over the stable 2a–2c host |

---

## Plan 2a — looks like (locked, execute now)

**Done when:** unit tests in spec §10 green; lab `session_capture` still green; README has run + keymap + UniKey-off; manual Notepad + Cursor composer Telex `xin chao`.

**File map:** extract `openvikey-session`; add `openvikey-win` (`policy`, `sync`, `inject`, `classify`, `hook`, `overlay`, `tray`, `host`).

**Task shape (detail in the historical 2a plan v3):** (1) extract session crate, (2) policy including OVK Pass / Backspace / Ctrl+C / Caps / Enter `CommitAndPass`, (3) sync from `obs.action` + punctuation delimiter, (4) `SendingGuard` + `Arc<AtomicBool>` + partial SendInput, (5) classify, (6) host + AcceptVisual/Undo + `SessionSaveSnapshot` + HWND, (7) hook return semantics, no key queue, (8) mouse + focus cache, (9) overlay + tray, (10) persist + main, (11) deny + `no_key_log` includes `host.rs` + manual Notepad/Cursor gate. P95 is `#[ignore]`, not CI. Post-checkpoint, ADR 0008 replaces the Windows passphrase prompt with open development JSON stores; core/lab encryption remains.

**Not in 2a:** TSF, InputScope, bait char, autostart, Authenticode.

---

## Plan 2b — executing

**Goal:** Password/PIN fields do not get Telex; `left_context` can use surrounding text when TSF exposes it.

**Architecture:** Register a TSF text service DLL that **does not** commit composition as the default path. It publishes InputScope + a small surrounding-text snapshot over IPC/shared memory to `openvikey-win`. Hook remains the typer. Credit VietType/VKey for InputScope *idea* only.

**Pre-design survey:** [`../specs/2026-08-19-openvikey-gd2b-tsf-context-survey.md`](../specs/2026-08-19-openvikey-gd2b-tsf-context-survey.md). It locks the scope and required Phase 0 evidence, not the final implementation choices.

**Selected implementation:** Rust `windows 0.62.2`; pure shared contracts in `openvikey-win-context`, COM DLL in `openvikey-win-tsf`, host bridge/cache in `openvikey-win`.

**Executable tasks:** See the linked GĐ2b plan. Phase 0 registration/lifecycle and initial sensitive policy are complete; protocol/cache, session rebase, bridge, read adapter, compatibility matrix and Data Inspector remain.

**Done when:** denylist 2a still works; explicit password/PIN scope makes hook `Pass` before it eats the key and produces zero context read/capture/model mutation; no double typing; the development Data Inspector can inspect normal learning/capture data without becoming the production settings shell. Development persistence remains plaintext `.ovkdev.json`; no password/passphrase is introduced in 2b.

**Out:** TSF as the key source; UWP primary; changing Tab/Esc.

---

## Plan 2c — cancelled as default product path (do not execute)

**Goal:** A user-managed list of executables uses TSF as **primary** input (UWP / anti-cheat), including Windows composition underline.

**Architecture:** Coordinator: if foreground exe is on the TSF-primary list, hook `Pass`es all keys; TSF KeyEventSink drives `LabSession`; `ReplaceRange` maps to UTF-16 `ITfRange`. Off-list stays 2a hook.

**Likely files:** extend TSF DLL (`CompositionManager`, `KeyEventSink`), `src/coordinator.rs`, per-app list file under `%LOCALAPPDATA%\OpenViKey\tsf-apps.txt`.

**Likely tasks:** (1) coordinator tests (hook vs TSF by exe), (2) UTF-16 range mapper tests, (3) prevent double SendInput, (4) Store app smoke, (5) README underline caveat.

**Done when:** listed app types Telex without hook inject; unlisted app unchanged from 2a.

**Out:** making TSF the default for Cursor (Electron stays hook unless the user lists it).

---

## Plan 2d — Windows product UI (after 2c)

**Goal:** Provide a UniKey-style Windows control surface without duplicating typing, learning, or app-routing logic in the UI.

**Scope:** method/tone settings, V/E and suggestion state, hotkeys, learned-rule inspection and deletion, open data folder, per-app hook/TSF policy, start-with-Windows, import/export settings. Installer, product icons, and signing are separate release gates after the control surface is stable.

**Architecture:** UI consumes versioned host/config/model-inspection APIs. It does not own hooks, TSF composition, ranking, persistence recovery, or a second copy of session state. The UI framework and process boundary are selected in the 2d spec after 2c fixes the app-routing schema.

**Done when:** every UI mutation round-trips through one validated settings contract; restarting the host preserves settings; learned data is inspectable and selectively forgettable; closing the window does not stop the tray host.

**Out:** cloud accounts/sync, telemetry, corpus editing, macro automation, automatic upload, and macOS UI.

---

## Order of docs to write later

1. After 2a ships: GĐ2b design spec (same template as 2a: locks, file map, tests, non-goals) → then writing-plans for 2b.
2. After 2b ships: GĐ2c design spec → writing-plans for 2c.
3. After 2c ships: GĐ2d Windows UI spec → writing-plans for 2d.
4. Do not pre-write later TDD plans — 2b follows live 2a seams, 2c follows 2b TSF seams, and 2d follows the stable 2c settings/app-routing schema.

---

## Pointers

- Host ADR: [`docs/decisions/0007-gd2-windows-hybrid-host.md`](../../decisions/0007-gd2-windows-hybrid-host.md)
- Development persistence ADR: [`docs/decisions/0008-open-development-persistence.md`](../../decisions/0008-open-development-persistence.md)
- Part 2 lab session remains the non-OS harness; README keeps that sentence.
