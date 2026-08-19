use openvikey_win::sensitive::{FieldVerdict, SensitiveSignals, classify_sensitive_signals};

#[test]
fn password_signal_wins_before_normal_accessibility_result() {
    let verdict = classify_sensitive_signals(SensitiveSignals {
        denied_process: false,
        win32_password: true,
        automation_password: Some(false),
    });

    assert_eq!(verdict, FieldVerdict::Sensitive);
}

#[test]
fn accessibility_false_is_an_explicit_normal_field() {
    let verdict = classify_sensitive_signals(SensitiveSignals {
        denied_process: false,
        win32_password: false,
        automation_password: Some(false),
    });

    assert_eq!(verdict, FieldVerdict::Normal);
}

#[test]
fn inaccessible_custom_field_fails_safe() {
    let verdict = classify_sensitive_signals(SensitiveSignals {
        denied_process: false,
        win32_password: false,
        automation_password: None,
    });

    assert_eq!(verdict, FieldVerdict::Unavailable);
}

#[test]
fn denied_process_is_always_sensitive() {
    let verdict = classify_sensitive_signals(SensitiveSignals {
        denied_process: true,
        win32_password: false,
        automation_password: Some(false),
    });

    assert_eq!(verdict, FieldVerdict::Sensitive);
}
