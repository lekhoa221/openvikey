//! Milestone 8: authenticated envelope encryption and passphrase rewrap.

use openvikey_core::store::envelope::{open, payload_section, rewrap, seal};
use openvikey_core::store::passphrase::{KdfConfig, PassphraseProvider};
use openvikey_core::store::{InMemorySecretProvider, StoreError};

#[test]
fn encrypted_blob_hides_plaintext_and_round_trips() {
    let provider = InMemorySecretProvider::new();
    let payload = br#"{"version":1,"secret":"toi go tieng Viet"}"#;
    let blob = seal(payload, &provider).unwrap();

    assert!(!blob.windows(payload.len()).any(|window| window == payload));
    assert_eq!(open(&blob, &provider).unwrap(), payload);
}

#[test]
fn each_write_uses_a_unique_payload_nonce() {
    let provider = InMemorySecretProvider::new();
    let first = seal(b"same model", &provider).unwrap();
    let second = seal(b"same model", &provider).unwrap();
    assert_ne!(first, second);
    assert_ne!(
        payload_section(&first).unwrap(),
        payload_section(&second).unwrap()
    );
}

#[test]
fn wrong_passphrase_header_version_and_ciphertext_are_distinct() {
    let provider = PassphraseProvider::new_for_testing("correct horse");
    let wrong = PassphraseProvider::new_for_testing("wrong battery");
    let blob = seal(b"private model", &provider).unwrap();
    assert_eq!(open(&blob, &wrong), Err(StoreError::WrongPassphrase));

    let mut bad_magic = blob.clone();
    bad_magic[0] ^= 0xff;
    assert_eq!(open(&bad_magic, &provider), Err(StoreError::CorruptHeader));

    let mut bad_version = blob.clone();
    bad_version[8] = 99;
    assert_eq!(
        open(&bad_version, &provider),
        Err(StoreError::UnsupportedVersion)
    );

    let mut bad_aad = blob.clone();
    bad_aad[10] ^= 0x01;
    assert_eq!(
        open(&bad_aad, &provider),
        Err(StoreError::CorruptCiphertext)
    );

    let mut bad_ciphertext = blob.clone();
    let payload_end = payload_section(&blob).unwrap().len();
    bad_ciphertext[payload_end - 1] ^= 0x01;
    assert_eq!(
        open(&bad_ciphertext, &provider),
        Err(StoreError::CorruptCiphertext)
    );
}

#[test]
fn passphrase_rewrap_keeps_encrypted_payload_unchanged() {
    let old = PassphraseProvider::new_for_testing("old passphrase");
    let new = PassphraseProvider::new_for_testing("new passphrase");
    let blob = seal(b"model payload", &old).unwrap();
    let immutable_before = payload_section(&blob).unwrap().to_vec();

    let rewrapped = rewrap(&blob, &old, &new).unwrap();
    assert_eq!(payload_section(&rewrapped).unwrap(), immutable_before);
    assert_eq!(open(&rewrapped, &new).unwrap(), b"model payload");
    assert_eq!(open(&rewrapped, &old), Err(StoreError::WrongPassphrase));
}

#[test]
fn production_provider_rejects_parameters_below_owasp_floor() {
    let weak = PassphraseProvider::new(
        "weak config",
        KdfConfig {
            memory_kib: 19 * 1_024 - 1,
            iterations: 2,
            parallelism: 1,
        },
    );
    assert!(matches!(seal(b"model", &weak), Err(StoreError::Kdf(_))));
}

#[test]
fn production_kdf_defaults_match_rfc9106_low_memory_profile() {
    let config = KdfConfig::default();
    assert_eq!(config.memory_kib, 64 * 1024);
    assert_eq!(config.iterations, 3);
    assert_eq!(config.parallelism, 4);
}

#[test]
fn passphrase_wrap_reuses_kdf_salt_and_keeps_unique_payload_nonces() {
    let provider = PassphraseProvider::new_for_testing("session passphrase");
    let first = seal(b"model-v1", &provider).unwrap();
    let second = seal(b"model-v2", &provider).unwrap();
    assert_eq!(wrap_salt(&first), wrap_salt(&second));
    assert_ne!(
        payload_section(&first).unwrap(),
        payload_section(&second).unwrap()
    );
    assert_eq!(open(&first, &provider).unwrap(), b"model-v1");
    assert_eq!(open(&second, &provider).unwrap(), b"model-v2");
}

fn wrap_salt(blob: &[u8]) -> [u8; 16] {
    let payload_len = payload_section(blob).unwrap().len();
    let rest = &blob[payload_len..];
    let wrapped_len = u32::from_le_bytes(rest[1..5].try_into().unwrap()) as usize;
    let wrapped = &rest[5..5 + wrapped_len];
    wrapped[20..36].try_into().expect("wrap salt")
}
