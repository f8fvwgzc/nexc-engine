//! Ed25519 signatures over the decisions the Agent Firewall API hands out:
//! an external agent platform keeps the signed bytes, the signature and the
//! public key, and anyone with the key can later check that nexc took that
//! decision, under those policy versions, at that moment.

use aws_lc_rs::signature::{ED25519, Ed25519KeyPair, KeyPair as _, UnparsedPublicKey};

/// Prefix of the seed derivation from the master key.
pub const DERIVATION_TAG: &[u8] = b"nexc-firewall-signing-v1:";

/// The seed the firewall uses when no `NEXC_FIREWALL_SIGNING_KEY` is
/// configured: SHA-256 of a tag and the master key, which only the backend holds.
pub fn derive_seed(master_key: &[u8]) -> [u8; 32] {
    let mut material = DERIVATION_TAG.to_vec();
    material.extend_from_slice(master_key);
    crate::kernel::sha256(&material)
}

/// A signing key and its public half.
pub struct Signer {
    pair: Ed25519KeyPair,
    public: [u8; 32],
}

impl std::fmt::Debug for Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signer")
            .field("public_key", &hex::encode(self.public))
            .finish_non_exhaustive()
    }
}

impl Signer {
    /// The key pair of a 32-byte seed.
    pub fn from_seed(seed: &[u8; 32]) -> anyhow::Result<Self> {
        let pair = Ed25519KeyPair::from_seed_unchecked(seed)
            .map_err(|_| anyhow::anyhow!("the firewall signing seed is not usable"))?;
        let public: [u8; 32] = pair
            .public_key()
            .as_ref()
            .try_into()
            .map_err(|_| anyhow::anyhow!("an Ed25519 public key is 32 bytes"))?;
        Ok(Signer { pair, public })
    }

    /// The public key, raw (32 bytes).
    pub fn public_key(&self) -> [u8; 32] {
        self.public
    }

    /// The signature of `message` (64 bytes).
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.pair.sign(message).as_ref().to_vec()
    }
}

/// Whether `signature` is `public_key`'s signature of `message`.
pub fn verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(message, signature)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_and_verifies_and_rejects_tampering() {
        let signer = Signer::from_seed(&derive_seed(b"master")).unwrap();
        let other = Signer::from_seed(&derive_seed(b"other")).unwrap();
        let message = b"{\"decision\":\"allow\"}";
        let signature = signer.sign(message);
        assert_eq!(signature.len(), 64);
        assert!(verify(&signer.public_key(), message, &signature));
        assert!(!verify(&other.public_key(), message, &signature));
        assert!(!verify(
            &signer.public_key(),
            b"{\"decision\":\"deny\"}",
            &signature
        ));
        let mut bad = signature.clone();
        bad[3] ^= 1;
        assert!(!verify(&signer.public_key(), message, &bad));
        assert!(!verify(b"short", message, &signature));
        assert_ne!(derive_seed(b"a"), derive_seed(b"b"));
        assert!(format!("{signer:?}").contains("public_key"));
    }
}
