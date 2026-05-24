pub mod pair_setup;
pub mod pair_verify;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PairingError {
    #[error("accessory returned error: {0}")]
    AccessoryError(u8),
    #[error("invalid PIN format — expected NNN-NN-NNN")]
    InvalidPin,
    #[error("SRP authentication failed — wrong PIN?")]
    SrpAuthFailed,
    #[error("signature verification failed during pair setup")]
    SignatureVerificationFailed,
    #[error("unexpected pairing state: expected {expected}, got {got}")]
    UnexpectedState { expected: u8, got: u8 },
    #[error(transparent)]
    Crypto(#[from] super::crypto::CryptoError),
    #[error(transparent)]
    Tlv(#[from] super::tlv::TlvError),
    #[error(transparent)]
    Transport(#[from] super::transport::TransportError),
}
