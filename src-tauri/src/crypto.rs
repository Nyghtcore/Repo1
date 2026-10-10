//! Cryptographic primitives for Zynky.
//!
//! * **Key derivation** – Argon2id (via the `argon2` crate) stretches the user's
//!   master password into a 256-bit symmetric key.
//! * **Authenticated encryption** – AES-256-GCM (via the `aes-gcm` crate)
//!   protects every secret with a fresh random 96-bit nonce.
//! * **Secure blob encoding** – ciphertexts are serialised into a single
//!   Base64 string (`secure_blob`) that is embedded in the YAML frontmatter of
//!   a credential's Markdown file.
//!
//! All functions return `Result<T, CryptoError>`; nothing panics on bad input.

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Key, Nonce,
};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

/// Length of the symmetric key in bytes (AES-256).
pub const KEY_LEN: usize = 32;
/// Length of the Argon2 salt in bytes.
pub const SALT_LEN: usize = 16;
/// Length of the AES-GCM nonce in bytes (96 bits, the recommended size).
pub const NONCE_LEN: usize = 12;
/// Magic prefix that identifies a Zynky secure blob (version 1).
const BLOB_MAGIC: &[u8; 4] = b"ZYK1";
/// Additional authenticated data bound to every ciphertext so that blobs
/// produced by a different application / format version cannot be replayed.
const BLOB_AAD: &[u8] = b"zynky.secure_blob.v1";

/// Errors that can be produced by the cryptography layer.
#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("invalid Argon2 parameters: {0}")]
    InvalidParams(String),
    #[error("key derivation failed: {0}")]
    KeyDerivation(String),
    #[error("encryption failed")]
    Encrypt,
    #[error("decryption failed: wrong master password or corrupted data")]
    Decrypt,
    #[error("secure blob is not valid base64: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("secure blob is malformed: {0}")]
    MalformedBlob(&'static str),
    #[error("salt must be exactly {SALT_LEN} bytes, got {0}")]
    InvalidSaltLength(usize),
    #[error("system random number generator failure")]
    Rng,
}

/// Tunable Argon2id cost parameters. Stored (unencrypted) in the vault
/// manifest so the same key can be re-derived on every device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Memory cost in KiB.
    pub m_cost_kib: u32,
    /// Number of iterations (time cost).
    pub t_cost: u32,
    /// Degree of parallelism (lanes).
    pub p_cost: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        // 64 MiB, 3 passes, 1 lane: a sensible interactive-login setting that
        // comfortably fits a desktop machine while still being GPU-hostile.
        Self {
            m_cost_kib: 64 * 1024,
            t_cost: 3,
            p_cost: 1,
        }
    }
}

impl KdfParams {
    fn to_argon2_params(self) -> Result<Params, CryptoError> {
        Params::new(self.m_cost_kib, self.t_cost, self.p_cost, Some(KEY_LEN))
            .map_err(|e| CryptoError::InvalidParams(e.to_string()))
    }
}

/// A 256-bit symmetric key that is wiped from memory when dropped.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct MasterKey(pub [u8; KEY_LEN]);

impl MasterKey {
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never leak key material through debug output.
        f.write_str("MasterKey(<redacted>)")
    }
}

/// Generate a fresh random salt for Argon2id.
pub fn generate_salt() -> Result<[u8; SALT_LEN], CryptoError> {
    let mut salt = [0u8; SALT_LEN];
    OsRng
        .try_fill_bytes(&mut salt)
        .map_err(|_| CryptoError::Rng)?;
    Ok(salt)
}

/// Stretch a master password into a 256-bit AES key with Argon2id.
///
/// The password bytes are wrapped in [`Zeroizing`] so the intermediate copy
/// is wiped as soon as derivation finishes.
pub fn derive_key(
    master_password: &str,
    salt: &[u8],
    params: KdfParams,
) -> Result<MasterKey, CryptoError> {
    if salt.len() != SALT_LEN {
        return Err(CryptoError::InvalidSaltLength(salt.len()));
    }
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params.to_argon2_params()?);
    let password = Zeroizing::new(master_password.as_bytes().to_vec());
    let mut key = [0u8; KEY_LEN];
    argon
        .hash_password_into(&password, salt, &mut key)
        .map_err(|e| CryptoError::KeyDerivation(e.to_string()))?;
    Ok(MasterKey(key))
}

/// Encrypt `plaintext` with AES-256-GCM.
///
/// Output layout: `MAGIC(4) || NONCE(12) || CIPHERTEXT+TAG`.
pub fn encrypt(key: &MasterKey, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_bytes()));
    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng
        .try_fill_bytes(&mut nonce_bytes)
        .map_err(|_| CryptoError::Rng)?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad: BLOB_AAD,
            },
        )
        .map_err(|_| CryptoError::Encrypt)?;

    let mut out = Vec::with_capacity(BLOB_MAGIC.len() + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(BLOB_MAGIC);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt a blob produced by [`encrypt`]. The returned buffer is zeroized on
/// drop so decrypted secrets never linger in freed memory.
pub fn decrypt(key: &MasterKey, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    if blob.len() < BLOB_MAGIC.len() + NONCE_LEN + 16 {
        return Err(CryptoError::MalformedBlob("blob too short"));
    }
    let (magic, rest) = blob.split_at(BLOB_MAGIC.len());
    if magic != BLOB_MAGIC {
        return Err(CryptoError::MalformedBlob("unknown blob version"));
    }
    let (nonce_bytes, ciphertext) = rest.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_bytes()));
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad: BLOB_AAD,
            },
        )
        .map_err(|_| CryptoError::Decrypt)?;
    Ok(Zeroizing::new(plaintext))
}

/// Encrypt and encode as a single Base64 string suitable for `secure_blob:`.
pub fn encrypt_to_base64(key: &MasterKey, plaintext: &[u8]) -> Result<String, CryptoError> {
    Ok(BASE64.encode(encrypt(key, plaintext)?))
}

/// Decode a `secure_blob:` Base64 string and decrypt it.
pub fn decrypt_from_base64(
    key: &MasterKey,
    blob_b64: &str,
) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
    let compact: String = blob_b64.chars().filter(|c| !c.is_whitespace()).collect();
    let blob = BASE64.decode(compact.as_bytes())?;
    decrypt(key, &blob)
}

/// Decrypt a Base64 blob and interpret the plaintext as UTF-8.
pub fn decrypt_string(key: &MasterKey, blob_b64: &str) -> Result<Zeroizing<String>, CryptoError> {
    let bytes = decrypt_from_base64(key, blob_b64)?;
    let text = String::from_utf8(bytes.to_vec())
        .map_err(|_| CryptoError::MalformedBlob("plaintext is not valid UTF-8"))?;
    Ok(Zeroizing::new(text))
}

/// Encode raw bytes as Base64 (used for salts in the manifest).
pub fn to_base64(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

/// Decode Base64 into raw bytes.
pub fn from_base64(text: &str) -> Result<Vec<u8>, CryptoError> {
    Ok(BASE64.decode(text.trim().as_bytes())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_params() -> KdfParams {
        KdfParams {
            m_cost_kib: 8 * 1024,
            t_cost: 1,
            p_cost: 1,
        }
    }

    #[test]
    fn derive_is_deterministic_for_same_inputs() {
        let salt = [7u8; SALT_LEN];
        let a = derive_key("correct horse", &salt, fast_params()).unwrap();
        let b = derive_key("correct horse", &salt, fast_params()).unwrap();
        assert_eq!(a.as_bytes(), b.as_bytes());
    }

    #[test]
    fn derive_differs_for_different_salt_or_password() {
        let a = derive_key("pw", &[1u8; SALT_LEN], fast_params()).unwrap();
        let b = derive_key("pw", &[2u8; SALT_LEN], fast_params()).unwrap();
        let c = derive_key("pw2", &[1u8; SALT_LEN], fast_params()).unwrap();
        assert_ne!(a.as_bytes(), b.as_bytes());
        assert_ne!(a.as_bytes(), c.as_bytes());
    }

    #[test]
    fn rejects_bad_salt_length() {
        let err = derive_key("pw", &[0u8; 3], fast_params()).unwrap_err();
        assert!(matches!(err, CryptoError::InvalidSaltLength(3)));
    }

    #[test]
    fn roundtrip_base64_blob() {
        let key = derive_key("pw", &[9u8; SALT_LEN], fast_params()).unwrap();
        let blob = encrypt_to_base64(&key, b"hunter2").unwrap();
        let back = decrypt_string(&key, &blob).unwrap();
        assert_eq!(back.as_str(), "hunter2");
    }

    #[test]
    fn wrong_key_fails_authentication() {
        let key = derive_key("pw", &[9u8; SALT_LEN], fast_params()).unwrap();
        let other = derive_key("pw!", &[9u8; SALT_LEN], fast_params()).unwrap();
        let blob = encrypt_to_base64(&key, b"secret").unwrap();
        assert!(matches!(
            decrypt_from_base64(&other, &blob).unwrap_err(),
            CryptoError::Decrypt
        ));
    }

    #[test]
    fn tampered_blob_fails() {
        let key = derive_key("pw", &[9u8; SALT_LEN], fast_params()).unwrap();
        let mut raw = encrypt(&key, b"secret").unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0x01;
        assert!(decrypt(&key, &raw).is_err());
    }

    #[test]
    fn malformed_blob_is_reported() {
        let key = derive_key("pw", &[9u8; SALT_LEN], fast_params()).unwrap();
        assert!(matches!(
            decrypt(&key, b"short").unwrap_err(),
            CryptoError::MalformedBlob(_)
        ));
        assert!(matches!(
            decrypt_from_base64(&key, "not base64!!").unwrap_err(),
            CryptoError::Base64(_)
        ));
    }
}
