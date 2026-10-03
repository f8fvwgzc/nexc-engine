//! AES-256-GCM encryption for secrets at rest (users' LLM API keys).
//!
//! Format: `nonce (12 bytes) || ciphertext || tag (16 bytes)`. A fresh
//! random 96-bit nonce is used for every encryption. The owner's user id is
//! bound as associated data so a ciphertext cannot be moved between users.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};

use super::random::random_bytes;

const NONCE_LEN: usize = 12;

/// Encrypts and decrypts small secrets with the master key.
pub struct SecretBox {
    cipher: Aes256Gcm,
}

impl std::fmt::Debug for SecretBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretBox([redacted])")
    }
}

impl SecretBox {
    /// Box keyed with the 32-byte master key.
    pub fn new(key: &[u8; 32]) -> Self {
        SecretBox {
            cipher: Aes256Gcm::new_from_slice(key).expect("32-byte key"),
        }
    }

    /// Encrypts `plaintext`, authenticating `aad` (e.g. the owner id).
    pub fn seal(&self, plaintext: &[u8], aad: &[u8]) -> anyhow::Result<Vec<u8>> {
        let nonce_bytes = random_bytes::<NONCE_LEN>();
        let nonce = Nonce::try_from(&nonce_bytes[..]).expect("12-byte nonce");
        let ct = self
            .cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| anyhow::anyhow!("encryption failed"))?;
        let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    /// Decrypts a value produced by [`SecretBox::seal`] with the same `aad`.
    pub fn open(&self, sealed: &[u8], aad: &[u8]) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(sealed.len() > NONCE_LEN, "ciphertext too short");
        let (nonce_bytes, ct) = sealed.split_at(NONCE_LEN);
        let nonce = Nonce::try_from(nonce_bytes).map_err(|_| anyhow::anyhow!("bad nonce"))?;
        self.cipher
            .decrypt(&nonce, Payload { msg: ct, aad })
            .map_err(|_| anyhow::anyhow!("decryption failed (wrong key or tampered data)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_detects_tampering() {
        let b = SecretBox::new(&[7u8; 32]);
        let sealed = b.seal(b"sk-ant-secret", b"user-1").unwrap();
        assert_ne!(
            b.seal(b"sk-ant-secret", b"user-1").unwrap(),
            sealed,
            "nonces differ"
        );
        assert_eq!(b.open(&sealed, b"user-1").unwrap(), b"sk-ant-secret");
        assert!(b.open(&sealed, b"user-2").is_err(), "aad is bound");
        let mut bad = sealed.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(b.open(&bad, b"user-1").is_err());
        assert!(SecretBox::new(&[8u8; 32]).open(&sealed, b"user-1").is_err());
        assert!(b.open(&[1, 2, 3], b"").is_err());
    }
}
