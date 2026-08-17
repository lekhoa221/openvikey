//! T6: key → InputEvent / hotkey mapping without a TTY.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use openvikey_core::types::InputKind;
use openvikey_lab::repl::{ReplAction, key_event_to_action, next_event_at_ms};

fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    let mut event = KeyEvent::new(code, modifiers);
    event.kind = KeyEventKind::Press;
    event
}

#[test]
fn printable_and_space_and_enter_map_to_input() {
    assert_eq!(
        key_event_to_action(&press(KeyCode::Char('a'), KeyModifiers::NONE)),
        Some(ReplAction::Input(InputKind::Key {
            logical: 'a',
            physical: None,
        }))
    );
    assert_eq!(
        key_event_to_action(&press(KeyCode::Char(' '), KeyModifiers::NONE)),
        Some(ReplAction::Input(InputKind::Boundary { delimiter: ' ' }))
    );
    assert_eq!(
        key_event_to_action(&press(KeyCode::Enter, KeyModifiers::NONE)),
        Some(ReplAction::Input(InputKind::Boundary { delimiter: ' ' }))
    );
    assert_eq!(
        key_event_to_action(&press(KeyCode::Backspace, KeyModifiers::NONE)),
        Some(ReplAction::Input(InputKind::Backspace))
    );
}

#[test]
fn hotkeys_map_to_feedback_and_quit() {
    assert_eq!(
        key_event_to_action(&press(KeyCode::Tab, KeyModifiers::NONE)),
        Some(ReplAction::AcceptTop)
    );
    assert_eq!(
        key_event_to_action(&press(KeyCode::Esc, KeyModifiers::NONE)),
        Some(ReplAction::RejectTop)
    );
    assert_eq!(
        key_event_to_action(&press(KeyCode::Char('z'), KeyModifiers::CONTROL)),
        Some(ReplAction::UndoLast)
    );
    assert_eq!(
        key_event_to_action(&press(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Some(ReplAction::Quit)
    );
    assert_eq!(
        key_event_to_action(&press(KeyCode::Char('d'), KeyModifiers::CONTROL)),
        Some(ReplAction::Quit)
    );
}

#[test]
fn key_release_is_ignored() {
    let mut event = press(KeyCode::Char('a'), KeyModifiers::NONE);
    event.kind = KeyEventKind::Release;
    assert_eq!(key_event_to_action(&event), None);
}

#[test]
fn unicode_ellipsis_maps_to_boundary() {
    assert_eq!(
        key_event_to_action(&press(KeyCode::Char('…'), KeyModifiers::NONE)),
        Some(ReplAction::Input(InputKind::Boundary { delimiter: '…' }))
    );
}

#[test]
fn event_clock_uses_wall_time_and_stays_monotonic() {
    assert_eq!(next_event_at_ms(100, 50), 101);
    assert_eq!(next_event_at_ms(100, 1_700_000_000_000), 1_700_000_000_000);
}
