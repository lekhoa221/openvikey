//! Versioned XChaCha20-Poly1305 envelope with independently rewrappable DEK slots.

use crate::store::{Dek, SecretProvider, StoreError, WrappedKey, WrapperKind};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};

const MAGIC: &[u8; 8] = b"OVKMODEL";
const VERSION: u16 = 1;
const NONCE_LEN: usize = 24;
const HEADER_LEN: usize = MAGIC.len() + 2 + NONCE_LEN + 8;
const TAG_LEN: usize = 16;

struct ParsedEnvelope<'a> {
    payload_section: &'a [u8],
    header: &'a [u8],
    nonce: &'a [u8],
    ciphertext: &'a [u8],
    wrapped: WrappedKey,
}

/// Encrypts a serialized model payload with a random DEK and payload nonce.
pub fn seal(payload: &[u8], provider: &dyn SecretProvider) -> Result<Vec<u8>, StoreError> {
    let mut dek_bytes = vec![0_u8; 32];
    getrandom::getrandom(&mut dek_bytes)
        .map_err(|error| StoreError::Crypto(format!("DEK randomness: {error}")))?;
    let dek = Dek::from_bytes(dek_bytes);
    let mut nonce = [0_u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce)
        .map_err(|error| StoreError::Crypto(format!("nonce randomness: {error}")))?;

    let cipher_len = payload
        .len()
        .checked_add(TAG_LEN)
        .ok_or(StoreError::CorruptHeader)?;
    let header = make_header(&nonce, cipher_len)?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(dek.as_bytes()));
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: payload,
                aad: &header,
            },
        )
        .map_err(|_| StoreError::Crypto("payload encryption failed".to_string()))?;
    let wrapped = provider.wrap(&dek)?;

    encode(&header, &ciphertext, &wrapped)
}

/// Authenticates and decrypts an envelope.
pub fn open(blob: &[u8], provider: &dyn SecretProvider) -> Result<Vec<u8>, StoreError> {
    let parsed = parse(blob)?;
    let dek = provider.unwrap(&parsed.wrapped)?;
    if dek.as_bytes().len() != 32 {
        return Err(StoreError::CorruptHeader);
    }
    let cipher = XChaCha20Poly1305::new(Key::from_slice(dek.as_bytes()));
    cipher
        .decrypt(
            XNonce::from_slice(parsed.nonce),
            Payload {
                msg: parsed.ciphertext,
                aad: parsed.header,
            },
        )
        .map_err(|_| StoreError::CorruptCiphertext)
}

/// Replaces only the authenticated wrapped-key slot; payload bytes remain identical.
pub fn rewrap(
    blob: &[u8],
    old_provider: &dyn SecretProvider,
    new_provider: &dyn SecretProvider,
) -> Result<Vec<u8>, StoreError> {
    let parsed = parse(blob)?;
    let dek = old_provider.unwrap(&parsed.wrapped)?;
    let wrapped = new_provider.wrap(&dek)?;
    encode(parsed.header, parsed.ciphertext, &wrapped)
}

/// Returns immutable header + encrypted payload for rewrap/recovery verification.
pub fn payload_section(blob: &[u8]) -> Result<&[u8], StoreError> {
    Ok(parse(blob)?.payload_section)
}

fn make_header(nonce: &[u8; NONCE_LEN], cipher_len: usize) -> Result<Vec<u8>, StoreError> {
    let cipher_len = u64::try_from(cipher_len).map_err(|_| StoreError::CorruptHeader)?;
    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&VERSION.to_le_bytes());
    header.extend_from_slice(nonce);
    header.extend_from_slice(&cipher_len.to_le_bytes());
    Ok(header)
}

fn encode(header: &[u8], ciphertext: &[u8], wrapped: &WrappedKey) -> Result<Vec<u8>, StoreError> {
    if header.len() != HEADER_LEN {
        return Err(StoreError::CorruptHeader);
    }
    let wrapped_len = u32::try_from(wrapped.bytes.len()).map_err(|_| StoreError::CorruptHeader)?;
    let mut blob = Vec::with_capacity(
        header.len() + ciphertext.len() + 1 + size_of::<u32>() + wrapped.bytes.len(),
    );
    blob.extend_from_slice(header);
    blob.extend_from_slice(ciphertext);
    blob.push(wrapped.kind.as_byte());
    blob.extend_from_slice(&wrapped_len.to_le_bytes());
    blob.extend_from_slice(&wrapped.bytes);
    Ok(blob)
}

fn parse(blob: &[u8]) -> Result<ParsedEnvelope<'_>, StoreError> {
    if blob.len() < HEADER_LEN + TAG_LEN + 1 + size_of::<u32>() {
        return Err(StoreError::CorruptHeader);
    }
    if &blob[..MAGIC.len()] != MAGIC {
        return Err(StoreError::CorruptHeader);
    }
    let version = u16::from_le_bytes(
        blob[MAGIC.len()..MAGIC.len() + 2]
            .try_into()
            .map_err(|_| StoreError::CorruptHeader)?,
    );
    if version != VERSION {
        return Err(StoreError::UnsupportedVersion);
    }
    let nonce_start = MAGIC.len() + 2;
    let nonce_end = nonce_start + NONCE_LEN;
    let len_end = nonce_end + 8;
    let cipher_len_u64 = u64::from_le_bytes(
        blob[nonce_end..len_end]
            .try_into()
            .map_err(|_| StoreError::CorruptHeader)?,
    );
    let cipher_len = usize::try_from(cipher_len_u64).map_err(|_| StoreError::CorruptHeader)?;
    if cipher_len < TAG_LEN {
        return Err(StoreError::CorruptHeader);
    }
    let cipher_end = HEADER_LEN
        .checked_add(cipher_len)
        .ok_or(StoreError::CorruptHeader)?;
    let key_header_end = cipher_end
        .checked_add(1 + size_of::<u32>())
        .ok_or(StoreError::CorruptHeader)?;
    if key_header_end > blob.len() {
        return Err(StoreError::CorruptHeader);
    }
    let kind = WrapperKind::from_byte(blob[cipher_end])?;
    let wrapped_len = u32::from_le_bytes(
        blob[cipher_end + 1..key_header_end]
            .try_into()
            .map_err(|_| StoreError::CorruptHeader)?,
    );
    let wrapped_len = usize::try_from(wrapped_len).map_err(|_| StoreError::CorruptHeader)?;
    if key_header_end.checked_add(wrapped_len) != Some(blob.len()) {
        return Err(StoreError::CorruptHeader);
    }

    Ok(ParsedEnvelope {
        payload_section: &blob[..cipher_end],
        header: &blob[..HEADER_LEN],
        nonce: &blob[nonce_start..nonce_end],
        ciphertext: &blob[HEADER_LEN..cipher_end],
        wrapped: WrappedKey {
            kind,
            bytes: blob[key_header_end..].to_vec(),
        },
    })
}
