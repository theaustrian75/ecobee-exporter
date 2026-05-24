use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};

use super::CryptoError;

const NONCE_SIZE: usize = 12;

pub struct EncryptedSession {
    send_cipher: ChaCha20Poly1305,
    recv_cipher: ChaCha20Poly1305,
    send_counter: u64,
    recv_counter: u64,
}

impl EncryptedSession {
    pub fn new(send_key: &[u8; 32], recv_key: &[u8; 32]) -> Self {
        Self {
            send_cipher: ChaCha20Poly1305::new(send_key.into()),
            recv_cipher: ChaCha20Poly1305::new(recv_key.into()),
            send_counter: 0,
            recv_counter: 0,
        }
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let nonce = self.make_nonce(self.send_counter);
        let ciphertext = self
            .send_cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| CryptoError::EncryptionFailed)?;
        self.send_counter += 1;
        Ok(ciphertext)
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let nonce = self.make_nonce(self.recv_counter);
        let plaintext = self
            .recv_cipher
            .decrypt(&nonce, ciphertext)
            .map_err(|_| CryptoError::DecryptionFailed)?;
        self.recv_counter += 1;
        Ok(plaintext)
    }

    fn make_nonce(&self, counter: u64) -> Nonce {
        let mut nonce = [0u8; NONCE_SIZE];
        nonce[4..].copy_from_slice(&counter.to_le_bytes());
        Nonce::from(nonce)
    }
}
