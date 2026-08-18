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
    assert_eq!(
        ll_return(&KeyDecision::CommitAndPass { delimiter: '\n' }),
        0
    );
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
fn dispatch_ll_backspace_eats_in_composition_but_passes_after_commit() {
    let composing = Mutex::new(TypingHost::new_telex_fixture());
    assert_eq!(dispatch_ll(&composing, key(0x58), 1), 1); // x
    composing.lock().unwrap().recorded.clear();
    assert_eq!(dispatch_ll(&composing, key(0x08), 2), 1);
    assert!(!composing.lock().unwrap().recorded.is_empty());

    let committed = Mutex::new(TypingHost::new_telex_fixture());
    assert_eq!(dispatch_ll(&committed, key(0x58), 1), 1); // x
    assert_eq!(dispatch_ll(&committed, key(0x20), 2), 1); // commit + space
    committed.lock().unwrap().recorded.clear();
    assert_eq!(dispatch_ll(&committed, key(0x08), 3), 0);
    assert!(committed.lock().unwrap().recorded.is_empty());
}

#[test]
fn dispatch_ll_commit_and_pass_returns_zero() {
    let host = Mutex::new(TypingHost::new_telex_fixture());
    let _ = dispatch_ll(&host, key(0x58), 1); // x
    // Enter → CommitAndPass → CallNextHookEx (0)
    assert_eq!(dispatch_ll(&host, key(0x0D), 2), 0);
}

#[test]
fn dispatch_ll_letter_passes_when_inject_fails() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use openvikey_win::inject::{
        InjectError, InjectProfile, InputSender, ProfilingInjector, SynthesizedEvent,
    };

    struct FailSender;
    impl InputSender for FailSender {
        fn send(&mut self, events: &[SynthesizedEvent]) -> Result<u32, InjectError> {
            Err(InjectError::Partial {
                sent: 0,
                expected: events.len(),
            })
        }
    }

    let mut typing = TypingHost::new_telex_fixture();
    let before = typing.session.composition_text();
    typing.set_injector(Box::new(ProfilingInjector {
        profile: InjectProfile::Win32,
        sender: FailSender,
        sending: Arc::new(AtomicBool::new(false)),
    }));
    let host = Mutex::new(typing);
    // Letter would be EatAndInject, but inject fails → Pass (0), do not eat.
    assert_eq!(dispatch_ll(&host, key(0x58), 1), 0);
    let guard = host.lock().unwrap();
    assert_eq!(
        guard.session.composition_text(),
        before,
        "session must roll back when SendInput fails"
    );
    assert!(guard.recorded.is_empty());
    assert!(guard.sent.is_empty());
}

#[test]
fn dispatch_ll_letter_inject_fail_restores_learning_model() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use openvikey_win::inject::{
        InjectError, InjectProfile, InputSender, ProfilingInjector, SynthesizedEvent,
    };

    struct FailSender;
    impl InputSender for FailSender {
        fn send(&mut self, events: &[SynthesizedEvent]) -> Result<u32, InjectError> {
            Err(InjectError::Partial {
                sent: 0,
                expected: events.len(),
            })
        }
    }

    let mut typing = TypingHost::new_telex_fixture();
    // Non-empty composition / Suggest path: "k" then failing "o" would record_decision for "ko"→"không".
    typing.handle_key(key(0x4B), 1);
    let before_composition = typing.session.composition_text();
    let before_model = typing.session.clone_model();
    assert!(!before_composition.is_empty());
    typing.set_injector(Box::new(ProfilingInjector {
        profile: InjectProfile::Win32,
        sender: FailSender,
        sending: Arc::new(AtomicBool::new(false)),
    }));
    let host = Mutex::new(typing);
    assert_eq!(dispatch_ll(&host, key(0x4F), 2), 0);
    let guard = host.lock().unwrap();
    assert_eq!(guard.session.composition_text(), before_composition);
    assert_eq!(
        guard.session.clone_model(),
        before_model,
        "learning/model must match pre-key state on letter InjectError"
    );
}

#[test]
fn dispatch_ll_lock_fail_hotkey_eats() {
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    let host = Arc::new(Mutex::new(TypingHost::new_telex_fixture()));
    let held = Arc::clone(&host);
    let blocker = thread::spawn(move || {
        let _guard = held.lock().unwrap();
        thread::sleep(Duration::from_millis(200));
    });
    thread::sleep(Duration::from_millis(20));
    let mut acc = key(0xBE);
    acc.control = true;
    assert_eq!(dispatch_ll(&host, acc, 1), 1);
    let mut letter = key(0x41);
    assert_eq!(dispatch_ll(&host, letter, 2), 0);
    assert_eq!(dispatch_ll(&host, key(0x0D), 3), 1);
    letter.extra_info = openvikey_win::policy::OVK_EXTRA;
    assert_eq!(dispatch_ll(&host, letter, 4), 0);
    blocker.join().unwrap();
}

#[test]
fn dispatch_ll_enter_eats_but_rolls_back_session_on_inject_fail() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use openvikey_win::inject::{
        InjectError, InjectProfile, InputSender, ProfilingInjector, SynthesizedEvent,
    };

    struct FailSender;
    impl InputSender for FailSender {
        fn send(&mut self, events: &[SynthesizedEvent]) -> Result<u32, InjectError> {
            Err(InjectError::Partial {
                sent: 0,
                expected: events.len(),
            })
        }
    }

    let mut typing = TypingHost::new_telex_fixture();
    // Letter succeeds without live injector (record-only).
    typing.handle_key(key(0x58), 1);
    let before_enter = typing.session.composition_text();
    assert!(!before_enter.is_empty());
    // Clear `sent` so Commit emits a Replace (otherwise Enter has zero inject cmds).
    typing.sent.clear();
    typing.set_injector(Box::new(ProfilingInjector {
        profile: InjectProfile::Win32,
        sender: FailSender,
        sending: Arc::new(AtomicBool::new(false)),
    }));
    let host = Mutex::new(typing);
    // Enter CommitAndPass: inject fails → EatAndIgnore (1), session unchanged.
    assert_eq!(dispatch_ll(&host, key(0x0D), 2), 1);
    let guard = host.lock().unwrap();
    assert_eq!(guard.session.composition_text(), before_enter);
}
