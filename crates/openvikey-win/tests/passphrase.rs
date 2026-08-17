//! Passphrase buffer key helper (no console I/O in these tests).

use openvikey_win::passphrase::apply_passphrase_key;

#[test]
fn passphrase_backspace() {
    let mut b = String::from("ab");
    assert!(!apply_passphrase_key(&mut b, 0x08, None));
    assert_eq!(b, "a");
}

#[test]
fn passphrase_enter_finishes() {
    let mut b = String::from("secret");
    assert!(apply_passphrase_key(&mut b, 0x0D, None));
    assert_eq!(b, "secret");
}

#[test]
fn passphrase_char_appends() {
    let mut b = String::new();
    assert!(!apply_passphrase_key(&mut b, 0x41, Some('a')));
    assert!(!apply_passphrase_key(&mut b, 0x42, Some('b')));
    assert_eq!(b, "ab");
}
