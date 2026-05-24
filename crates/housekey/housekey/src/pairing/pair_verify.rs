//! HAP pair-verify handshake (establishes encrypted session keys).

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use x25519_dalek::{EphemeralSecret, PublicKey as X25519PublicKey};

use super::PairingError;
use crate::crypto::{hap_decrypt, hap_encrypt, hkdf_derive};
use crate::tlv::{self, TlvMap, TlvType};

pub struct PairVerifyKeys {
    pub write_key: [u8; 32],
    pub read_key: [u8; 32],
}

pub struct PairVerify {
    accessory_pairing_id: String,
    accessory_ltpk: [u8; 32],
    controller_pairing_id: String,
    controller_ltsk: SigningKey,
}

impl PairVerify {
    pub fn new(
        accessory_pairing_id: &str,
        accessory_ltpk: [u8; 32],
        controller_pairing_id: &str,
        controller_ltsk_bytes: &[u8; 32],
    ) -> Self {
        Self {
            accessory_pairing_id: accessory_pairing_id.to_string(),
            accessory_ltpk,
            controller_pairing_id: controller_pairing_id.to_string(),
            controller_ltsk: SigningKey::from_bytes(controller_ltsk_bytes),
        }
    }

    pub fn build_m1(&self) -> (Vec<u8>, EphemeralSecret) {
        let secret = EphemeralSecret::random_from_rng(rand::rngs::OsRng);
        let public = X25519PublicKey::from(&secret);
        let mut tlvs = TlvMap::new();
        tlvs.insert(TlvType::State, vec![0x01]);
        tlvs.insert(TlvType::PublicKey, public.as_bytes().to_vec());
        (tlv::encode(&tlvs), secret)
    }

    pub fn process_m2(
        &self,
        data: &[u8],
        our_secret: EphemeralSecret,
        our_public: &[u8; 32],
    ) -> Result<(Vec<u8>, PairVerifyKeys), PairingError> {
        let tlvs = tlv::decode(data)?;
        check_state(&tlvs, 0x02)?;
        check_error(&tlvs)?;

        let accessory_session_pk =
            tlvs.get(&TlvType::PublicKey)
                .ok_or(PairingError::UnexpectedState {
                    expected: 0x02,
                    got: 0,
                })?;
        let encrypted = tlvs
            .get(&TlvType::EncryptedData)
            .ok_or(PairingError::UnexpectedState {
                expected: 0x02,
                got: 0,
            })?;

        let accessory_pk: [u8; 32] = accessory_session_pk.as_slice().try_into().map_err(|_| {
            PairingError::Crypto(crate::crypto::CryptoError::InvalidKeyLength {
                expected: 32,
                got: accessory_session_pk.len(),
            })
        })?;

        let shared = our_secret.diffie_hellman(&X25519PublicKey::from(accessory_pk));
        let session_key = hkdf_derive(
            shared.as_bytes(),
            b"Pair-Verify-Encrypt-Salt",
            b"Pair-Verify-Encrypt-Info",
        )
        .map_err(PairingError::Crypto)?;

        let decrypted =
            hap_decrypt(&session_key, b"PV-Msg02", encrypted).map_err(PairingError::Crypto)?;
        let sub = tlv::decode(&decrypted)?;

        let accessory_id = sub
            .get(&TlvType::Identifier)
            .ok_or(PairingError::SignatureVerificationFailed)?;
        if !ids_match(&self.accessory_pairing_id, accessory_id) {
            return Err(PairingError::SignatureVerificationFailed);
        }

        let accessory_sig = sub
            .get(&TlvType::Signature)
            .ok_or(PairingError::SignatureVerificationFailed)?;

        let mut accessory_info = Vec::new();
        accessory_info.extend_from_slice(&accessory_pk);
        accessory_info.extend_from_slice(accessory_id);
        accessory_info.extend_from_slice(our_public);

        let verifying = VerifyingKey::from_bytes(&self.accessory_ltpk)
            .map_err(|_| PairingError::SignatureVerificationFailed)?;
        let signature = ed25519_dalek::Signature::from_slice(accessory_sig)
            .map_err(|_| PairingError::SignatureVerificationFailed)?;
        verifying
            .verify_strict(&accessory_info, &signature)
            .map_err(|_| PairingError::SignatureVerificationFailed)?;

        let mut ios_info = Vec::new();
        ios_info.extend_from_slice(our_public);
        ios_info.extend_from_slice(self.controller_pairing_id.as_bytes());
        ios_info.extend_from_slice(&accessory_pk);

        let ios_sig = self.controller_ltsk.sign(&ios_info);

        let mut sub_tlvs = TlvMap::new();
        sub_tlvs.insert(
            TlvType::Identifier,
            self.controller_pairing_id.as_bytes().to_vec(),
        );
        sub_tlvs.insert(TlvType::Signature, ios_sig.to_bytes().to_vec());
        let encrypted_m3 = hap_encrypt(&session_key, b"PV-Msg03", &tlv::encode(&sub_tlvs))
            .map_err(PairingError::Crypto)?;

        let mut tlvs = TlvMap::new();
        tlvs.insert(TlvType::State, vec![0x03]);
        tlvs.insert(TlvType::EncryptedData, encrypted_m3);

        let write_key = hkdf_derive(
            shared.as_bytes(),
            b"Control-Salt",
            b"Control-Write-Encryption-Key",
        )
        .map_err(PairingError::Crypto)?;
        let read_key = hkdf_derive(
            shared.as_bytes(),
            b"Control-Salt",
            b"Control-Read-Encryption-Key",
        )
        .map_err(PairingError::Crypto)?;

        Ok((
            tlv::encode(&tlvs),
            PairVerifyKeys {
                write_key,
                read_key,
            },
        ))
    }

    pub fn verify_m4(data: &[u8]) -> Result<(), PairingError> {
        let tlvs = tlv::decode(data)?;
        check_state(&tlvs, 0x04)?;
        check_error(&tlvs)?;
        Ok(())
    }
}

fn check_state(tlvs: &TlvMap, expected: u8) -> Result<(), PairingError> {
    let state = tlvs
        .get(&TlvType::State)
        .and_then(|v| v.first().copied())
        .unwrap_or(0);
    if state != expected {
        return Err(PairingError::UnexpectedState {
            expected,
            got: state,
        });
    }
    Ok(())
}

fn check_error(tlvs: &TlvMap) -> Result<(), PairingError> {
    if let Some(err) = tlvs.get(&TlvType::Error)
        && let Some(&code) = err.first()
    {
        return Err(PairingError::AccessoryError(code));
    }
    Ok(())
}

fn ids_match(stored: &str, received: &[u8]) -> bool {
    if received == stored.as_bytes() {
        return true;
    }
    let hex = crate::discovery::normalize_accessory_id(stored);
    if hex.len() == 12 {
        if let Ok(decoded) = hex::decode(&hex) {
            return decoded == received;
        }
    }
    false
}

mod hex {
    pub fn decode(s: &str) -> Result<Vec<u8>, ()> {
        if !s.len().is_multiple_of(2) {
            return Err(());
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_accepts_mac_string_or_six_bytes() {
        let raw = [0x18, 0xe2, 0x7f, 0xfe, 0x8d, 0x24];
        assert!(ids_match("18:E2:7F:FE:8D:24", &raw));
        assert!(ids_match("18E27FFE8D24", &raw));
        assert!(!ids_match("AA:BB:CC:DD:EE:FF", &raw));
    }
}
