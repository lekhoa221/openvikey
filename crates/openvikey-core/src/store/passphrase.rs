//! Argon2id passphrase wrapper for independently authenticated DEK slots.

use crate::store::{Dek, SecretProvider, StoreError, WrappedKey, WrapperKind};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const SLOT_MAGIC: &[u8; 8] = b"OVKPASS1";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const DEK_LEN: usize = 32;
const TAG_LEN: usize = 16;
const PREFIX_LEN: usize = SLOT_MAGIC.len() + 12 + SALT_LEN + NONCE_LEN;
const SLOT_LEN: usize = PREFIX_LEN + DEK_LEN + TAG_LEN;

/// Persisted Argon2id work factors. Memory is expressed in KiB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfConfig {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

impl KdfConfig {
    /// Fast explicit profile for unit tests only.
    #[must_use]
    pub const fn testing() -> Self {
        Self {
            memory_kib: 1_024,
            iterations: 1,
            parallelism: 1,
        }
    }

    fn validate(self, allow_weak_for_tests: bool) -> Result<(), StoreError> {
        let invalid_shape = self.memory_kib == 0
            || self.memory_kib > 256 * 1_024
            || self.iterations == 0
            || self.iterations > 10
            || self.parallelism == 0
            || self.parallelism > 16;
        let below_owasp_floor =
            self.memory_kib < 19 * 1_024 || self.iterations < 2 || self.parallelism < 1;
        if invalid_shape || (!allow_weak_for_tests && below_owasp_floor) {
            return Err(StoreError::Kdf(
                "parameters outside safe bounds".to_string(),
            ));
        }
        Ok(())
    }
}

impl Default for KdfConfig {
    fn default() -> Self {
        Self {
            memory_kib: 64 * 1_024,
            iterations: 3,
            parallelism: 4,
        }
    }
}

/// Passphrase-backed DEK wrapper. Debug output never exposes the passphrase.
#[derive(Clone)]
pub struct PassphraseProvider {
    passphrase: Zeroizing<Vec<u8>>,
    config: KdfConfig,
    allow_weak_for_tests: bool,
}

impl PassphraseProvider {
    #[must_use]
    pub fn new(passphrase: impl AsRef<str>, config: KdfConfig) -> Self {
        Self {
            passphrase: Zeroizing::new(passphrase.as_ref().as_bytes().to_vec()),
            config,
            allow_weak_for_tests: false,
        }
    }

    /// Explicit weak provider for integration tests; never use for persisted user data.
    #[must_use]
    pub fn new_for_testing(passphrase: impl AsRef<str>) -> Self {
        Self {
            passphrase: Zeroizing::new(passphrase.as_ref().as_bytes().to_vec()),
            config: KdfConfig::testing(),
            allow_weak_for_tests: true,
        }
    }

    #[must_use]
    pub const fn config(&self) -> KdfConfig {
        self.config
    }
}

impl std::fmt::Debug for PassphraseProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PassphraseProvider")
            .field("passphrase", &"[redacted]")
            .field("config", &self.config)
            .field("allow_weak_for_tests", &self.allow_weak_for_tests)
            .finish()
    }
}

impl SecretProvider for PassphraseProvider {
    fn wrap(&self, dek: &Dek) -> Result<WrappedKey, StoreError> {
        if dek.as_bytes().len() != DEK_LEN {
            return Err(StoreError::CorruptHeader);
        }
        self.config.validate(self.allow_weak_for_tests)?;
        let mut salt = [0_u8; SALT_LEN];
        let mut nonce = [0_u8; NONCE_LEN];
        getrandom::getrandom(&mut salt)
            .map_err(|error| StoreError::Crypto(format!("salt randomness: {error}")))?;
        getrandom::getrandom(&mut nonce)
            .map_err(|error| StoreError::Crypto(format!("slot nonce randomness: {error}")))?;
        let prefix = make_prefix(self.config, &salt, &nonce);
        let key = derive_key(&self.passphrase, &salt, self.config)?;
        let cipher = XChaCha20Poly1305::new(Key::from_slice(&key[..]));
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: dek.as_bytes(),
                    aad: &prefix,
                },
            )
            .map_err(|_| StoreError::Crypto("DEK wrapping failed".to_string()))?;
        let mut bytes = prefix;
        bytes.extend_from_slice(&ciphertext);
        Ok(WrappedKey {
            kind: WrapperKind::Passphrase,
            bytes,
        })
    }

    fn unwrap(&self, wrapped: &WrappedKey) -> Result<Dek, StoreError> {
        if wrapped.kind != WrapperKind::Passphrase {
            return Err(StoreError::UnsupportedVersion);
        }
        if wrapped.bytes.len() != SLOT_LEN || &wrapped.bytes[..SLOT_MAGIC.len()] != SLOT_MAGIC {
            return Err(StoreError::CorruptHeader);
        }
        let config = KdfConfig {
            memory_kib: read_u32(&wrapped.bytes, 8)?,
            iterations: read_u32(&wrapped.bytes, 12)?,
            parallelism: read_u32(&wrapped.bytes, 16)?,
        };
        config.validate(self.allow_weak_for_tests)?;
        let salt_start = SLOT_MAGIC.len() + 12;
        let salt_end = salt_start + SALT_LEN;
        let nonce_end = salt_end + NONCE_LEN;
        let salt: [u8; SALT_LEN] = wrapped.bytes[salt_start..salt_end]
            .try_into()
            .map_err(|_| StoreError::CorruptHeader)?;
        let nonce = &wrapped.bytes[salt_end..nonce_end];
        let key = derive_key(&self.passphrase, &salt, config)?;
        let cipher = XChaCha20Poly1305::new(Key::from_slice(&key[..]));
        let dek = cipher
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: &wrapped.bytes[nonce_end..],
                    aad: &wrapped.bytes[..nonce_end],
                },
            )
            .map_err(|_| StoreError::WrongPassphrase)?;
        if dek.len() != DEK_LEN {
            return Err(StoreError::CorruptHeader);
        }
        Ok(Dek::from_bytes(dek))
    }
}

fn make_prefix(config: KdfConfig, salt: &[u8; SALT_LEN], nonce: &[u8; NONCE_LEN]) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(PREFIX_LEN);
    prefix.extend_from_slice(SLOT_MAGIC);
    prefix.extend_from_slice(&config.memory_kib.to_le_bytes());
    prefix.extend_from_slice(&config.iterations.to_le_bytes());
    prefix.extend_from_slice(&config.parallelism.to_le_bytes());
    prefix.extend_from_slice(salt);
    prefix.extend_from_slice(nonce);
    prefix
}

fn derive_key(
    passphrase: &[u8],
    salt: &[u8; SALT_LEN],
    config: KdfConfig,
) -> Result<Zeroizing<[u8; 32]>, StoreError> {
    let params = Params::new(
        config.memory_kib,
        config.iterations,
        config.parallelism,
        Some(32),
    )
    .map_err(|error| StoreError::Kdf(error.to_string()))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut output = Zeroizing::new([0_u8; 32]);
    argon
        .hash_password_into(passphrase, salt, output.as_mut())
        .map_err(|error| StoreError::Kdf(error.to_string()))?;
    Ok(output)
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, StoreError> {
    let end = offset.checked_add(4).ok_or(StoreError::CorruptHeader)?;
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..end)
            .ok_or(StoreError::CorruptHeader)?
            .try_into()
            .map_err(|_| StoreError::CorruptHeader)?,
    ))
}
