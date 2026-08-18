# ADR 0007: GĐ2 Windows hybrid host (hook first, TSF later)

- **Date:** 2026-08-18
- **Status:** Accepted
- **Gate:** GĐ2 Windows system integration

## Context

The v1 design spec named GĐ2 “Windows via TSF”. Part 2 shipped a lab-only capture REPL and explicitly forbade OS hooks (ADR 0006). The project owner then asked to type in every app (Cursor Agent prompts included), not in the TTY harness.

A survey of `D:\Workspace\CloneFromGit\VNKeyboard` (2026-08-18) showed:

- Daily-driver Vietnamese IMEs on Windows are **hook + SendInput** (OpenKey win32, UniKey host, VKey default path).
- VKey, the most complete open-source Windows IME in that tree, keeps **hook as primary input** and uses TSF for surrounding context / InputScope, with TSF-as-primary only per app. License **GPL-3.0** — ideas only.
- Electron/Chromium hosts need a different inject profile than classic Win32 (VKey tests name Electron/WebView2). Cursor is Electron.
- OpenViKey is **MIT**; workspace `unsafe_code = forbid` on core; TSF-on-Rust was already listed as a risk in the v1 spec.

TSF-only would delay the owner’s daily Agent workflow and still miss apps where UniKey works. Hook-only would never get reliable password InputScope or UTF-16 `ReplaceRange`.

## Decision

1. **GĐ2 is a hybrid Windows host**, not “TSF-only” and not “hook forever”.
2. **Three plans:** GĐ2a hook + Electron inject; GĐ2b TSF read-only context; GĐ2c TSF primary per app. Specs: `docs/superpowers/specs/2026-08-18-openvikey-gd2-windows-host-design.md` and `...-gd2a-hook-electron-design.md`.
3. Extract **`openvikey-session`** from lab so the host does not depend on the REPL. Core `engine/`, `types.rs`, `model.rs` stay frozen.
4. **Tab/Esc pass through.** Accept/reject are `Ctrl+.` / `Ctrl+,`.
5. **No GPL code.** Credit VKey, OpenKey, and VietType (InputScope, later) in GĐ2 docs.

## Consequences

- README / v1 spec wording “v2 = TSF” is outdated; they point at this ADR and the GĐ2 master spec.
- `openvikey-win` must allow `unsafe` in FFI modules only. Antivirus heuristics will flag the hook; document, don’t pretend TSF-only avoids that for 2a.
- Lab `session` remains valid for headless learning tests. Same encrypted files if paths/passphrase match.
- GĐ2 does not close G3 (lexicon size).

## Amendment (2026-08-18)

GĐ2a spec v2: host must read `SessionObservation.action` for Auto (`ReplaceRange` is not in `engine_actions`); Enter is `CommitAndPass`; hook thread is try-lock-only; Reset/caret-break must not SendInput backspaces into the newly focused window. See spec v2 changelog.

## Amendment v3 (2026-08-18)

GĐ2a spec/plan v3, from a second review of WH_KEYBOARD_LL return semantics and `LabSession::capture_log`:

- Injected keys (`dwExtraInfo == OVK_EXTRA`) **Pass** (`CallNextHookEx`). Eating them would swallow our own `SendInput`.
- No async key queue. Session + inject run **synchronously** in the LL callback via `try_lock`; Enter is forwarded only **after** replacement.
- Saver clones `SessionSaveSnapshot` under one lock; serialize and SHA-256 **after** unlock. Do not call `capture_log()` on that path.
- Shared `Arc<AtomicBool>` + `SendingGuard` (Drop clears even on partial `SendInput`).
- Real-keyboard policy includes Backspace, Ctrl/Alt/Win shortcuts, Caps Lock, punctuation delimiters, and the documented hotkeys.
