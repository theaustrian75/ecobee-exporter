use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;

use super::PairingError;
use crate::crypto::srp::{SrpClient, SrpProof};
use crate::crypto::{hap_decrypt, hap_encrypt, hkdf_derive};
use crate::tlv::{self, TlvMap, TlvType};

const METHOD_PAIR_SETUP_WITH_AUTH: u8 = 0x01;

pub struct PairSetupResult {
    pub accessory_pairing_id: Vec<u8>,
    pub accessory_ltpk: [u8; 32],
    pub controller_pairing_id: Vec<u8>,
    pub controller_ltsk: [u8; 32],
    pub controller_ltpk: [u8; 32],
}

pub struct PairSetup {
    pin: String,
    pairing_id: Vec<u8>,
}

impl PairSetup {
    pub fn new(pin: &str, pairing_id: &[u8]) -> Result<Self, PairingError> {
        if !is_valid_pin(pin) {
            return Err(PairingError::InvalidPin);
        }
        Ok(Self {
            pin: strip_dashes(pin),
            pairing_id: pairing_id.to_vec(),
        })
    }

    pub fn build_m1(&self) -> Vec<u8> {
        let mut tlvs = TlvMap::new();
        tlvs.insert(TlvType::State, vec![0x01]);
        tlvs.insert(TlvType::Method, vec![METHOD_PAIR_SETUP_WITH_AUTH]);
        tlv::encode(&tlvs)
    }

    pub fn process_m2(&self, data: &[u8]) -> Result<PairSetupM3, PairingError> {
        let tlvs = tlv::decode(data)?;

        check_state(&tlvs, 0x02)?;
        check_error(&tlvs)?;

        let salt = tlvs
            .get(&TlvType::Salt)
            .ok_or(PairingError::UnexpectedState {
                expected: 0x02,
                got: 0x00,
            })?;

        let server_pk = tlvs
            .get(&TlvType::PublicKey)
            .ok_or(PairingError::UnexpectedState {
                expected: 0x02,
                got: 0x00,
            })?;

        let srp_client = SrpClient::new(b"Pair-Setup", self.pin.as_bytes());
        let proof = srp_client
            .process_challenge(salt, server_pk)
            .map_err(|_| PairingError::SrpAuthFailed)?;

        Ok(PairSetupM3 {
            proof,
            pairing_id: self.pairing_id.clone(),
        })
    }
}

pub struct PairSetupM3 {
    proof: SrpProof,
    pairing_id: Vec<u8>,
}

impl PairSetupM3 {
    pub fn build_m3(&self) -> Vec<u8> {
        let mut tlvs = TlvMap::new();
        tlvs.insert(TlvType::State, vec![0x03]);
        tlvs.insert(TlvType::PublicKey, self.proof.a_public_bytes.clone());
        tlvs.insert(TlvType::Proof, self.proof.m1_proof.clone());
        tlv::encode(&tlvs)
    }

    pub fn process_m4(&self, data: &[u8]) -> Result<PairSetupM5, PairingError> {
        let tlvs = tlv::decode(data)?;

        check_state(&tlvs, 0x04)?;
        check_error(&tlvs)?;

        let server_proof = tlvs
            .get(&TlvType::Proof)
            .ok_or(PairingError::SrpAuthFailed)?;

        if !self.proof.verify_server_proof(server_proof) {
            return Err(PairingError::SrpAuthFailed);
        }

        Ok(PairSetupM5 {
            session_key: self.proof.session_key.clone(),
            pairing_id: self.pairing_id.clone(),
        })
    }
}

pub struct PairSetupM5 {
    session_key: Vec<u8>,
    pairing_id: Vec<u8>,
}

impl PairSetupM5 {
    pub fn build_m5(&self) -> Result<(Vec<u8>, SigningKey), PairingError> {
        let controller_sign_key = hkdf_derive(
            &self.session_key,
            b"Pair-Setup-Controller-Sign-Salt",
            b"Pair-Setup-Controller-Sign-Info",
        )
        .map_err(PairingError::Crypto)?;

        let encrypt_key = hkdf_derive(
            &self.session_key,
            b"Pair-Setup-Encrypt-Salt",
            b"Pair-Setup-Encrypt-Info",
        )
        .map_err(PairingError::Crypto)?;

        let ltsk = SigningKey::generate(&mut OsRng);
        let ltpk = ltsk.verifying_key();

        // device_info = controller_sign_key || pairing_id || ltpk
        let mut device_info = Vec::new();
        device_info.extend_from_slice(&controller_sign_key);
        device_info.extend_from_slice(&self.pairing_id);
        device_info.extend_from_slice(ltpk.as_bytes());

        let signature = ltsk.sign(&device_info);

        let mut sub_tlvs = TlvMap::new();
        sub_tlvs.insert(TlvType::Identifier, self.pairing_id.clone());
        sub_tlvs.insert(TlvType::PublicKey, ltpk.as_bytes().to_vec());
        sub_tlvs.insert(TlvType::Signature, signature.to_bytes().to_vec());
        let sub_tlv_bytes = tlv::encode(&sub_tlvs);

        let encrypted =
            hap_encrypt(&encrypt_key, b"PS-Msg05", &sub_tlv_bytes).map_err(PairingError::Crypto)?;

        let mut tlvs = TlvMap::new();
        tlvs.insert(TlvType::State, vec![0x05]);
        tlvs.insert(TlvType::EncryptedData, encrypted);

        Ok((tlv::encode(&tlvs), ltsk))
    }

    pub fn process_m6(
        &self,
        data: &[u8],
        controller_ltsk: &SigningKey,
    ) -> Result<PairSetupResult, PairingError> {
        let tlvs = tlv::decode(data)?;

        check_state(&tlvs, 0x06)?;
        check_error(&tlvs)?;

        let encrypted_data =
            tlvs.get(&TlvType::EncryptedData)
                .ok_or(PairingError::UnexpectedState {
                    expected: 0x06,
                    got: 0x00,
                })?;

        let encrypt_key = hkdf_derive(
            &self.session_key,
            b"Pair-Setup-Encrypt-Salt",
            b"Pair-Setup-Encrypt-Info",
        )
        .map_err(PairingError::Crypto)?;

        let decrypted =
            hap_decrypt(&encrypt_key, b"PS-Msg06", encrypted_data).map_err(PairingError::Crypto)?;

        let sub_tlvs = tlv::decode(&decrypted)?;

        let accessory_ltpk_bytes = sub_tlvs
            .get(&TlvType::PublicKey)
            .ok_or(PairingError::SignatureVerificationFailed)?;
        let accessory_pairing_id = sub_tlvs
            .get(&TlvType::Identifier)
            .ok_or(PairingError::SignatureVerificationFailed)?;
        let accessory_signature = sub_tlvs
            .get(&TlvType::Signature)
            .ok_or(PairingError::SignatureVerificationFailed)?;

        let accessory_ltpk: [u8; 32] = accessory_ltpk_bytes
            .as_slice()
            .try_into()
            .map_err(|_| PairingError::SignatureVerificationFailed)?;

        let accessory_sign_key = hkdf_derive(
            &self.session_key,
            b"Pair-Setup-Accessory-Sign-Salt",
            b"Pair-Setup-Accessory-Sign-Info",
        )
        .map_err(PairingError::Crypto)?;

        // accessory_info = accessory_sign_key || pairing_id || ltpk
        let mut accessory_info = Vec::new();
        accessory_info.extend_from_slice(&accessory_sign_key);
        accessory_info.extend_from_slice(accessory_pairing_id);
        accessory_info.extend_from_slice(&accessory_ltpk);

        let verifying_key = VerifyingKey::from_bytes(&accessory_ltpk)
            .map_err(|_| PairingError::SignatureVerificationFailed)?;
        let signature = ed25519_dalek::Signature::from_slice(accessory_signature)
            .map_err(|_| PairingError::SignatureVerificationFailed)?;

        verifying_key
            .verify_strict(&accessory_info, &signature)
            .map_err(|_| PairingError::SignatureVerificationFailed)?;

        let controller_ltpk = controller_ltsk.verifying_key();

        Ok(PairSetupResult {
            accessory_pairing_id: accessory_pairing_id.clone(),
            accessory_ltpk,
            controller_pairing_id: self.pairing_id.clone(),
            controller_ltsk: controller_ltsk.to_bytes(),
            controller_ltpk: controller_ltpk.to_bytes(),
        })
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

fn is_valid_pin(pin: &str) -> bool {
    let digits: String = pin.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.len() == 8
}

fn strip_dashes(pin: &str) -> String {
    pin.chars().filter(|c| c.is_ascii_digit()).collect()
}
