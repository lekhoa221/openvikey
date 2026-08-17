//! Low-level keyboard hook return semantics (no OS install in these tests).

use openvikey_core::types::InputKind;
use openvikey_win::hook::{ll_return, raw_from_ll};
use openvikey_win::policy::KeyDecision;

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
