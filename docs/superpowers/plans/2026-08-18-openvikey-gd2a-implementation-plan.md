# OpenViKey GĐ2a — Hook + Electron Implementation Plan

> **Historical implementation record:** Task 10 documents the original encrypted passphrase adapter. Post-checkpoint ADR 0008 removes the passphrase from `openvikey-win` and uses open `model.ovkdev.json` / `capture.ovkdev.json`; core and lab encryption remain intact.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract the Part 2 session reducer into `openvikey-session` and ship `openvikey-win` so Telex types into Notepad and Cursor via a keyboard hook, Enter still submits/newlines, and Auto replacements actually appear in the app.

**Architecture:** Policy is lock-free on the hook thread. `OVK_EXTRA` Passes so `SendInput` reaches the app. Session + inject run **synchronously** in the LL callback (`try_lock`, no key mpsc). `commands_from_typed` reads `SessionObservation.action` for Auto. Caret-break never SendInputs. Saver clones `SessionSaveSnapshot` under one lock and serializes after unlock.

**Tech Stack:** Rust 1.96, `openvikey-core` unchanged, `windows` **0.62.2** (MIT OR Apache-2.0), no crossterm in win.

**Spec:** [`../specs/2026-08-18-openvikey-gd2a-hook-electron-design.md`](../specs/2026-08-18-openvikey-gd2a-hook-electron-design.md) **v3**

## Global Constraints

- Do not modify `crates/openvikey-core/src/engine/**`, `types.rs`, or `model.rs`.
- Do not copy VKey/OpenKey/espanso source.
- `unsafe` only in `hook.rs`, `mouse.rs`, `inject.rs`, `overlay.rs`, `tray.rs`, `focus.rs`, and post-checkpoint `console.rs`; `passphrase.rs` is retired.
- Do not `[lints] workspace = true` on `openvikey-win` (workspace `unsafe_code = "forbid"` cannot be allowed later).
- Tab/Esc always `Pass`. Enter is `CommitAndPass`, never U+0020.
- `OVK_EXTRA` → `Pass` (never eat injected keys). Physical keys while `is_sending` → `EatAndIgnore`.
- No key-processing mpsc queue. `try_lock` + SendInput run synchronously in the LL callback.
- `OVK_EXTRA = 0x4F564B31`.
- Do not delete files: copy session modules then `pub use` from lab.
- `cargo test --workspace --all-features` and clippy `-D warnings` stay green.

## File map

Create: `crates/openvikey-session/**`, `crates/openvikey-win/**`.

Modify: workspace `Cargo.toml` (members + `windows` dep), lab `Cargo.toml` + four modules as re-exports, `deny.toml` only if deny fails, README **runbook for `openvikey-win` only** (GĐ2 wording already updated — do not rewrite ADR/spec §2.3).

---

### Task 1: Extract `openvikey-session`

**Files:**
- Create: `crates/openvikey-session/Cargo.toml`, `src/lib.rs`, copy `document.rs`, `capture.rs`, `session.rs`, `persistence.rs` from lab
- Modify: workspace members; lab depends on `openvikey-session`; lab four modules become `pub use`

**Produces:** `openvikey_session::{document, capture, session, persistence}`

- [x] **Step 1: Add crate**

`crates/openvikey-session/Cargo.toml`:

```toml
[package]
name = "openvikey-session"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
description = "Personal capture-and-learn session reducer (no TTY, no OS hook)"

[dependencies]
openvikey-core = { path = "../openvikey-core" }
serde = { workspace = true }
serde_json = { workspace = true }
sha2 = { workspace = true }
hex = { workspace = true }
thiserror = { workspace = true }
unicode-normalization = { workspace = true }
unicode-segmentation = { workspace = true }
zeroize = { workspace = true }

[lints]
workspace = true
```

`src/lib.rs`:

```rust
pub mod capture;
pub mod document;
pub mod persistence;
pub mod session;
```

Copy the four lab files unchanged except crate-internal `crate::` paths stay valid inside the new crate. Add `"crates/openvikey-session"` to `[workspace].members`.

- [x] **Step 2: Lab re-exports (do not delete lab files)**

Replace each lab module body with the matching `pub use openvikey_session::…::*;` (or explicit types if clippy `wildcard_imports` fires). Add `openvikey-session = { path = "../openvikey-session" }` to lab.

- [x] **Step 3: Run** `cargo test -p openvikey-lab --test session_capture --all-features`

Expected: PASS.

Run: `cargo test -p openvikey-lab --all-features`

Expected: PASS.

- [x] **Step 4: Commit** `refactor(session): extract capture reducer from lab into openvikey-session`

---

### Task 2: Key policy

**Files:** Create `crates/openvikey-win/Cargo.toml`, `src/lib.rs`, `src/policy.rs`. Test: `tests/policy.rs`.

**Produces:** `decide`, `KeyDecision`, `OVK_EXTRA`

```rust
pub const OVK_EXTRA: usize = 0x4F564B31;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode { Viet, English }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostState {
    pub mode: Mode,
    pub foreground_exe: String,
    pub is_sending: bool,
    pub caps_lock: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawKey {
    pub vk: u16,
    pub down: bool,
    pub control: bool,
    pub shift: bool,
    pub extra_info: usize,
    pub left_ctrl: bool,
    pub left_shift: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostHotkey { AcceptTop, RejectTop, UndoLast, ToggleMode }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyDecision {
    Pass,
    EatAndIgnore,
    EatAndInject(openvikey_core::types::InputKind),
    CommitAndPass { delimiter: char },
    Hotkey(HostHotkey),
    CaretBreakAndPass,
}
```

`Cargo.toml` (no workspace lints inherit):

```toml
[package]
name = "openvikey-win"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
description = "Windows hook host for OpenViKey"

[dependencies]
openvikey-core = { path = "../openvikey-core" }
openvikey-session = { path = "../openvikey-session" }
clap = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
unicode-segmentation = { workspace = true }
zeroize = { workspace = true }
windows = { workspace = true }

[lints.rust]
unsafe_code = "allow"

[lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -2 }
doc_markdown = "allow"
must_use_candidate = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"
module_name_repetitions = "allow"
```

Workspace `Cargo.toml` add member `crates/openvikey-win` and:

```toml
windows = { version = "0.62.2", features = [
    "Win32_Foundation",
    "Win32_Graphics_Gdi",
    "Win32_Security",
    "Win32_System_Console",
    "Win32_System_Threading",
    "Win32_System_ProcessStatus",
    "Win32_UI_Accessibility",
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_UI_Shell",
    "Win32_UI_WindowsAndMessaging",
] }
```

If a feature name fails to compile on 0.62.2, drop only the missing feature after `cargo build -p openvikey-win` error; do not downgrade to 0.58.

- [x] **Step 1: Failing tests** `tests/policy.rs`

```rust
use openvikey_core::types::InputKind;
use openvikey_win::policy::{
    decide, HostHotkey, HostState, KeyDecision, Mode, RawKey, OVK_EXTRA,
};

fn viet() -> HostState {
    HostState {
        mode: Mode::Viet,
        foreground_exe: "notepad.exe".into(),
        is_sending: false,
        caps_lock: false,
        alt: false,
        meta: false,
    }
}

fn key(vk: u16) -> RawKey {
    RawKey {
        vk,
        down: true,
        control: false,
        shift: false,
        extra_info: 0,
        left_ctrl: false,
        left_shift: false,
    }
}

#[test]
fn tab_and_esc_always_pass() {
    for vk in [0x09u16, 0x1B] {
        assert_eq!(decide(&key(vk), &viet()), KeyDecision::Pass);
    }
}

#[test]
fn letter_eaten_as_key_in_viet() {
    assert_eq!(
        decide(&key(0x41), &viet()),
        KeyDecision::EatAndInject(InputKind::Key { logical: 'a', physical: None })
    );
}

#[test]
fn powershell_denylist_passes_letters() {
    let mut st = viet();
    st.foreground_exe = "powershell.exe".into();
    assert_eq!(decide(&key(0x41), &st), KeyDecision::Pass);
}

#[test]
fn ovk_extra_passes_so_sendinput_reaches_app() {
    let mut k = key(0x41);
    k.extra_info = OVK_EXTRA;
    assert_eq!(decide(&k, &viet()), KeyDecision::Pass);
}

#[test]
fn ovk_extra_passes_even_while_sending() {
    let mut st = viet();
    st.is_sending = true;
    let mut k = key(0x41);
    k.extra_info = OVK_EXTRA;
    assert_eq!(decide(&k, &st), KeyDecision::Pass);
}

#[test]
fn ctrl_period_accepts_ctrl_z_does_not_undo() {
    let mut acc = key(0xBE);
    acc.control = true;
    assert_eq!(decide(&acc, &viet()), KeyDecision::Hotkey(HostHotkey::AcceptTop));
    let mut comma = key(0xBC);
    comma.control = true;
    assert_eq!(decide(&comma, &viet()), KeyDecision::Hotkey(HostHotkey::RejectTop));
    let mut undo = key(0x5A);
    undo.control = true;
    undo.shift = true;
    assert_eq!(decide(&undo, &viet()), KeyDecision::Hotkey(HostHotkey::UndoLast));
    let mut z = key(0x5A);
    z.control = true;
    assert_ne!(decide(&z, &viet()), KeyDecision::Hotkey(HostHotkey::UndoLast));
    assert_eq!(decide(&z, &viet()), KeyDecision::Pass);
}

#[test]
fn enter_is_commit_and_pass_newline() {
    assert_eq!(
        decide(&key(0x0D), &viet()),
        KeyDecision::CommitAndPass { delimiter: '\n' }
    );
}

#[test]
fn left_arrow_is_caret_break_and_pass() {
    assert_eq!(decide(&key(0x25), &viet()), KeyDecision::CaretBreakAndPass);
}

#[test]
fn sending_eats_physical_repeat() {
    let mut st = viet();
    st.is_sending = true;
    assert_eq!(decide(&key(0x41), &st), KeyDecision::EatAndIgnore);
}

#[test]
fn backspace_is_eaten() {
    assert_eq!(
        decide(&key(0x08), &viet()),
        KeyDecision::EatAndInject(InputKind::Backspace)
    );
}

#[test]
fn ctrl_c_passes() {
    let mut k = key(0x43);
    k.control = true;
    assert_eq!(decide(&k, &viet()), KeyDecision::Pass);
}

#[test]
fn letter_keyup_passes() {
    let mut k = key(0x41);
    k.down = false;
    assert_eq!(decide(&k, &viet()), KeyDecision::Pass);
}

#[test]
fn caps_lock_makes_a_uppercase() {
    let mut st = viet();
    st.caps_lock = true;
    assert_eq!(
        decide(&key(0x41), &st),
        KeyDecision::EatAndInject(InputKind::Key { logical: 'A', physical: None })
    );
}

#[test]
fn period_is_boundary() {
    assert_eq!(
        decide(&key(0xBE), &viet()),
        KeyDecision::EatAndInject(InputKind::Boundary { delimiter: '.' })
    );
}

#[test]
fn comma_is_boundary_without_ctrl() {
    assert_eq!(
        decide(&key(0xBC), &viet()),
        KeyDecision::EatAndInject(InputKind::Boundary { delimiter: ',' })
    );
}

#[test]
fn ctrl_v_x_a_pass() {
    for vk in [0x56u16, 0x58, 0x41] {
        let mut k = key(vk);
        k.control = true;
        assert_eq!(decide(&k, &viet()), KeyDecision::Pass);
    }
}

#[test]
fn alt_or_win_letter_passes() {
    let mut alt = key(0x41);
    alt.control = false;
    let mut st = viet();
    st.alt = true;
    assert_eq!(decide(&alt, &st), KeyDecision::Pass);
    st.alt = false;
    st.meta = true;
    assert_eq!(decide(&alt, &st), KeyDecision::Pass);
}

#[test]
fn left_ctrl_left_shift_keyup_toggles() {
    let mut k = key(0xA0); // VK_LSHIFT
    k.down = false;
    k.left_ctrl = true;
    k.left_shift = true;
    assert_eq!(decide(&k, &viet()), KeyDecision::Hotkey(HostHotkey::ToggleMode));
}
```

- [x] **Step 2:** `cargo test -p openvikey-win --test policy` — FAIL (crate missing).

- [x] **Step 3: Implement `policy.rs`** following spec §3.1 order. `VK_BACK` `0x08` → Backspace. `control|alt|meta` and not an OpenViKey hotkey → Pass. Keyup → Pass except toggle chord (Left-Ctrl+Left-Shift). Caps XOR shift for letters. `0xBE` without Ctrl → Boundary `'.'`; `0xBC` without Ctrl → Boundary `','`. Nav VKs as spec. Denylist `eq_ignore_ascii_case`.

`src/lib.rs`: `pub mod policy;`

Also add `pub fn hook_allows_next(decision: &KeyDecision) -> bool` — `true` for `Pass`, `CommitAndPass`, `CaretBreakAndPass`; `false` for eat/hotkey/ignore. Test: `OVK_EXTRA` decision is Pass so `hook_allows_next` is true.

- [x] **Step 4:** tests PASS. Commit `feat(win): add key policy with commit-and-pass enter`

---

### Task 3: Composition sync (core contract)

**Files:** Create `src/sync.rs`. Test: `tests/sync.rs`. Modify session only if adding types used here — visual structs are Task 6; this task uses `SessionObservation` + `ReplaceRangeAction` as they exist today.

**Produces:** `InjectCommand`, `commands_from_typed`, `commands_from_caret_break`

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectCommand {
    Replace { backspace_graphemes: usize, text_nfc: String },
    AppendDelimiter { delimiter: char },
}
```

Helper in test file:

```rust
use openvikey_core::types::{
    CompositionSnapshot, EditRange, EngineAction, RangeBasis, ReplaceRangeAction,
};
use openvikey_session::session::SessionObservation;
use openvikey_win::sync::{commands_from_caret_break, commands_from_typed, InjectCommand};

fn obs(
    engine_actions: Vec<EngineAction>,
    action: Option<EngineAction>,
) -> SessionObservation {
    SessionObservation {
        event_seq: 1,
        snapshot: CompositionSnapshot::new(1, String::new(), String::new()),
        engine_actions,
        candidates: Vec::new(),
        decision: None,
        action,
    }
}
```

- [x] **Step 1: Failing tests**

```rust
#[test]
fn update_replaces_previous_sent() {
    let observation = obs(
        vec![EngineAction::UpdateComposition { revision: 1, text: "à".into() }],
        None,
    );
    let (cmds, sent) = commands_from_typed(&observation, "a");
    assert_eq!(
        cmds,
        vec![InjectCommand::Replace { backspace_graphemes: 1, text_nfc: "à".into() }]
    );
    assert_eq!(sent, "à");
}

#[test]
fn space_commit_appends_space_and_clears_sent() {
    let observation = obs(
        vec![EngineAction::Commit { revision: 1, text: "xin".into(), delimiter: Some(' ') }],
        None,
    );
    let (cmds, sent) = commands_from_typed(&observation, "xin");
    assert_eq!(cmds, vec![InjectCommand::AppendDelimiter { delimiter: ' ' }]);
    assert_eq!(sent, "");
}

#[test]
fn auto_replace_uses_action_not_commit_text() {
    let action = EngineAction::ReplaceRange(ReplaceRangeAction {
        edit_id: 1,
        range: EditRange {
            basis: RangeBasis::ActiveComposition,
            start_grapheme: 0,
            length_grapheme: 2,
            revision: 1,
        },
        original: "ko".into(),
        replacement: "không".into(),
        delimiter: Some(' '),
    });
    let observation = obs(
        vec![EngineAction::Commit { revision: 1, text: "ko".into(), delimiter: Some(' ') }],
        Some(action),
    );
    let (cmds, sent) = commands_from_typed(&observation, "ko");
    assert_eq!(
        cmds,
        vec![
            InjectCommand::Replace { backspace_graphemes: 2, text_nfc: "không".into() },
            InjectCommand::AppendDelimiter { delimiter: ' ' },
        ]
    );
    assert_eq!(sent, "");
    assert!(!cmds.iter().any(|c| matches!(
        c,
        InjectCommand::Replace { text_nfc, .. } if text_nfc == "ko"
    )));
}

#[test]
fn caret_break_does_not_backspace_into_new_focus() {
    let (cmds, sent) = commands_from_caret_break("chào");
    assert!(cmds.is_empty());
    assert_eq!(sent, "");
}

#[test]
fn newline_commit_does_not_append_delimiter() {
    let observation = obs(
        vec![EngineAction::Commit { revision: 1, text: "xin".into(), delimiter: Some('\n') }],
        None,
    );
    let (cmds, sent) = commands_from_typed(&observation, "xin");
    assert_eq!(cmds, Vec::<InjectCommand>::new());
    assert_eq!(sent, "");
}

#[test]
fn period_commit_appends_period() {
    let observation = obs(
        vec![EngineAction::Commit { revision: 1, text: "xin".into(), delimiter: Some('.') }],
        None,
    );
    let (cmds, sent) = commands_from_typed(&observation, "xin");
    assert_eq!(cmds, vec![InjectCommand::AppendDelimiter { delimiter: '.' }]);
    assert_eq!(sent, "");
}
```

- [x] **Step 2:** `cargo test -p openvikey-win --test sync` — FAIL (`commands_from_typed` missing).

- [x] **Step 3: Implement**

```rust
use openvikey_core::types::EngineAction;
use openvikey_session::session::SessionObservation;
use unicode_segmentation::UnicodeSegmentation;

pub fn grapheme_len(s: &str) -> usize {
    s.graphemes(true).count()
}

pub fn commands_from_caret_break(_sent_nfc: &str) -> (Vec<InjectCommand>, String) {
    (Vec::new(), String::new())
}

pub fn commands_from_typed(
    obs: &SessionObservation,
    sent_nfc: &str,
) -> (Vec<InjectCommand>, String) {
    if let Some(EngineAction::ReplaceRange(action)) = &obs.action {
        if obs.engine_actions.iter().any(|a| matches!(a, EngineAction::Commit { .. })) {
            let mut cmds = vec![InjectCommand::Replace {
                backspace_graphemes: grapheme_len(sent_nfc),
                text_nfc: action.replacement.clone(),
            }];
            let delimiter = obs.engine_actions.iter().find_map(|a| match a {
                EngineAction::Commit { delimiter, .. } => *delimiter,
                _ => None,
            });
            if let Some(d) = delimiter {
                if d != '\n' {
                    cmds.push(InjectCommand::AppendDelimiter { delimiter: d });
                }
            }
            return (cmds, String::new());
        }
    }
    let mut sent = sent_nfc.to_string();
    let mut cmds = Vec::new();
    for action in &obs.engine_actions {
        match action {
            EngineAction::UpdateComposition { text, .. } => {
                cmds.push(InjectCommand::Replace {
                    backspace_graphemes: grapheme_len(&sent),
                    text_nfc: text.clone(),
                });
                sent.clone_from(text);
            }
            EngineAction::Commit { text, delimiter, .. } => {
                if sent != *text {
                    cmds.push(InjectCommand::Replace {
                        backspace_graphemes: grapheme_len(&sent),
                        text_nfc: text.clone(),
                    });
                    sent.clone_from(text);
                }
                if let Some(d) = *delimiter {
                    if d != '\n' {
                        cmds.push(InjectCommand::AppendDelimiter { delimiter: d });
                    }
                }
                sent.clear();
            }
            EngineAction::ReplaceRange(_) | EngineAction::ShowSuggestions { .. } => {}
        }
    }
    (cmds, sent)
}
```

`engine_actions` will not contain `ReplaceRange` from `Engine::process`; the match arms exist so a mistaken push is ignored.

- [x] **Step 4:** tests PASS. Commit `feat(win): sync inject commands from observation.action`

---

### Task 4: Injector batching (no sleep)

**Files:** `src/inject.rs`. Test: `tests/inject.rs`.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynthesizedEvent { Backspace, Utf16(u16) }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectProfile { Win32, Electron }

pub trait InputSender {
    fn send(&mut self, events: &[SynthesizedEvent]) -> Result<u32, InjectError>;
}

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct SendingGuard {
    flag: Arc<AtomicBool>,
}

impl SendingGuard {
    pub fn enter(flag: Arc<AtomicBool>) -> Self {
        flag.store(true, Ordering::SeqCst);
        Self { flag }
    }
}

impl Drop for SendingGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

pub struct ProfilingInjector<S: InputSender> {
    pub profile: InjectProfile,
    pub sender: S,
    pub sending: Arc<AtomicBool>,
}

impl<S: InputSender> ProfilingInjector<S> {
    pub fn replace(&mut self, backspace_graphemes: usize, text_nfc: &str) -> Result<(), InjectError> {
        let mut events = Vec::new();
        for _ in 0..backspace_graphemes {
            events.push(SynthesizedEvent::Backspace);
        }
        for unit in text_nfc.encode_utf16() {
            events.push(SynthesizedEvent::Utf16(unit));
        }
        self.dispatch(&events)
    }

    pub fn append_delimiter(&mut self, delimiter: char) -> Result<(), InjectError> {
        let mut buf = [0u16; 2];
        let encoded: Vec<SynthesizedEvent> = delimiter
            .encode_utf16(&mut buf)
            .iter()
            .copied()
            .map(SynthesizedEvent::Utf16)
            .collect();
        self.dispatch(&encoded)
    }

    fn dispatch(&mut self, events: &[SynthesizedEvent]) -> Result<(), InjectError> {
        if events.is_empty() {
            return Ok(());
        }
        let _guard = SendingGuard::enter(Arc::clone(&self.sending));
        match self.profile {
            InjectProfile::Win32 => {
                let n = self.sender.send(events)?;
                if n as usize != events.len() {
                    return Err(InjectError::Partial { sent: n, expected: events.len() });
                }
                Ok(())
            }
            InjectProfile::Electron => {
                let split = events.iter().position(|e| matches!(e, SynthesizedEvent::Utf16(_)));
                match split {
                    None | Some(0) => {
                        let n = self.sender.send(events)?;
                        if n as usize != events.len() {
                            return Err(InjectError::Partial { sent: n, expected: events.len() });
                        }
                        Ok(())
                    }
                    Some(i) => {
                        let n = self.sender.send(&events[..i])?;
                        if n as usize != i {
                            return Err(InjectError::Partial { sent: n, expected: i });
                        }
                        let n = self.sender.send(&events[i..])?;
                        if n as usize != events.len() - i {
                            return Err(InjectError::Partial { sent: n, expected: events.len() - i });
                        }
                        Ok(())
                    }
                }
            }
        }
    }
}
```

`SendingGuard` Drop clears the flag on partial error — do not write a second `store(false)` that can race.

- [x] **Step 1: Tests**

```rust
fn inj(profile: InjectProfile) -> ProfilingInjector<VecSender> {
    ProfilingInjector {
        profile,
        sender: VecSender { batches: vec![] },
        sending: Arc::new(AtomicBool::new(false)),
    }
}

struct VecSender { batches: Vec<Vec<SynthesizedEvent>> }
impl InputSender for VecSender {
    fn send(&mut self, events: &[SynthesizedEvent]) -> Result<u32, InjectError> {
        self.batches.push(events.to_vec());
        Ok(events.len() as u32)
    }
}

struct PartialSender;
impl InputSender for PartialSender {
    fn send(&mut self, events: &[SynthesizedEvent]) -> Result<u32, InjectError> {
        Ok(events.len().saturating_sub(1) as u32)
    }
}

#[test]
fn win32_replace_one_batch_contents() {
    let mut injector = inj(InjectProfile::Win32);
    injector.replace(1, "à").unwrap();
    assert_eq!(injector.sender.batches.len(), 1);
    assert_eq!(injector.sender.batches[0][0], SynthesizedEvent::Backspace);
    assert!(!injector.sending.load(Ordering::SeqCst));
}

#[test]
fn electron_replace_two_batches() {
    let mut injector = inj(InjectProfile::Electron);
    injector.replace(1, "à").unwrap();
    assert_eq!(injector.sender.batches.len(), 2);
    assert_eq!(injector.sender.batches[0], vec![SynthesizedEvent::Backspace]);
}

#[test]
fn electron_zero_backspace_one_batch() {
    let mut injector = inj(InjectProfile::Electron);
    injector.replace(0, "a").unwrap();
    assert_eq!(injector.sender.batches.len(), 1);
}

#[test]
fn partial_send_is_error_and_clears_sending() {
    let mut injector = ProfilingInjector {
        profile: InjectProfile::Win32,
        sender: PartialSender,
        sending: Arc::new(AtomicBool::new(false)),
    };
    let err = injector.replace(1, "a").unwrap_err();
    assert!(matches!(err, InjectError::Partial { .. }));
    assert!(!injector.sending.load(Ordering::SeqCst));
}
```

`InjectError` includes `Partial { sent: u32, expected: usize }`. **No `thread::sleep` in `inject.rs`.**

Add `to_win32_events(events: &[SynthesizedEvent]) -> Vec<(u16, u32)>` mapping each Backspace to VK_BACK down+up and each Utf16 to UNICODE down+up. Test: one Backspace → 2 INPUT intents; `"a"` → 2 UNICODE intents.

- [x] **Step 2:** FAIL. **Step 3:** implement as above. **Step 4:** PASS. Commit `feat(win): batch win32 and electron inject without sleeping`

---

### Task 5: Classify exe → profile

**Files:** `src/classify.rs`. Test: `tests/classify.rs`.

```rust
pub fn profile_for_exe(path_or_name: &str) -> InjectProfile {
    let name = std::path::Path::new(path_or_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path_or_name);
    const ELECTRON: &[&str] = &[
        "Cursor.exe", "Code.exe", "chrome.exe", "msedge.exe",
        "firefox.exe", "Discord.exe", "Slack.exe", "WhatsApp.exe",
    ];
    if ELECTRON.iter().any(|e| name.eq_ignore_ascii_case(e)) {
        InjectProfile::Electron
    } else {
        InjectProfile::Win32
    }
}
```

- [x] **Tests:**

```rust
#[test]
fn cursor_is_electron() {
    assert_eq!(profile_for_exe(r"C:\Users\x\Cursor.exe"), InjectProfile::Electron);
    assert_eq!(profile_for_exe("cursor.EXE"), InjectProfile::Electron);
}

#[test]
fn notepad_is_win32() {
    assert_eq!(profile_for_exe("notepad.exe"), InjectProfile::Win32);
}
```

- [x] FAIL / implement / PASS. Commit `feat(win): classify electron vs win32 inject profiles`

---

### Task 6: Session visual API + host (no hook)

**Files:** Modify `openvikey-session/src/session.rs` (`accept_top`/`undo_last` return visuals; add `clone_model`). Create `src/host.rs`. Tests: `crates/openvikey-session` unit via existing `session_capture` + `crates/openvikey-win/tests/host.rs`.

**Produces:**

```rust
// in openvikey-session
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptVisual {
    pub candidate_nfc: String,
    pub was_composing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoVisual {
    pub show_nfc: String,
}

impl LabSession {
    pub fn accept_top(&mut self, at_ms: i64) -> Option<AcceptVisual> { /* existing body; return Some at success */ }
    pub fn undo_last(&mut self, at_ms: i64) -> Option<UndoVisual> { /* show_nfc = outcome.inverse.replacement */ }
    pub fn clone_model(&self) -> AdaptiveModel { self.model().clone() }
}
```

Lab `repl.rs` match **must** use blocks (arms cannot mix `Option<AcceptVisual>` and `Option<UndoVisual>`):

```rust
ReplAction::AcceptTop => {
    let _ = session.accept_top(at_ms);
}
ReplAction::RejectTop => session.reject_top(at_ms),
ReplAction::UndoLast => {
    let _ = session.undo_last(at_ms);
}
```

Add `commands_from_accept` / `commands_from_undo` in `sync.rs`:

```rust
pub fn commands_from_accept(
    visual: &AcceptVisual,
    sent_nfc: &str,
    last_injected_token: &str,
) -> (Vec<InjectCommand>, String, String) {
    if visual.was_composing {
        (
            vec![
                InjectCommand::Replace {
                    backspace_graphemes: grapheme_len(sent_nfc),
                    text_nfc: visual.candidate_nfc.clone(),
                },
                InjectCommand::AppendDelimiter { delimiter: ' ' },
            ],
            String::new(),
            visual.candidate_nfc.clone(),
        )
    } else {
        (
            vec![InjectCommand::Replace {
                backspace_graphemes: grapheme_len(last_injected_token),
                text_nfc: visual.candidate_nfc.clone(),
            }],
            String::new(),
            visual.candidate_nfc.clone(),
        )
    }
}

pub fn commands_from_undo(visual: &UndoVisual, last_injected_token: &str) -> Vec<InjectCommand> {
    vec![InjectCommand::Replace {
        backspace_graphemes: grapheme_len(last_injected_token),
        text_nfc: visual.show_nfc.clone(),
    }]
}
```

`was_composing`: `!self.engine.snapshot().is_empty()` **before** reset in current `accept_top`.

```rust
pub struct SessionSaveSnapshot {
    pub model: AdaptiveModel,
    pub capture_records: Vec<CaptureRecord>,
    pub cursors: SessionCursors,
    pub last_at_ms: i64,
}

impl LabSession {
    pub fn save_snapshot(&self) -> SessionSaveSnapshot {
        SessionSaveSnapshot {
            model: self.model().clone(),
            capture_records: self.capture.clone(),
            cursors: self.cursors(),
            last_at_ms: self.last_at_ms,
        }
    }
}
```

Do **not** call `capture_log()` on the save path (`capture_log` runs `model_payload()`).

- [x] **Step 1:** `session_capture` PASS after return types. REPL compiles.

- [x] **Step 2: Host tests** — `TypingHost` fields: `session`, `sent`, `last_injected_token`, `last_injected_hwnd: u64`, `hwnd: u64`.

```rust
#[test]
fn typed_letter_records_replace() { /* handle_key 'x' → recorded not empty */ }

#[test]
fn space_commit_updates_last_injected_token() {
    let mut host = TypingHost::new_telex_fixture();
    for vk in [0x58u16, 0x49, 0x4E, 0x20] { // x i n space
        host.handle_key(key(vk), 1);
    }
    assert_eq!(host.last_injected_token, "xin");
}

#[test]
fn focus_change_does_not_backspace() {
    let mut host = TypingHost::new_telex_fixture();
    host.handle_key(key(0x41), 1);
    host.recorded.clear();
    host.set_hwnd(99, "Cursor.exe".into(), 2);
    assert!(host.recorded.is_empty());
    assert!(host.last_injected_token.is_empty());
}

#[test]
fn accept_composing_replaces_sent() { /* after "ko" still composing, AcceptVisual → Replace + space */ }

#[test]
fn accept_after_commit_same_hwnd_replaces_token() { /* commit xin, accept → replace last_injected */ }

#[test]
fn accept_after_commit_other_hwnd_is_noop_visual() {
    host.set_hwnd(2, "notepad.exe".into(), 3);
    host.handle_hotkey(HostHotkey::AcceptTop, 4);
    assert!(host.recorded.is_empty());
}

#[test]
fn undo_replaces_last_injected() { /* commands_from_undo */ }

#[test]
fn try_lock_fail_on_letter_is_pass() { /* other thread holds Mutex */ }

#[test]
fn save_snapshot_does_not_call_to_json() {
    let snap = session.save_snapshot();
    let _ = snap.model.to_json_payload().unwrap();
}
```

`handle_key` is **synchronous** (no queue). Enter: inject commands first, then the caller returns `CommitAndPass` so the hook can `CallNextHookEx` after `handle_key` returns. Test `enter_runs_after_replace_on_same_stack`: a `RecordingInjector` that pushes `"enter"` only when `handle_key` for Enter is invoked sees prior Replace in `recorded`.

`handle_key_locked`: `try_lock` fail → `Pass` for letters; for `VK_RETURN` return `CommitAndPass` **without** processing is wrong — return a new `KeyDecision::EatAndIgnore` equivalent: add `fn on_try_lock_fail(raw) -> KeyDecision` = Enter → eat (do not pass), letter → Pass. Test both.

P95: **not in this file**. Optional `tests/host_perf.rs` with `#[ignore]`.

- [x] `cargo test -p openvikey-lab --test session_capture` PASS
- [x] `cargo test -p openvikey-lab --test repl_keymap` PASS if present; `cargo test -p openvikey-lab` PASS
- [x] `cargo test -p openvikey-win --test host` PASS
- [x] Commit `feat(win): add typing host and accept/undo visuals`

---

### Task 7: Hook return semantics (no key queue)

**Files:** `src/hook.rs`. Tests: `tests/hook.rs`, `tests/hook_thread_discipline.rs`.

```rust
pub fn ll_return(decision: &KeyDecision) -> isize {
    // 0 = CallNextHookEx, 1 = eat
    match decision {
        KeyDecision::Pass | KeyDecision::CommitAndPass { .. } | KeyDecision::CaretBreakAndPass => 0,
        _ => 1,
    }
}

#[test]
fn ovk_pass_does_not_eat() {
    assert_eq!(ll_return(&KeyDecision::Pass), 0);
}

#[test]
fn eat_and_inject_eats() {
    assert_eq!(
        ll_return(&KeyDecision::EatAndInject(InputKind::Backspace)),
        1
    );
}

#[test]
fn commit_and_pass_forwards_after_handle_key() {
    assert_eq!(ll_return(&KeyDecision::CommitAndPass { delimiter: '\n' }), 0);
}
```

Callback order (document in `hook.rs` comment, implement): `decide` → if need session, `try_lock` + `host.handle_key` (SendInput) → `ll_return(decision)`. **No `mpsc` for keys.**

Discipline: `hook.rs`/`mouse.rs` must not contain `lock(`, `sleep`, `model_payload`, `to_payload`, `recv(`. `try_lock` only in `host.rs`.

```rust
#[test]
fn raw_from_ll_keyup() {
    assert!(!raw_from_ll(0x41, 0x0080, 0).down);
}
```

- [x] FAIL / implement / PASS. Commit `feat(win): define ll hook return semantics without a key queue`

---

### Task 8: Mouse + focus cache

**Files:** `src/mouse.rs`, `src/focus.rs`. Test: `tests/focus.rs`.

```rust
#[test]
fn focus_cache_roundtrip() {
    let cache = FocusCache::new();
    cache.set(1, "notepad.exe");
    assert_eq!(cache.get(), (1, "notepad.exe".into()));
}

#[test]
fn mouse_lbutton_is_caret_break() {
    assert_eq!(mouse_decision(0x0201), KeyDecision::CaretBreakAndPass); // WM_LBUTTONDOWN
}
```

Winevent install: `unsafe` in `focus.rs`, not unit-tested. Ordering: focus `set` happens on winevent thread; hook `try_read`s cache.

- [x] Commit `feat(win): add focus cache and mouse caret-break`

---

### Task 9: Overlay + tray

**Files:** `src/overlay.rs`, `src/tray.rs`. Tests: `tests/overlay.rs`, `tests/tray.rs`.

```rust
pub fn overlay_lines(candidates: &[String], max: usize) -> Vec<String> {
    candidates.iter().take(max).cloned().collect()
}

#[test]
fn overlay_caps_at_three() {
    let lines = overlay_lines(&["a".into(), "b".into(), "c".into(), "d".into()], 3);
    assert_eq!(lines, ["a", "b", "c"]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent { LeftClick, Exit }

pub fn tray_hotkey(event: TrayEvent) -> Option<HostHotkey> {
    match event {
        TrayEvent::LeftClick => Some(HostHotkey::ToggleMode),
        TrayEvent::Exit => None,
    }
}

#[test]
fn left_click_toggles_mode() {
    assert_eq!(tray_hotkey(TrayEvent::LeftClick), Some(HostHotkey::ToggleMode));
}
```

HWND overlay `#[cfg(windows)]` untested.

- [x] Commit `feat(win): add overlay lines and tray events`

---

### Task 10: Passphrase + persist load errors + main coordinator

**Files:** `src/passphrase.rs`, `src/main.rs`. Tests: `tests/passphrase.rs`, `tests/persist.rs`.

```rust
pub fn apply_passphrase_key(buffer: &mut String, vk: u16, ch: Option<char>) -> bool {
    match vk {
        0x0D => true,
        0x08 => { buffer.pop(); false }
        _ => {
            if let Some(c) = ch { buffer.push(c); }
            false
        }
    }
}

#[test]
fn passphrase_backspace() {
    let mut b = String::from("ab");
    assert!(!apply_passphrase_key(&mut b, 0x08, None));
    assert_eq!(b, "a");
}
```

Windows `read_hidden_passphrase` uses `ReadConsoleW` + `SetConsoleMode` without echo. **Zero `crossterm` in this crate's Cargo.toml.**

`persist.rs` tests (session crate or win): missing model → default; wrong passphrase → error; `save_snapshot` then seal both files with matching sha.

`main.rs`: clap; `%LOCALAPPDATA%\OpenViKey`; install hook+mouse+winevent; message loop; Exit: unhook, `flush` savers. Shutdown test: `HostShutdown::run()` sets a flag consumed by a fake loop — unit test the flag, not GetMessage.

- [x] Commit `feat(win): add passphrase, persist paths, and host main`

---

### Task 11: Deny, no_key_log, README, manual gate

```rust
#[test]
fn no_println_on_hot_path() {
    for path in ["src/hook.rs", "src/inject.rs", "src/host.rs"] {
        let src = std::fs::read_to_string(path).unwrap();
        assert!(!src.contains("println!"), "{path}");
        assert!(!src.contains("eprintln!"), "{path}");
    }
}
```

README runbook only. Manual checklist (not CI): Notepad newline; Cursor Enter submits **after** visible Vietnamese; UniKey off.

- [x] `cargo deny check`
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `cargo test --workspace --all-features`
- [x] Commit `docs: add openvikey-win runbook and hook log discipline tests`

---

## Spec coverage

| Spec | Task |
|---|---|
| Extract | 1 |
| Policy including OVK Pass, Backspace, Ctrl+C, Caps, hotkeys | 2 |
| action vs engine_actions, punctuation delimiter | 3 |
| AtomicBool guard, partial SendInput | 4 |
| Classify | 5 |
| Accept/Undo/HWND/save_snapshot/REPL match | 6 |
| Hook return, no key queue | 7 |
| Mouse/focus | 8 |
| Overlay/tray | 9 |
| Passphrase/main/persist | 10 |
| no_key_log host.rs, deny, manual | 11 |
| session_capture | 1, 6 |

## Self-review

- OVK_EXTRA is Pass, not EatAndIgnore.
- No key mpsc; Enter CallNextHookEx after synchronous inject.
- `capture_log()` is not on the save path.
- REPL match uses `let _ =`.
- `windows` 0.62.2; no `[lints.clippy] workspace = true`.
- P95 is not a CI unit assert.

