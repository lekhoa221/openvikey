//! Opaque model persistence and encrypted-store seams.
//!
//! Store implementations persist serialized bytes and never depend on model/Beta types.

pub mod envelope;
pub mod file;
pub mod passphrase;

use zeroize::{Zeroize, ZeroizeOnDrop};

/// Data-encryption key material. Callers must not log this.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct Dek(Vec<u8>);

impl Dek {
    #[must_use]
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for Dek {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Dek([redacted])")
    }
}

/// How a DEK was wrapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapperKind {
    InMemory,
    Passphrase,
}

impl WrapperKind {
    pub(crate) const fn as_byte(self) -> u8 {
        match self {
            Self::InMemory => 1,
            Self::Passphrase => 2,
        }
    }

    pub(crate) fn from_byte(value: u8) -> Result<Self, StoreError> {
        match value {
            1 => Ok(Self::InMemory),
            2 => Ok(Self::Passphrase),
            _ => Err(StoreError::UnsupportedVersion),
        }
    }
}

/// Wrapped DEK. Opaque to the encrypted payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedKey {
    pub kind: WrapperKind,
    pub bytes: Vec<u8>,
}

/// Injected secret wrapper. Core never talks to an OS keyring.
pub trait SecretProvider {
    fn wrap(&self, dek: &Dek) -> Result<WrappedKey, StoreError>;
    fn unwrap(&self, wrapped: &WrappedKey) -> Result<Dek, StoreError>;
}

/// Persist an already-serialized model payload.
pub trait ModelStore {
    fn save(&mut self, payload: &[u8], provider: &dyn SecretProvider) -> Result<(), StoreError>;
    fn load(&self, provider: &dyn SecretProvider) -> Result<Vec<u8>, StoreError>;
}

/// Distinct envelope, authentication, recovery, and I/O failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    WrongPassphrase,
    CorruptHeader,
    CorruptCiphertext,
    UnsupportedVersion,
    Empty,
    Crypto(String),
    Kdf(String),
    Io(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongPassphrase => write!(f, "wrong passphrase"),
            Self::CorruptHeader => write!(f, "corrupt store header"),
            Self::CorruptCiphertext => write!(f, "corrupt store ciphertext"),
            Self::UnsupportedVersion => write!(f, "unsupported store version"),
            Self::Empty => write!(f, "store is empty"),
            Self::Crypto(message) => write!(f, "cryptographic failure: {message}"),
            Self::Kdf(message) => write!(f, "key derivation failure: {message}"),
            Self::Io(message) => write!(f, "store I/O failure: {message}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Deterministic test adapter. Identity wrap; never used in production lab.
#[derive(Debug, Default, Clone)]
pub struct InMemorySecretProvider;

impl InMemorySecretProvider {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl SecretProvider for InMemorySecretProvider {
    fn wrap(&self, dek: &Dek) -> Result<WrappedKey, StoreError> {
        Ok(WrappedKey {
            kind: WrapperKind::InMemory,
            bytes: dek.as_bytes().to_vec(),
        })
    }

    fn unwrap(&self, wrapped: &WrappedKey) -> Result<Dek, StoreError> {
        if wrapped.kind != WrapperKind::InMemory {
            return Err(StoreError::UnsupportedVersion);
        }
        Ok(Dek::from_bytes(wrapped.bytes.clone()))
    }
}

/// Process-local payload holder used by fast seam tests.
#[derive(Debug, Default)]
pub struct InMemoryStore {
    payload: Option<Vec<u8>>,
    wrapped_dek: Option<WrappedKey>,
}

impl InMemoryStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl ModelStore for InMemoryStore {
    fn save(&mut self, payload: &[u8], provider: &dyn SecretProvider) -> Result<(), StoreError> {
        let dek = Dek::from_bytes(vec![0; 32]);
        self.wrapped_dek = Some(provider.wrap(&dek)?);
        self.payload = Some(payload.to_vec());
        Ok(())
    }

    fn load(&self, provider: &dyn SecretProvider) -> Result<Vec<u8>, StoreError> {
        let wrapped = self.wrapped_dek.as_ref().ok_or(StoreError::Empty)?;
        let _dek = provider.unwrap(wrapped)?;
        self.payload.clone().ok_or(StoreError::CorruptCiphertext)
    }
}
