//! Opaque model persistence seam.
//!
//! Wave 0 locks `SecretProvider` and a byte-oriented `ModelStore`.
//! Envelope encryption (XChaCha20-Poly1305, Argon2id, rewrap) is Milestone 8.
//! This module must not import Beta / `EmptyModel` types.

/// Data-encryption key material. Callers must not log this.
#[derive(Clone, PartialEq, Eq)]
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

/// How a DEK was wrapped. M8 adds a passphrase slot; Wave 0 only has in-memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapperKind {
    InMemory,
    Passphrase,
}

/// Wrapped DEK. Opaque to the store payload.
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

/// Distinct recovery failures. M8 fills these from the envelope parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    WrongPassphrase,
    CorruptHeader,
    CorruptCiphertext,
    UnsupportedVersion,
    Empty,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongPassphrase => write!(f, "wrong passphrase"),
            Self::CorruptHeader => write!(f, "corrupt store header"),
            Self::CorruptCiphertext => write!(f, "corrupt store ciphertext"),
            Self::UnsupportedVersion => write!(f, "unsupported store version"),
            Self::Empty => write!(f, "store is empty"),
        }
    }
}

impl std::error::Error for StoreError {}

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

/// Process-local payload holder. Does not encrypt; M8 replaces this path.
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
