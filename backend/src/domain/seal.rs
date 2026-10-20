//! The signed seal of a run's trace. When a run ends, the backend signs a
//! statement of the run's id, how many spans its chain holds, the chain's
//! final hash and when it was sealed, with its Ed25519 key, named by the
//! key's id (`kid`). The statement, the signature and the kid are kept on
//! the run and travel in its evidence bundle: anyone with the published key
//! can check that nexc sealed exactly that chain, without the database.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::security::signing;

/// What a seal statement says it is.
pub const SEAL_TYPE: &str = "nexc.run_seal.v1";

/// The statement a seal signs. Its fields are declared in sorted order, so
/// serialising it gives canonical JSON (sorted keys, no whitespace).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealStatement {
    /// The key the statement is signed with.
    pub kid: String,
    pub run_id: Uuid,
    /// RFC 3339, UTC, microseconds.
    pub sealed_at: String,
    /// Spans in the chain.
    pub spans: i64,
    /// Hex SHA-256 of the chain's last span.
    pub trace_hash: String,
    /// [`SEAL_TYPE`].
    #[serde(rename = "type")]
    pub kind: String,
}

impl SealStatement {
    /// The statement sealing a chain of `spans` spans ending at `hash`.
    pub fn new(run_id: Uuid, spans: i64, hash: &[u8; 32], at: DateTime<Utc>, kid: &str) -> Self {
        SealStatement {
            kid: kid.to_owned(),
            run_id,
            sealed_at: at.to_rfc3339_opts(SecondsFormat::Micros, true),
            spans,
            trace_hash: hex::encode(hash),
            kind: SEAL_TYPE.to_owned(),
        }
    }

    /// The bytes that are signed.
    pub fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("a seal statement serialises")
    }

    /// Reads signed bytes back; `None` when they are not a seal statement.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice::<SealStatement>(bytes)
            .ok()
            .filter(|s| s.kind == SEAL_TYPE)
    }
}

/// A signed seal, as kept on the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedSeal {
    /// The statement's bytes, exactly as signed.
    pub signed: Vec<u8>,
    /// Ed25519 signature of `signed` (64 bytes).
    pub signature: Vec<u8>,
    pub kid: String,
}

impl SignedSeal {
    /// Seals a chain of `spans` spans ending at `hash` with `signer`, now.
    pub fn sign(signer: &signing::Signer, run_id: Uuid, spans: i64, hash: &[u8; 32]) -> Self {
        let statement = SealStatement::new(run_id, spans, hash, Utc::now(), signer.kid());
        let signed = statement.bytes();
        SignedSeal {
            signature: signer.sign(&signed),
            signed,
            kid: signer.kid().to_owned(),
        }
    }

    /// Checks the seal against the run it belongs to and the chain recomputed
    /// from its spans (`chain`: span count and last hash), with the public
    /// key published under the seal's kid (`None` when no key has that id).
    pub fn check(
        &self,
        run_id: Uuid,
        chain: Option<(i64, [u8; 32])>,
        key: Option<&[u8; 32]>,
    ) -> SealVerification {
        let statement = SealStatement::parse(&self.signed);
        let reason = match (&statement, key) {
            (None, _) => Some("the signed bytes are not a seal statement".to_owned()),
            (Some(s), _) if s.kid != self.kid => {
                Some("the statement names another key than the seal".to_owned())
            }
            (Some(_), None) => Some(format!("no published key has the id {}", self.kid)),
            (Some(_), Some(key)) if !signing::verify(key, &self.signed, &self.signature) => {
                Some("the signature does not verify with the published key".to_owned())
            }
            (Some(s), Some(_)) if s.run_id != run_id => {
                Some("the statement seals another run".to_owned())
            }
            (Some(s), Some(_)) => match chain {
                Some((spans, hash)) if s.spans == spans && s.trace_hash == hex::encode(hash) => {
                    None
                }
                _ => Some("the statement does not seal this chain".to_owned()),
            },
        };
        SealVerification {
            ok: reason.is_none(),
            kid: self.kid.clone(),
            sealed_at: statement.map(|s| s.sealed_at),
            reason,
        }
    }
}

/// Whether a run's signed seal holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SealVerification {
    /// True when the signature verifies with the published key named by
    /// `kid` and the signed statement names this run and this chain.
    pub ok: bool,
    /// The key the seal is signed with (`/.well-known/jwks.json`).
    pub kid: String,
    /// When the run was sealed, as signed.
    #[schema(required = true)]
    pub sealed_at: Option<String>,
    /// Why it does not hold, when `ok` is false.
    #[schema(required = true)]
    pub reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::signing::{Signer, derive_seed};

    #[test]
    fn a_seal_holds_for_its_run_and_chain_only() {
        let signer = Signer::from_seed(&derive_seed(b"seal")).unwrap();
        let other = Signer::from_seed(&derive_seed(b"other")).unwrap();
        let run = Uuid::now_v7();
        let hash = [9u8; 32];
        let seal = SignedSeal::sign(&signer, run, 12, &hash);
        let key = signer.public_key();
        let text = String::from_utf8(seal.signed.clone()).unwrap();
        assert!(text.starts_with(r#"{"kid":""#), "{text}");
        assert!(text.contains(r#""type":"nexc.run_seal.v1""#));
        assert!(seal.check(run, Some((12, hash)), Some(&key)).ok);

        let wrong = |v: SealVerification| {
            assert!(!v.ok);
            v.reason.unwrap()
        };
        assert!(wrong(seal.check(run, Some((11, hash)), Some(&key))).contains("chain"));
        assert!(wrong(seal.check(run, Some((12, [8; 32])), Some(&key))).contains("chain"));
        assert!(wrong(seal.check(run, None, Some(&key))).contains("chain"));
        assert!(wrong(seal.check(Uuid::now_v7(), Some((12, hash)), Some(&key))).contains("run"));
        assert!(wrong(seal.check(run, Some((12, hash)), None)).contains("no published key"));
        let other_key = other.public_key();
        assert!(wrong(seal.check(run, Some((12, hash)), Some(&other_key))).contains("signature"));

        let mut forged = seal.clone();
        forged.signed = String::from_utf8(seal.signed.clone())
            .unwrap()
            .replace(r#""spans":12"#, r#""spans":13"#)
            .into_bytes();
        assert!(wrong(forged.check(run, Some((13, hash)), Some(&key))).contains("signature"));
        let mut renamed = seal.clone();
        renamed.kid = other.kid().to_owned();
        assert!(wrong(renamed.check(run, Some((12, hash)), Some(&key))).contains("another key"));
        assert_eq!(SealStatement::parse(b"{}"), None);
    }
}
