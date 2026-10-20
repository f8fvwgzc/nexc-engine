//! Ed25519 signatures over what nexc vouches for: the decisions the Agent
//! Firewall API hands out, the seal of every run's trace, and the evidence
//! bundles it exports. Whoever keeps the signed bytes and the signature can
//! later check them against the public key, named by its key id (`kid`, the
//! RFC 7638 thumbprint of the key) and published with the keys it replaced
//! at `/.well-known/jwks.json`.

use aws_lc_rs::signature::{ED25519, Ed25519KeyPair, KeyPair as _, UnparsedPublicKey};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Prefix of the seed derivation from the master key.
pub const DERIVATION_TAG: &[u8] = b"nexc-firewall-signing-v1:";

/// The seed nexc signs with when no `NEXC_SIGNING_KEY` is
/// configured: SHA-256 of a tag and the master key, which only the backend holds.
pub fn derive_seed(master_key: &[u8]) -> [u8; 32] {
    let mut material = DERIVATION_TAG.to_vec();
    material.extend_from_slice(master_key);
    crate::kernel::sha256(&material)
}

/// The key id of an Ed25519 public key: its RFC 7638 JWK thumbprint, the
/// base64url SHA-256 of `{"crv":"Ed25519","kty":"OKP","x":"<key>"}`. Anyone
/// holding the key computes the same id, so an id cannot name another key.
pub fn kid(public_key: &[u8; 32]) -> String {
    let jwk = format!(
        r#"{{"crv":"Ed25519","kty":"OKP","x":"{}"}}"#,
        URL_SAFE_NO_PAD.encode(public_key)
    );
    URL_SAFE_NO_PAD.encode(crate::kernel::sha256(jwk.as_bytes()))
}

/// A signing key and its public half.
pub struct Signer {
    pair: Ed25519KeyPair,
    public: [u8; 32],
    kid: String,
}

impl std::fmt::Debug for Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Signer")
            .field("public_key", &hex::encode(self.public))
            .field("kid", &self.kid)
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
        Ok(Signer {
            kid: kid(&public),
            pair,
            public,
        })
    }

    /// The public key, raw (32 bytes).
    pub fn public_key(&self) -> [u8; 32] {
        self.public
    }

    /// The id the public key is published under.
    pub fn kid(&self) -> &str {
        &self.kid
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

/// Whether a published key signs today or only verifies what it signed before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum KeyStatus {
    Current,
    Retired,
}

/// One public key as published (RFC 8037 OKP key in a JWK set), with its status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Jwk {
    /// `OKP`.
    pub kty: String,
    /// `Ed25519`.
    pub crv: String,
    /// The raw 32-byte public key, base64url without padding.
    pub x: String,
    /// The RFC 7638 thumbprint of the key.
    pub kid: String,
    /// `EdDSA`.
    pub alg: String,
    /// `sig`.
    #[serde(rename = "use")]
    pub use_: String,
    pub status: KeyStatus,
}

impl Jwk {
    /// The JWK of `public_key`.
    pub fn of(public_key: &[u8; 32], status: KeyStatus) -> Self {
        Jwk {
            kty: "OKP".into(),
            crv: "Ed25519".into(),
            x: URL_SAFE_NO_PAD.encode(public_key),
            kid: kid(public_key),
            alg: "EdDSA".into(),
            use_: "sig".into(),
            status,
        }
    }

    /// The key, when this is an Ed25519 key whose `kid` is its own thumbprint.
    pub fn public_key(&self) -> Option<[u8; 32]> {
        if self.kty != "OKP" || self.crv != "Ed25519" {
            return None;
        }
        let key: [u8; 32] = URL_SAFE_NO_PAD.decode(&self.x).ok()?.try_into().ok()?;
        (kid(&key) == self.kid).then_some(key)
    }
}

/// The keys nexc signs and has signed with (`GET /.well-known/jwks.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Jwks {
    pub keys: Vec<Jwk>,
}

impl Jwks {
    /// The key published under `kid`, if it is a usable Ed25519 key.
    pub fn find(&self, kid: &str) -> Option<[u8; 32]> {
        self.keys
            .iter()
            .filter(|k| k.kid == kid)
            .find_map(Jwk::public_key)
    }
}

/// The current signing key and the public halves of the keys it replaced:
/// what was signed before a rotation still verifies by its `kid`.
#[derive(Debug)]
pub struct Keyring {
    current: std::sync::Arc<Signer>,
    retired: Vec<[u8; 32]>,
}

impl Keyring {
    /// A keyring signing with `current`; `retired` keys are published for verification only.
    pub fn new(current: std::sync::Arc<Signer>, retired: Vec<[u8; 32]>) -> Self {
        let retired = retired
            .into_iter()
            .filter(|key| *key != current.public_key())
            .collect();
        Keyring { current, retired }
    }

    /// The key that signs.
    pub fn current(&self) -> &Signer {
        &self.current
    }

    /// The public key published under `kid`, current or retired.
    pub fn public_key(&self, kid: &str) -> Option<[u8; 32]> {
        if kid == self.current.kid() {
            return Some(self.current.public_key());
        }
        self.retired
            .iter()
            .find(|key| self::kid(key) == kid)
            .copied()
    }

    /// Every key as published: the current one first.
    pub fn jwks(&self) -> Jwks {
        let mut keys = vec![Jwk::of(&self.current.public_key(), KeyStatus::Current)];
        keys.extend(
            self.retired
                .iter()
                .map(|key| Jwk::of(key, KeyStatus::Retired)),
        );
        Jwks { keys }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_named_by_their_thumbprint_and_published_with_the_retired_ones() {
        let old = Signer::from_seed(&derive_seed(b"old")).unwrap();
        let new = std::sync::Arc::new(Signer::from_seed(&derive_seed(b"new")).unwrap());
        assert_eq!(new.kid().len(), 43, "base64url of 32 bytes");
        assert_ne!(new.kid(), old.kid());
        let ring = Keyring::new(new.clone(), vec![old.public_key(), new.public_key()]);
        let jwks = ring.jwks();
        assert_eq!(jwks.keys.len(), 2, "the current key is not listed twice");
        assert_eq!(jwks.keys[0].status, KeyStatus::Current);
        assert_eq!(jwks.keys[1].kid, old.kid());
        assert_eq!(ring.public_key(old.kid()), Some(old.public_key()));
        assert_eq!(jwks.find(new.kid()), Some(new.public_key()));
        assert_eq!(ring.public_key("nope"), None);
        // A key published under another key's id is not taken.
        let mut forged = jwks.clone();
        forged.keys[1].kid = new.kid().to_owned();
        forged.keys.remove(0);
        assert_eq!(forged.find(new.kid()), None);
        let text = serde_json::to_string(&jwks).unwrap();
        assert!(text.contains(r#""use":"sig""#) && text.contains(r#""crv":"Ed25519""#));
    }

    /// RFC 8037 Appendix A.3: the thumbprint of its example Ed25519 key.
    #[test]
    fn the_kid_is_the_rfc_7638_thumbprint() {
        let x = URL_SAFE_NO_PAD
            .decode("11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo")
            .unwrap();
        let key: [u8; 32] = x.try_into().unwrap();
        assert_eq!(kid(&key), "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k");
    }

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
