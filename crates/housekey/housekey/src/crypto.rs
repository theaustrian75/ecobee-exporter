pub mod keys;
pub mod session;
pub mod srp;

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use hkdf::Hkdf;
use sha2::Sha512;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("decryption failed — invalid auth tag or corrupted data")]
    DecryptionFailed,
    #[error("encryption failed")]
    EncryptionFailed,
    #[error("key derivation failed")]
    KeyDerivationFailed,
    #[error("invalid key length: expected {expected}, got {got}")]
    InvalidKeyLength { expected: usize, got: usize },
    #[error("signature verification failed")]
    SignatureVerificationFailed,
}

pub fn hkdf_derive(ikm: &[u8], salt: &[u8], info: &[u8]) -> Result<[u8; 32], CryptoError> {
    let hkdf = Hkdf::<Sha512>::new(Some(salt), ikm);
    let mut okm = [0u8; 32];
    hkdf.expand(info, &mut okm)
        .map_err(|_| CryptoError::KeyDerivationFailed)?;
    Ok(okm)
}

pub fn hap_nonce(label: &[u8; 8]) -> Nonce {
    let mut nonce = [0u8; 12];
    nonce[4..].copy_from_slice(label);
    Nonce::from(nonce)
}

pub fn hap_encrypt(
    key: &[u8; 32],
    nonce_label: &[u8; 8],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = ChaCha20Poly1305::new(key.into());
    cipher
        .encrypt(&hap_nonce(nonce_label), plaintext)
        .map_err(|_| CryptoError::EncryptionFailed)
}

pub fn hap_decrypt(
    key: &[u8; 32],
    nonce_label: &[u8; 8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = ChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(&hap_nonce(nonce_label), ciphertext)
        .map_err(|_| CryptoError::DecryptionFailed)
}
