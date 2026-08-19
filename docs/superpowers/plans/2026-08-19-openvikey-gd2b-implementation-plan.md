# OpenViKey GĐ2b — TSF read-only context implementation plan

> **For agentic workers:** Execute one vertical slice at a time with red → green TDD. Do not start GĐ2c key-event/composition or GĐ2d product UI work in this plan.

**Goal:** Keep the GĐ2a hook as the typing path while adding read-only TSF context so explicit password/PIN fields pass through untouched and normal fields can rebase the session from one bounded previous token.

**Architecture:** A Rust in-process TSF COM DLL classifies InputScope before any text read and publishes owned, bounded snapshots over a per-user named pipe. The host validates snapshots against foreground identity and exposes a lock-free projection to the hook. Development persistence remains plaintext and a final Data Inspector is read-only.

**Tech Stack:** Rust 1.96; `windows`/`windows-core` 0.62.2; `windows-registry` 0.6.1; serde; existing `openvikey-session` persistence schemas.

**Spec:** [`../specs/2026-08-19-openvikey-gd2b-tsf-context-design.md`](../specs/2026-08-19-openvikey-gd2b-tsf-context-design.md) v0.3

## Global constraints

- No password/passphrase, encryption migration, TSF key sink, composition or write edit session.
- No COM, IPC or blocking lock in the low-level hook callback.
- Sensitive classification happens before surrounding-text reads; sensitive snapshots never contain a token.
- Frames are versioned and bounded to 4 KiB; tokens are NFC and at most 128 UTF-8 bytes.
- Foreground match uses PID/TID plus generation; matching HWND, when both present, is additional corroboration.
- Data Inspector never writes, repairs, deletes or migrates data.
- Preserve GĐ2a fallback for apps without an active TSF source.

## Task 0 — Rust TSF registration/lifecycle gate

**Files:** `crates/openvikey-win-tsf/**`, workspace dependency/features, design §Phase 0.

- [x] Build x64 COM `cdylib`, class factory and `ITfTextInputProcessorEx` lifecycle.
- [x] Add explicit per-user registration helper; do not mutate registry from host startup.
- [x] Register/unregister 20 elevated cycles with clean rollback.
- [x] Activate the registered COM server through `CoCreateInstance`; advise/unadvise thread-manager sink and return object/lock count to zero.
- [ ] Prove Windows auto-activation/focus/deactivation in a real app and record unload evidence.

## Task 1 — Pure context contract and extraction

**Files:** `crates/openvikey-win-context/src/lib.rs` and focused unit tests.

- [x] Red → green: password/PIN InputScope classification.
- [x] Red → green: extract the previous token from bounded UTF-16-left text, normalize NFC, reject output above 128 UTF-8 bytes.
- [x] Red → green: define `ForegroundIdentity`, `ContextSnapshot`, `ContextProjection` and `ReadContextResult` as owned values.
- [x] Red → green: validate protocol version, nonzero identity, state/token invariants and monotonic sequence per source instance.

**Gate:** pure crate tests cover sensitive precedence, Unicode, token bound and malformed snapshot rejection without Windows APIs.

## Task 2 — Context cache projection

**Files:** `crates/openvikey-win-context/src/cache.rs` or the smallest equivalent module; tests in the same crate.

- [x] Red → green: new matching source projects `Pending` until its first result.
- [x] Red → green: exact PID/TID/generation match projects Normal/Sensitive/Unavailable; HWND mismatch rejects.
- [x] Red → green: stale instance/context sequence and focus generation never reuse a token.
- [x] Red → green: disconnect invalidates the source; no source projects `Unsupported`.

**Gate:** projection is an owned/copyable small value suitable for publication outside the cache lock.

## Task 3 — Host policy, transition and session rebase

**Files:** `crates/openvikey-win/src/policy.rs`, host/context integration, `crates/openvikey-session/src/session.rs`, focused tests.

- [x] Red → green: Sensitive/Pending/Unavailable physical keys pass before normal hook classification; own `SendInput` still passes first.
- [x] Red → green: `LabSession::rebase_left_context` normalizes one token, clears caret-dependent anchors and does not emit capture/save mutation by itself.
- [x] Red → green: Normal → non-normal transition clears composition/external context once without injecting text.
- [x] Replace runtime `ContextState::Unsupported` placeholder with a lock-free current projection; cache read failure passes one physical key.

**Gate:** focused host test proves a sensitive key changes neither session snapshot nor model/capture files.

## Task 4 — Bounded bridge

**Files:** shared frame codec in `openvikey-win-context`; TSF client and host server adapters; lifecycle tests.

- [x] Red → green: length-prefixed codec rejects unknown version, oversize, malformed identity and invalid state/token pairs.
- [ ] Implement user-local named-pipe server with current-user/SYSTEM ACL and finite I/O.
- [ ] Implement TSF bounded latest-value publisher; COM callbacks only enqueue with no unbounded allocation or wait.
- [ ] Test host absent/restart, client reconnect/disconnect, focus storm and clean cancellation.

**Gate:** stale/malformed frames never reach hook projection and DLL deactivation never waits indefinitely for I/O.

## Task 5 — TSF read adapter

**Files:** `crates/openvikey-win-tsf/src/lib.rs` plus Windows-only integration tests.

- [ ] Implement focus/context sink lifecycle and request `TF_ES_READ | TF_ES_ASYNCDONTCARE`.
- [ ] Read `GUID_PROP_INPUTSCOPE` first; if sensitive, publish immediately with no `ITfRange::GetText` call.
- [ ] For normal context only, read at most 128 UTF-16 code units left of caret and use the shared extractor.
- [ ] Publish Pending/Normal/Sensitive/Unavailable with source/context sequences and optional active-view HWND.
- [ ] Instrument tests so sensitive scope proves text-read count is zero.

**Gate:** real-app smoke reads a normal Notepad token; password/PIN cases publish Sensitive without surrounding text.

## Task 6 — Development-only Data Inspector

**Files:** smallest new binary crate or bin target reusing public read-only persistence inspection APIs; README/runbook; tests.

- [ ] Red → green: load default or explicit `.ovkdev.json` paths with the same schema/provenance validation as host.
- [ ] Show summary and filters for original, candidate, source, left token, evidence/count and time.
- [ ] Manual refresh; clear warnings for missing/invalid/provenance mismatch.
- [ ] Prove byte-for-byte that opening/filtering/refreshing does not change either input file.

**Gate:** useful CLI/TUI inspection exists; settings mutation and polished UniKey-style window remain GĐ2d.

## Task 7 — Release evidence and closure

- [ ] Run fmt, workspace clippy `-D warnings`, workspace tests/all-features and `cargo deny check`.
- [ ] Manual matrix: Notepad/Cursor normal; Win32/WPF/WinUI/Chrome/Edge password/PIN; unsupported app fallback; host restart/focus storm.
- [ ] Record TSF active/scopes/policy and model/capture before-after for every sensitive case.
- [ ] Decide and document x86 support boundary; never imply x86 support without build/register/smoke evidence.
- [ ] Update README, ADR 0007, governing design and this checklist from live evidence.

**Done:** hook typing remains GĐ2a; explicit sensitive fields pass untouched with zero surrounding-text read and zero learning/capture mutation; normal TSF context rebases one bounded token; unsupported apps fall back; Data Inspector is read-only; no password is introduced.
