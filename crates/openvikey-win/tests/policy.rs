use openvikey_core::types::InputKind;
use openvikey_win::policy::{HostHotkey, HostState, KeyDecision, Mode, OVK_EXTRA, RawKey, decide};

fn viet() -> HostState {
    HostState {
        mode: Mode::Viet,
        foreground_exe: "notepad.exe".into(),
        is_sending: false,
        allow_terminal: false,
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
        KeyDecision::EatAndInject(InputKind::Key {
            logical: 'a',
            physical: None
        })
    );
}

#[test]
fn terminal_apps_are_transformed_during_development() {
    for exe in [
        "WindowsTerminal.exe",
        "powershell.exe",
        "pwsh.exe",
        "cmd.exe",
        "conhost.exe",
        "Cursor.exe",
        "Code.exe",
    ] {
        let mut st = viet();
        st.foreground_exe = exe.into();
        st.allow_terminal = true;
        assert_eq!(
            decide(&key(0x41), &st),
            KeyDecision::EatAndInject(InputKind::Key {
                logical: 'a',
                physical: None,
            }),
            "{exe} should allow OpenViKey while the Windows host is under development"
        );
    }
}

#[test]
fn terminals_require_explicit_opt_in() {
    let mut st = viet();
    st.foreground_exe = "WindowsTerminal.exe".into();
    assert_eq!(decide(&key(0x41), &st), KeyDecision::Pass);
}

#[test]
fn security_sensitive_apps_remain_denylisted() {
    for exe in [
        "1Password.exe",
        "KeePass.exe",
        "KeePassXC.exe",
        "Bitwarden.exe",
        "LogonUI.exe",
        "CredentialUIBroker.exe",
        "ssh.exe",
    ] {
        let mut st = viet();
        st.foreground_exe = exe.into();
        st.allow_terminal = true;
        assert_eq!(decide(&key(0x41), &st), KeyDecision::Pass, "{exe}");
    }
}

#[test]
fn unknown_foreground_fails_open_to_the_application_without_transforming() {
    let mut st = viet();
    st.foreground_exe.clear();
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
    assert_eq!(
        decide(&acc, &viet()),
        KeyDecision::Hotkey(HostHotkey::AcceptTop)
    );
    let mut comma = key(0xBC);
    comma.control = true;
    assert_eq!(
        decide(&comma, &viet()),
        KeyDecision::Hotkey(HostHotkey::RejectTop)
    );
    let mut undo = key(0x5A);
    undo.control = true;
    undo.shift = true;
    assert_eq!(
        decide(&undo, &viet()),
        KeyDecision::Hotkey(HostHotkey::UndoLast)
    );
    let mut z = key(0x5A);
    z.control = true;
    assert_ne!(
        decide(&z, &viet()),
        KeyDecision::Hotkey(HostHotkey::UndoLast)
    );
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
        KeyDecision::EatAndInject(InputKind::Key {
            logical: 'A',
            physical: None
        })
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
    // Drop any leftover chord latch on this worker thread.
    let mut idle = key(0x41);
    idle.down = false;
    assert_eq!(decide(&idle, &viet()), KeyDecision::Pass);

    let mut k = key(0xA0); // VK_LSHIFT
    k.down = false;
    k.left_ctrl = true;
    k.left_shift = true;
    assert_eq!(
        decide(&k, &viet()),
        KeyDecision::Hotkey(HostHotkey::ToggleMode)
    );
}

#[test]
fn ctrl_shift_z_then_shift_up_is_not_toggle() {
    let mut idle = key(0x41);
    idle.down = false;
    assert_eq!(decide(&idle, &viet()), KeyDecision::Pass);

    let mut z = key(0x5A);
    z.control = true;
    z.shift = true;
    z.left_ctrl = true;
    z.left_shift = true;
    assert_eq!(
        decide(&z, &viet()),
        KeyDecision::Hotkey(HostHotkey::UndoLast)
    );

    let mut shift_up = key(0xA0);
    shift_up.down = false;
    shift_up.left_ctrl = true;
    shift_up.left_shift = true;
    assert_eq!(decide(&shift_up, &viet()), KeyDecision::Pass);
}

#[test]
fn ovk_extra_pass_allows_next_hook() {
    use openvikey_win::policy::hook_allows_next;
    let mut k = key(0x41);
    k.extra_info = OVK_EXTRA;
    let d = decide(&k, &viet());
    assert_eq!(d, KeyDecision::Pass);
    assert!(hook_allows_next(&d));
}
