//! Lock-free key policy for the WH_KEYBOARD_LL hook path.

use std::cell::Cell;
use std::sync::{Arc, OnceLock};

use arc_swap::ArcSwap;

use openvikey_core::engine::backend::is_boundary_char;
use openvikey_core::types::InputKind;
use openvikey_win_context::ContextState;

use crate::settings::{AppTransformPolicy, HotkeySettingsV1};

/// Marker stamped on our own `SendInput` events so the hook must Pass them.
pub const OVK_EXTRA: usize = 0x4F56_4B31;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Viet,
    English,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct HostState {
    pub mode: Mode,
    pub foreground_exe: String,
    pub is_sending: bool,
    pub allow_terminal: bool,
    pub app_transform: AppTransformPolicy,
    pub caps_lock: bool,
    pub alt: bool,
    pub meta: bool,
    pub context_state: ContextState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
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
pub enum HostHotkey {
    AcceptTop,
    RejectTop,
    UndoLast,
    ForgetLastRule,
    ToggleMode,
    ToggleSuggestions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyDecision {
    Pass,
    EatAndIgnore,
    EatAndInject(InputKind),
    CommitAndPass { delimiter: char },
    Hotkey(HostHotkey),
    CaretBreakAndPass,
}

const TERMINAL_APPS: &[&str] = &[
    "WindowsTerminal.exe",
    "powershell.exe",
    "pwsh.exe",
    "cmd.exe",
    "conhost.exe",
];

const AMBIGUOUS_TERMINAL_HOSTS: &[&str] = &["Cursor.exe", "Code.exe"];

const DENYLIST: &[&str] = &[
    "OpenSSH.exe",
    "ssh.exe",
    "putty.exe",
    "1Password.exe",
    "KeePass.exe",
    "KeePassXC.exe",
    "Bitwarden.exe",
    "LogonUI.exe",
    "CredentialUIBroker.exe",
];

/// Whether the LL hook should call `CallNextHookEx` for this decision.
#[must_use]
pub fn hook_allows_next(decision: &KeyDecision) -> bool {
    matches!(
        decision,
        KeyDecision::Pass | KeyDecision::CommitAndPass { .. } | KeyDecision::CaretBreakAndPass
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct HotkeyChord {
    vk: u16,
    control: bool,
    shift: bool,
    alt: bool,
    meta: bool,
}

#[derive(Debug, Clone, Copy)]
struct RuntimeHotkeys {
    default_toggle: bool,
    toggle: Option<HotkeyChord>,
    accept: HotkeyChord,
    reject: HotkeyChord,
    undo: HotkeyChord,
    forget: HotkeyChord,
}

static HOTKEYS: OnceLock<Arc<ArcSwap<RuntimeHotkeys>>> = OnceLock::new();

fn parse_hotkey(value: &str, allow_modifier_pair: bool) -> Result<Option<HotkeyChord>, String> {
    if allow_modifier_pair && value.eq_ignore_ascii_case("LeftCtrl+LeftShift") {
        return Ok(None);
    }
    let parts = value.split('+').map(str::trim).collect::<Vec<_>>();
    let Some(key) = parts.last() else {
        return Err("phím tắt trống".into());
    };
    let control = parts[..parts.len().saturating_sub(1)]
        .iter()
        .any(|part| part.eq_ignore_ascii_case("Ctrl"));
    let shift = parts[..parts.len().saturating_sub(1)]
        .iter()
        .any(|part| part.eq_ignore_ascii_case("Shift"));
    let alt = parts[..parts.len().saturating_sub(1)]
        .iter()
        .any(|part| part.eq_ignore_ascii_case("Alt"));
    let meta = parts[..parts.len().saturating_sub(1)]
        .iter()
        .any(|part| part.eq_ignore_ascii_case("Win"));
    if !control && !shift && !alt && !meta {
        return Err(format!("phím tắt '{value}' cần Ctrl, Shift, Alt hoặc Win"));
    }
    let vk = match *key {
        "." => 0xBE,
        "," => 0xBC,
        value if value.eq_ignore_ascii_case("Space") => 0x20,
        value if value.len() == 1 => {
            let byte = value.as_bytes()[0].to_ascii_uppercase();
            if !byte.is_ascii_alphanumeric() {
                return Err(format!("phím chính không hỗ trợ: {value}"));
            }
            u16::from(byte)
        }
        _ => return Err(format!("phím chính không hỗ trợ: {key}")),
    };
    Ok(Some(HotkeyChord {
        vk,
        control,
        shift,
        alt,
        meta,
    }))
}

fn parsed_hotkeys(settings: &HotkeySettingsV1) -> Result<RuntimeHotkeys, String> {
    let toggle = parse_hotkey(&settings.toggle_mode, true)?;
    let parsed = RuntimeHotkeys {
        default_toggle: toggle.is_none(),
        toggle,
        accept: parse_hotkey(&settings.accept_top, false)?.ok_or("thiếu phím Accept")?,
        reject: parse_hotkey(&settings.reject_top, false)?.ok_or("thiếu phím Reject")?,
        undo: parse_hotkey(&settings.undo_last, false)?.ok_or("thiếu phím Undo")?,
        forget: parse_hotkey(&settings.forget_last, false)?.ok_or("thiếu phím Forget")?,
    };
    let mut chords = vec![parsed.accept, parsed.reject, parsed.undo, parsed.forget];
    if let Some(toggle) = parsed.toggle {
        chords.push(toggle);
    }
    for (index, chord) in chords.iter().enumerate() {
        if chords[index + 1..].contains(chord) {
            return Err("hai hành động không thể dùng cùng một phím tắt".into());
        }
    }
    Ok(parsed)
}

fn default_hotkeys() -> RuntimeHotkeys {
    parsed_hotkeys(&HotkeySettingsV1::default()).expect("default hotkeys are valid")
}

fn hotkey_slot() -> &'static Arc<ArcSwap<RuntimeHotkeys>> {
    HOTKEYS.get_or_init(|| Arc::new(ArcSwap::from_pointee(default_hotkeys())))
}

/// Validate and atomically replace hook hotkeys.
pub fn set_runtime_hotkeys(settings: &HotkeySettingsV1) -> Result<(), String> {
    let parsed = parsed_hotkeys(settings)?;
    hotkey_slot().store(Arc::new(parsed));
    Ok(())
}

thread_local! {
    static TOGGLE_BOTH: Cell<bool> = const { Cell::new(false) };
    static TOGGLE_DIRTY: Cell<bool> = const { Cell::new(false) };
}

/// Decide how the keyboard LL hook should treat `raw` given `state`.
#[must_use]
pub fn decide(raw: &RawKey, state: &HostState) -> KeyDecision {
    // 1. Own SendInput must always reach the app.
    if raw.extra_info == OVK_EXTRA {
        return KeyDecision::Pass;
    }

    // TSF-sensitive or unresolved active contexts must see physical input unchanged.
    if matches!(
        state.context_state,
        ContextState::Sensitive | ContextState::Pending | ContextState::Unavailable
    ) {
        return KeyDecision::Pass;
    }

    let hotkeys = hotkey_slot().load();
    if hotkeys.default_toggle {
        note_toggle_chord(raw);
    }

    // 2. Swallow physical leak/auto-repeat while we are injecting.
    if state.is_sending {
        return KeyDecision::EatAndIgnore;
    }

    // Keyup: only Toggle chord is special; everything else Passes.
    if !raw.down {
        if hotkeys.default_toggle && is_toggle_chord(raw) {
            return KeyDecision::Hotkey(HostHotkey::ToggleMode);
        }
        return KeyDecision::Pass;
    }

    if raw.down
        && hotkeys
            .toggle
            .is_some_and(|chord| chord_matches(chord, raw, state))
    {
        return KeyDecision::Hotkey(HostHotkey::ToggleMode);
    }

    // 3. Tab / Esc
    if raw.vk == 0x09 || raw.vk == 0x1B {
        return KeyDecision::Pass;
    }

    // 4. Unknown/sensitive targets, non-opted-in terminals, or English → Pass.
    if state.mode == Mode::English
        || state.foreground_exe.is_empty()
        || is_denylisted(&state.foreground_exe)
        || state.app_transform == AppTransformPolicy::Block
        || (requires_terminal_opt_in(&state.foreground_exe) && !state.allow_terminal)
    {
        return KeyDecision::Pass;
    }

    // 6 (before 5): detect OpenViKey hotkeys so step 5 can Pass other modifiers.
    if let Some(hk) = match_hotkey_keydown(raw, state, &hotkeys) {
        return KeyDecision::Hotkey(hk);
    }

    // 5. Non-OVK modifier chords Pass (Ctrl+C/V/X/A, Alt+…, Win+…)
    if raw.control || state.alt || state.meta {
        return KeyDecision::Pass;
    }

    // 7. Navigation / delete → caret break
    if is_nav_vk(raw.vk) {
        return KeyDecision::CaretBreakAndPass;
    }

    // 8. Enter
    if raw.vk == 0x0D {
        return KeyDecision::CommitAndPass { delimiter: '\n' };
    }

    // 9. Space
    if raw.vk == 0x20 {
        return KeyDecision::EatAndInject(InputKind::Boundary { delimiter: ' ' });
    }

    // 10. Backspace
    if raw.vk == 0x08 {
        return KeyDecision::EatAndInject(InputKind::Backspace);
    }

    // 11. Letters / digits
    if let Some(logical) = map_letter_or_digit(raw, state) {
        return KeyDecision::EatAndInject(InputKind::Key {
            logical,
            physical: None,
        });
    }

    // 12. ASCII punctuation that is a boundary
    if let Some(ch) = map_punctuation(raw)
        && is_boundary_char(ch)
    {
        return KeyDecision::EatAndInject(InputKind::Boundary { delimiter: ch });
    }

    KeyDecision::Pass
}

fn executable_name(exe: &str) -> &str {
    exe.rsplit(['/', '\\']).next().unwrap_or(exe)
}

#[must_use]
pub fn is_terminal_exe(exe: &str) -> bool {
    let name = executable_name(exe);
    TERMINAL_APPS
        .iter()
        .any(|terminal| name.eq_ignore_ascii_case(terminal))
}

#[must_use]
pub fn is_denylisted(exe: &str) -> bool {
    let name = executable_name(exe);
    DENYLIST
        .iter()
        .any(|denied| name.eq_ignore_ascii_case(denied))
}

#[must_use]
pub fn allows_learning(exe: &str) -> bool {
    !is_terminal_exe(exe) && !is_denylisted(exe)
}

fn requires_terminal_opt_in(exe: &str) -> bool {
    let name = executable_name(exe);
    is_terminal_exe(exe)
        || AMBIGUOUS_TERMINAL_HOSTS
            .iter()
            .any(|host| name.eq_ignore_ascii_case(host))
}

fn note_toggle_chord(raw: &RawKey) {
    let both = raw.left_ctrl && raw.left_shift;
    let was_both = TOGGLE_BOTH.get();
    if both && !was_both {
        TOGGLE_DIRTY.set(false);
    }
    if both && raw.down && raw.vk != 0xA0 && raw.vk != 0xA2 {
        TOGGLE_DIRTY.set(true);
    }
    if !both {
        TOGGLE_DIRTY.set(false);
    }
    TOGGLE_BOTH.set(both);
}

fn is_toggle_chord(raw: &RawKey) -> bool {
    // Both Left-Ctrl and Left-Shift held; keyup of either (VK_LSHIFT / VK_LCONTROL);
    // no other key was down during the chord (spec §3.1.6).
    raw.left_ctrl && raw.left_shift && (raw.vk == 0xA0 || raw.vk == 0xA2) && !TOGGLE_DIRTY.get()
}

fn chord_matches(chord: HotkeyChord, raw: &RawKey, state: &HostState) -> bool {
    raw.vk == chord.vk
        && raw.control == chord.control
        && raw.shift == chord.shift
        && state.alt == chord.alt
        && state.meta == chord.meta
}

fn match_hotkey_keydown(
    raw: &RawKey,
    state: &HostState,
    hotkeys: &RuntimeHotkeys,
) -> Option<HostHotkey> {
    [
        (hotkeys.forget, HostHotkey::ForgetLastRule),
        (hotkeys.accept, HostHotkey::AcceptTop),
        (hotkeys.reject, HostHotkey::RejectTop),
        (hotkeys.undo, HostHotkey::UndoLast),
    ]
    .into_iter()
    .find_map(|(chord, action)| chord_matches(chord, raw, state).then_some(action))
}

fn is_nav_vk(vk: u16) -> bool {
    matches!(
        vk,
        0x25 | // Left
        0x27 | // Right
        0x26 | // Up
        0x28 | // Down
        0x24 | // Home
        0x23 | // End
        0x21 | // Prior
        0x22 | // Next
        0x2E // Delete
    )
}

fn map_letter_or_digit(raw: &RawKey, state: &HostState) -> Option<char> {
    let upper = match raw.vk {
        0x41..=0x5A => char::from_u32(u32::from(raw.vk - 0x41) + u32::from(b'A'))?,
        0x30..=0x39 => {
            if raw.shift {
                return shifted_digit(raw.vk);
            }
            return char::from_u32(u32::from(raw.vk));
        }
        _ => return None,
    };
    // Caps XOR Shift for letters
    let upper_case = state.caps_lock ^ raw.shift;
    Some(if upper_case {
        upper
    } else {
        upper.to_ascii_lowercase()
    })
}

fn shifted_digit(vk: u16) -> Option<char> {
    Some(match vk {
        0x30 => ')',
        0x31 => '!',
        0x32 => '@',
        0x33 => '#',
        0x34 => '$',
        0x35 => '%',
        0x36 => '^',
        0x37 => '&',
        0x38 => '*',
        0x39 => '(',
        _ => return None,
    })
}

fn map_punctuation(raw: &RawKey) -> Option<char> {
    // US QWERTY OEM keys (unshifted / shifted).
    let (base, shifted) = match raw.vk {
        0xBA => (';', ':'),  // OEM_1
        0xBB => ('=', '+'),  // OEM_PLUS
        0xBC => (',', '<'),  // OEM_COMMA
        0xBD => ('-', '_'),  // OEM_MINUS
        0xBE => ('.', '>'),  // OEM_PERIOD
        0xBF => ('/', '?'),  // OEM_2
        0xC0 => ('`', '~'),  // OEM_3
        0xDB => ('[', '{'),  // OEM_4
        0xDC => ('\\', '|'), // OEM_5
        0xDD => (']', '}'),  // OEM_6
        0xDE => ('\'', '"'), // OEM_7
        _ => return None,
    };
    Some(if raw.shift { shifted } else { base })
}
