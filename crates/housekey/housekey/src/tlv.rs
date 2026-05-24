use std::collections::BTreeMap;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TlvError {
    #[error("unexpected end of TLV data")]
    UnexpectedEof,
    #[error("value exceeds maximum TLV fragment size")]
    ValueTooLarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum TlvType {
    Method = 0x00,
    Identifier = 0x01,
    Salt = 0x02,
    PublicKey = 0x03,
    Proof = 0x04,
    EncryptedData = 0x05,
    State = 0x06,
    Error = 0x07,
    RetryDelay = 0x08,
    Certificate = 0x09,
    Signature = 0x0A,
    Permissions = 0x0B,
    FragmentData = 0x0C,
    FragmentLast = 0x0D,
    SessionId = 0x0E,
    Separator = 0xFF,
}

impl TlvType {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x00 => Some(Self::Method),
            0x01 => Some(Self::Identifier),
            0x02 => Some(Self::Salt),
            0x03 => Some(Self::PublicKey),
            0x04 => Some(Self::Proof),
            0x05 => Some(Self::EncryptedData),
            0x06 => Some(Self::State),
            0x07 => Some(Self::Error),
            0x08 => Some(Self::RetryDelay),
            0x09 => Some(Self::Certificate),
            0x0A => Some(Self::Signature),
            0x0B => Some(Self::Permissions),
            0x0C => Some(Self::FragmentData),
            0x0D => Some(Self::FragmentLast),
            0x0E => Some(Self::SessionId),
            0xFF => Some(Self::Separator),
            _ => None,
        }
    }
}

pub type TlvMap = BTreeMap<TlvType, Vec<u8>>;

pub fn encode(items: &TlvMap) -> Vec<u8> {
    let mut buf = Vec::new();
    for (&tag, value) in items {
        for chunk in value.chunks(255) {
            buf.push(tag as u8);
            buf.push(chunk.len() as u8);
            buf.extend_from_slice(chunk);
        }
        if value.is_empty() {
            buf.push(tag as u8);
            buf.push(0);
        }
    }
    buf
}

pub fn decode(data: &[u8]) -> Result<TlvMap, TlvError> {
    let mut map = TlvMap::new();
    let mut pos = 0;

    while pos < data.len() {
        if pos + 2 > data.len() {
            return Err(TlvError::UnexpectedEof);
        }

        let tag = data[pos];
        let len = data[pos + 1] as usize;
        pos += 2;

        if pos + len > data.len() {
            return Err(TlvError::UnexpectedEof);
        }

        let value = &data[pos..pos + len];
        pos += len;

        if let Some(tlv_type) = TlvType::from_u8(tag) {
            map.entry(tlv_type).or_default().extend_from_slice(value);
        }
    }

    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_simple() {
        let mut input = TlvMap::new();
        input.insert(TlvType::State, vec![0x01]);
        input.insert(TlvType::Method, vec![0x00]);

        let encoded = encode(&input);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(input, decoded);
    }

    #[test]
    fn fragments_long_values() {
        let mut input = TlvMap::new();
        input.insert(TlvType::PublicKey, vec![0xAB; 300]);

        let encoded = encode(&input);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(input, decoded);
    }
}
