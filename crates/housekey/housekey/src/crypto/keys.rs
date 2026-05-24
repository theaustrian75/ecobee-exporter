use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use x25519_dalek::{EphemeralSecret, PublicKey as X25519PublicKey};

pub struct LongTermKeyPair {
    pub signing_key: SigningKey,
    pub verifying_key: VerifyingKey,
}

impl LongTermKeyPair {
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }
}

pub struct EphemeralKeyPair {
    pub secret: EphemeralSecret,
    pub public: X25519PublicKey,
}

impl EphemeralKeyPair {
    pub fn generate() -> Self {
        let secret = EphemeralSecret::random_from_rng(OsRng);
        let public = X25519PublicKey::from(&secret);
        Self { secret, public }
    }
}
