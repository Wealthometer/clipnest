//! Optional encryption at rest. A random data key encrypts clips (XChaCha20-Poly1305); the data
//! key itself is stored wrapped by a key derived from your passphrase (Argon2id). Changing the
//! passphrase only re-wraps the data key. Without the passphrase the clips can't be read: there
//! is no recovery.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// What's saved in settings: never the key, only the salt and the wrapped key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VaultRecord {
    pub salt: String,
    pub wrapped: String,
    pub m_cost: u32,
    pub t_cost: u32,
}

#[derive(Clone)]
pub struct DataKey([u8; 32]);

impl std::fmt::Debug for DataKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DataKey(..)")
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum VaultError {
    #[error("That passphrase isn't right")]
    WrongPassphrase,
    #[error("The passphrase needs at least 8 characters")]
    TooShort,
    #[error("This item couldn't be decrypted")]
    Corrupt,
}

fn kek(pass: &str, salt: &[u8], m_cost: u32, t_cost: u32) -> [u8; 32] {
    let params = Params::new(m_cost, t_cost, 1, Some(32)).expect("valid Argon2 params");
    let mut out = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(pass.as_bytes(), salt, &mut out)
        .expect("Argon2 never fails with valid params");
    out
}

/// Seals bytes: 24-byte nonce followed by the ciphertext.
pub fn seal(key: &[u8; 32], plain: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce);
    let mut out = nonce.to_vec();
    out.extend(cipher.encrypt(XNonce::from_slice(&nonce), plain).expect("encryption doesn't fail"));
    out
}

pub fn open(key: &[u8; 32], sealed: &[u8]) -> Result<Vec<u8>, VaultError> {
    if sealed.len() < 24 + 16 {
        return Err(VaultError::Corrupt);
    }
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher.decrypt(XNonce::from_slice(&sealed[..24]), &sealed[24..]).map_err(|_| VaultError::Corrupt)
}

impl DataKey {
    pub fn seal(&self, plain: &[u8]) -> Vec<u8> {
        seal(&self.0, plain)
    }
    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>, VaultError> {
        open(&self.0, sealed)
    }
    /// A keyed fingerprint, so duplicates can be spotted without storing a plain hash of a secret.
    pub fn fingerprint(&self, data: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(self.0);
        h.update(data);
        hex(&h.finalize())
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn plain_fingerprint(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// Creates a vault for a passphrase. Production uses ~19 MiB / 2 passes (OWASP's Argon2id floor);
/// tests pass smaller costs.
pub fn create(pass: &str, m_cost: u32, t_cost: u32) -> Result<(VaultRecord, DataKey), VaultError> {
    if pass.chars().count() < 8 {
        return Err(VaultError::TooShort);
    }
    let mut salt = [0u8; 16];
    let mut dk = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut dk);
    let k = kek(pass, &salt, m_cost, t_cost);
    let rec = VaultRecord { salt: B64.encode(salt), wrapped: B64.encode(seal(&k, &dk)), m_cost, t_cost };
    Ok((rec, DataKey(dk)))
}

pub fn unlock(rec: &VaultRecord, pass: &str) -> Result<DataKey, VaultError> {
    let salt = B64.decode(&rec.salt).map_err(|_| VaultError::Corrupt)?;
    let wrapped = B64.decode(&rec.wrapped).map_err(|_| VaultError::Corrupt)?;
    let k = kek(pass, &salt, rec.m_cost, rec.t_cost);
    let dk = open(&k, &wrapped).map_err(|_| VaultError::WrongPassphrase)?;
    let arr: [u8; 32] = dk.try_into().map_err(|_| VaultError::Corrupt)?;
    Ok(DataKey(arr))
}

/// Re-wraps the same data key under a new passphrase.
pub fn rewrap(dk: &DataKey, new_pass: &str, m_cost: u32, t_cost: u32) -> Result<VaultRecord, VaultError> {
    if new_pass.chars().count() < 8 {
        return Err(VaultError::TooShort);
    }
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    let k = kek(new_pass, &salt, m_cost, t_cost);
    Ok(VaultRecord { salt: B64.encode(salt), wrapped: B64.encode(seal(&k, &dk.0)), m_cost, t_cost })
}

pub const M_COST: u32 = 19 * 1024;
pub const T_COST: u32 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_wrong_passphrase() {
        let (rec, dk) = create("correct horse", 256, 1).unwrap();
        let sealed = dk.seal(b"secret clip");
        assert_ne!(&sealed[24..], b"secret clip");
        let again = unlock(&rec, "correct horse").unwrap();
        assert_eq!(again.open(&sealed).unwrap(), b"secret clip");
        assert_eq!(unlock(&rec, "wrong horse").unwrap_err(), VaultError::WrongPassphrase);
        assert_eq!(create("short", 256, 1).unwrap_err(), VaultError::TooShort);
        // Tampering is detected.
        let mut bad = sealed.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert_eq!(dk.open(&bad).unwrap_err(), VaultError::Corrupt);
        // New passphrase, same data key.
        let rec2 = rewrap(&dk, "battery staple", 256, 1).unwrap();
        assert_eq!(unlock(&rec2, "battery staple").unwrap().open(&sealed).unwrap(), b"secret clip");
        assert!(unlock(&rec2, "correct horse").is_err());
        // Fingerprints depend on the key, so they don't reveal content.
        assert_ne!(dk.fingerprint(b"x"), plain_fingerprint(b"x"));
        assert_eq!(dk.fingerprint(b"x"), again.fingerprint(b"x"));
    }
}
