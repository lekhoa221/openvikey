use openvikey_win::console::{ConsoleControl, apply_console_control};
use openvikey_win::persist::HostShutdown;

#[test]
fn ctrl_c_requests_graceful_shutdown_for_final_flush() {
    let shutdown = HostShutdown::new();
    assert!(apply_console_control(ConsoleControl::CtrlC, &shutdown));
    assert!(shutdown.is_requested());
}

#[test]
fn unrelated_console_events_are_not_claimed() {
    let shutdown = HostShutdown::new();
    assert!(!apply_console_control(ConsoleControl::Other, &shutdown));
    assert!(!shutdown.is_requested());
}
