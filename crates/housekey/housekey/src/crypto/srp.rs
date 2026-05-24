use num_bigint::BigUint;
use rand::RngCore;
use sha2::{Digest, Sha512};

use super::CryptoError;

const N_HEX: &str = "\
FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E08\
8A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B\
302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9\
A637ED6B0BFF5CB6F406B7EDEE386BFB5A899FA5AE9F24117C4B1FE6\
49286651ECE45B3DC2007CB8A163BF0598DA48361C55D39A69163FA8F\
D24CF5F83655D23DCA3AD961C62F356208552BB9ED529077096966D67\
0C354E4ABC9804F1746C08CA18217C32905E462E36CE3BE39E772C180\
E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF6955817183995\
497CEA956AE515D2261898FA051015728E5A8AAAC42DAD33170D04507\
A33A85521ABDF1CBA64ECFB850458DBEF0A8AEA71575D060C7DB3970\
F85A6E1E4C7ABF5AE8CDB0933D71E8C94E04A25619DCEE3D2261AD2E\
E6BF12FFA06D98A0864D87602733EC86A64521F2B18177B200CBBE117\
577A615D6C770988C0BAD946E208E24FA074E5AB3143DB5BFCE0FD108\
E4B82D120A93AD2CAFFFFFFFFFFFFFFFF";

const G: u64 = 5;
const KEY_LENGTH: usize = 384;

pub struct SrpClient {
    n: BigUint,
    g: BigUint,
    a_secret: BigUint,
    a_public: BigUint,
    username: Vec<u8>,
    password: Vec<u8>,
}

pub struct SrpProof {
    pub a_public_bytes: Vec<u8>,
    pub m1_proof: Vec<u8>,
    pub session_key: Vec<u8>,
    m2_expected: Vec<u8>,
}

impl SrpClient {
    pub fn new(username: &[u8], password: &[u8]) -> Self {
        let n = BigUint::parse_bytes(N_HEX.as_bytes(), 16).unwrap();
        let g = BigUint::from(G);

        let mut a_bytes = [0u8; 64];
        rand::thread_rng().fill_bytes(&mut a_bytes);
        let a_secret = BigUint::from_bytes_be(&a_bytes);
        let a_public = g.modpow(&a_secret, &n);

        Self {
            n,
            g,
            a_secret,
            a_public,
            username: username.to_vec(),
            password: password.to_vec(),
        }
    }

    pub fn public_key_bytes(&self) -> Vec<u8> {
        pad_to(&self.a_public.to_bytes_be(), KEY_LENGTH)
    }

    pub fn process_challenge(
        &self,
        salt: &[u8],
        server_public_key: &[u8],
    ) -> Result<SrpProof, CryptoError> {
        let b = BigUint::from_bytes_be(server_public_key);

        if &b % &self.n == BigUint::ZERO {
            return Err(CryptoError::KeyDerivationFailed);
        }

        let a_bytes = pad_to(&self.a_public.to_bytes_be(), KEY_LENGTH);
        let b_bytes = pad_to(&b.to_bytes_be(), KEY_LENGTH);

        let identity_hash = sha512_hash(&[&self.username, b":", &self.password]);
        let x = BigUint::from_bytes_be(&sha512_hash(&[salt, &identity_hash]));

        let u = BigUint::from_bytes_be(&sha512_hash(&[&a_bytes, &b_bytes]));
        if u == BigUint::ZERO {
            return Err(CryptoError::KeyDerivationFailed);
        }

        let n_bytes = self.n.to_bytes_be();
        let g_padded = pad_to(&self.g.to_bytes_be(), n_bytes.len());
        let k = BigUint::from_bytes_be(&sha512_hash(&[&n_bytes, &g_padded]));

        // S = (B - k * g^x mod N) ^ (a + u*x) mod N
        let g_x = self.g.modpow(&x, &self.n);
        let k_g_x = (&k * &g_x) % &self.n;
        let base = if b >= k_g_x {
            &b - &k_g_x
        } else {
            &self.n - &k_g_x + &b
        };
        let exp = &self.a_secret + &u * &x;
        let shared_secret = base.modpow(&exp, &self.n);

        let shared_secret_bytes = pad_to(&shared_secret.to_bytes_be(), KEY_LENGTH);
        let session_key = sha512_hash(&[&shared_secret_bytes]);

        // M1 = H(H(N) XOR H(g) | H(username) | salt | A | B | K)
        // Note: g is NOT padded in the H(g) computation (Apple's variant)
        let h_n = sha512_hash(&[&n_bytes]);
        let h_g = sha512_hash(&[&self.g.to_bytes_be()]);
        let h_group: Vec<u8> = h_n.iter().zip(h_g.iter()).map(|(a, b)| a ^ b).collect();
        let h_username = sha512_hash(&[&self.username]);

        let m1 = sha512_hash(&[
            &h_group,
            &h_username,
            salt,
            &a_bytes,
            &b_bytes,
            &session_key,
        ]);

        let m2_expected = sha512_hash(&[&a_bytes, &m1, &session_key]);

        Ok(SrpProof {
            a_public_bytes: a_bytes,
            m1_proof: m1,
            session_key,
            m2_expected,
        })
    }
}

impl SrpProof {
    pub fn verify_server_proof(&self, server_proof: &[u8]) -> bool {
        self.m2_expected == server_proof
    }
}

fn sha512_hash(parts: &[&[u8]]) -> Vec<u8> {
    let mut hasher = Sha512::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().to_vec()
}

fn pad_to(data: &[u8], len: usize) -> Vec<u8> {
    if data.len() >= len {
        return data.to_vec();
    }
    let mut padded = vec![0u8; len - data.len()];
    padded.extend_from_slice(data);
    padded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srp_client_generates_valid_public_key() {
        let client = SrpClient::new(b"Pair-Setup", b"123-45-678");
        let pk = client.public_key_bytes();
        assert_eq!(pk.len(), KEY_LENGTH);
        let a = BigUint::from_bytes_be(&pk);
        let n = BigUint::parse_bytes(N_HEX.as_bytes(), 16).unwrap();
        assert!(a < n);
        assert_ne!(a, BigUint::ZERO);
    }

    #[test]
    fn pad_to_works() {
        assert_eq!(pad_to(&[0x01, 0x02], 4), vec![0, 0, 1, 2]);
        assert_eq!(pad_to(&[0x01, 0x02], 2), vec![1, 2]);
        assert_eq!(pad_to(&[0x01, 0x02], 1), vec![1, 2]);
    }

    #[test]
    fn sha512_hash_deterministic() {
        let h1 = sha512_hash(&[b"test"]);
        let h2 = sha512_hash(&[b"test"]);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn rejects_zero_server_key() {
        let client = SrpClient::new(b"Pair-Setup", b"123-45-678");
        let salt = [0u8; 16];
        let zero_key = vec![0u8; KEY_LENGTH];
        assert!(client.process_challenge(&salt, &zero_key).is_err());
    }
}
