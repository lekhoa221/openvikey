//! Low-level keyboard hook return semantics (no OS install in these tests).

use std::sync::Mutex;

use openvikey_core::types::InputKind;
use openvikey_win::hook::{dispatch_ll, ll_return, raw_from_ll};
use openvikey_win::host::TypingHost;
use openvikey_win::policy::{KeyDecision, RawKey};

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

#[test]
fn raw_from_ll_keyup() {
    assert!(!raw_from_ll(0x41, 0x0080, 0).down);
}

#[test]
fn dispatch_ll_eat_and_inject_returns_one() {
    let host = Mutex::new(TypingHost::new_telex_fixture());
    // Letter 'x' → EatAndInject → eat (1)
    assert_eq!(dispatch_ll(&host, key(0x58), 1), 1);
}

#[test]
fn dispatch_ll_commit_and_pass_returns_zero() {
    let host = Mutex::new(TypingHost::new_telex_fixture());
    let _ = dispatch_ll(&host, key(0x58), 1); // x
    // Enter → CommitAndPass → CallNextHookEx (0)
    assert_eq!(dispatch_ll(&host, key(0x0D), 2), 0);
}
