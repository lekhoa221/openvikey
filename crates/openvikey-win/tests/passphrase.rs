//! The Windows development host must start without passphrase authentication.

#[test]
fn windows_host_runtime_has_no_passphrase_prompt_or_provider() {
    let main = std::fs::read_to_string("src/main.rs").unwrap();
    let lib = std::fs::read_to_string("src/lib.rs").unwrap();
    let manifest = std::fs::read_to_string("Cargo.toml").unwrap();

    for forbidden in [
        "read_hidden_passphrase",
        "PassphraseProvider",
        "spawn_encrypted",
        "KdfConfig",
    ] {
        assert!(
            !main.contains(forbidden),
            "openvikey-win runtime must not depend on {forbidden}"
        );
    }
    assert!(!lib.contains("pub mod passphrase"));
    assert!(!manifest.contains("zeroize"));
}
