//! Per-run agent tokens: what the backend hands the agent runtime with every
//! node, signed so that the runtime (which is not trusted) can only do what
//! the backend decided it may. EdDSA (Ed25519): only the backend holds the
//! private key; the runtime and any external verifier get public keys, from
//! their configuration or from `GET /api/v1/agent-tokens/jwks.json`, and
//! pick the one the token's `kid` names. The claims: the principal, the human
//! it acts for, the run and node, a snapshot of the capabilities and the
//! policy version that governed them. The token outlives the node's timeout
//! by [`EXPIRY_SLACK`] and nothing else.

use std::time::Duration;

use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair as _};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, decode_header, encode,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::domain::capabilities::{Capabilities, DataLabel};
use crate::domain::policy::Rules;
use crate::domain::workspace::WorkspaceRole;

/// What the runtime sends up of what its agents say and hear: the kinds the
/// workspace records. Nothing travels for a kind that is off.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordingClaim {
    #[serde(default)]
    pub prompts: bool,
    #[serde(default)]
    pub completions: bool,
    #[serde(default)]
    pub tool_io: bool,
}

/// The human an agent acts for: who started the run, and their role in the
/// run's workspace when it was prepared.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct OnBehalfOf {
    pub user_id: Uuid,
    /// `null` for a run outside any workspace.
    #[schema(required = true)]
    pub workspace_role: Option<WorkspaceRole>,
}

impl std::fmt::Display for OnBehalfOf {
    /// `user <id> (<role>)`, as audit details name the human.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.workspace_role {
            Some(role) => write!(f, "user {} ({})", self.user_id, role.as_str()),
            None => write!(f, "user {} (no workspace)", self.user_id),
        }
    }
}

const ISSUER: &str = "nexc-engine";
const AUDIENCE: &str = "nexc-runtime";
/// Grace on top of the node's timeout before its token expires.
pub const EXPIRY_SLACK: Duration = Duration::from_secs(60);
/// Prefix of the seed derivation from the master key.
pub const DERIVATION_TAG: &[u8] = b"nexc-agent-token-v2:";

/// Claims of a per-run agent token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentClaims {
    pub iss: String,
    pub aud: String,
    /// The principal: `agent://{workspace}/{name}`, or `system`.
    pub sub: String,
    pub jti: Uuid,
    pub iat: i64,
    pub nbf: i64,
    pub exp: i64,
    pub run_id: Uuid,
    pub node_id: Uuid,
    /// The human the agent acts for. Required: a token without it is refused.
    pub on_behalf_of: OnBehalfOf,
    #[serde(default)]
    pub agent_id: Option<Uuid>,
    #[serde(default)]
    pub workspace_id: Option<Uuid>,
    /// The principal's place in the delegation tree (the root agent is 0).
    #[serde(default)]
    pub depth: u8,
    #[serde(default)]
    pub policy_version: i32,
    #[serde(default)]
    pub capabilities: Capabilities,
    /// The resolved rules beside the capabilities (approvals, limits, DLP,
    /// the injection screen, the policy versions in force).
    #[serde(default)]
    pub rules: Rules,
    /// The classifications of the data the node was given.
    #[serde(default)]
    pub labels: Vec<DataLabel>,
    /// What the runtime sends up of what its agents say and hear.
    #[serde(default)]
    pub recording: RecordingClaim,
}

/// What a token grants: everything but the times and the id.
#[derive(Debug, Clone)]
pub struct AgentGrant<'a> {
    pub principal: &'a str,
    pub on_behalf_of: OnBehalfOf,
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub workspace_id: Option<Uuid>,
    pub policy_version: i32,
    pub capabilities: &'a Capabilities,
    pub rules: &'a Rules,
    pub labels: &'a [DataLabel],
    pub recording: RecordingClaim,
}

/// A token the backend issued: the compact JWT and what identifies it.
#[derive(Debug, Clone)]
pub struct Issued {
    pub token: String,
    pub jti: Uuid,
    pub kid: String,
    pub exp: i64,
}

/// The seed agent tokens are signed with when no `NEXC_AGENT_TOKEN_SIGNING_KEY`
/// is configured: SHA-256 of a tag and the master key, which only the backend
/// holds (the runtime never sees it). Another tag than the firewall's, so the
/// two keys differ.
pub fn derive_seed(master_key: &[u8]) -> [u8; 32] {
    let mut material = DERIVATION_TAG.to_vec();
    material.extend_from_slice(master_key);
    crate::kernel::sha256(&material)
}

/// The key id of an Ed25519 public key: its RFC 7638 JWK thumbprint
/// (SHA-256 of `{"crv":"Ed25519","kty":"OKP","x":…}`, base64url), so that
/// anyone holding the key computes the same id.
pub fn key_id(public: &[u8; 32]) -> String {
    let canonical = format!(
        r#"{{"crv":"Ed25519","kty":"OKP","x":"{}"}}"#,
        URL_SAFE_NO_PAD.encode(public)
    );
    URL_SAFE_NO_PAD.encode(crate::kernel::sha256(canonical.as_bytes()))
}

/// A public key as configured: base64url (a JWK's `x`) or base64 of 32 bytes.
pub fn parse_public_key(raw: &str) -> Option<[u8; 32]> {
    let raw = raw.trim().trim_end_matches('=');
    let bytes = URL_SAFE_NO_PAD
        .decode(raw)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(raw))
        .ok()?;
    <[u8; 32]>::try_from(bytes.as_slice()).ok()
}

/// The public half of an Ed25519 seed.
pub fn public_key_of(seed: &[u8; 32]) -> anyhow::Result<[u8; 32]> {
    let pair = Ed25519KeyPair::from_seed_unchecked(seed)
        .map_err(|_| anyhow::anyhow!("the agent token signing seed is not usable"))?;
    pair.public_key()
        .as_ref()
        .try_into()
        .map_err(|_| anyhow::anyhow!("an Ed25519 public key is 32 bytes"))
}

/// One key of the set the backend publishes (RFC 7517, `kty` `OKP`, RFC 8037).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Jwk {
    /// `OKP`.
    pub kty: &'static str,
    /// `Ed25519`.
    pub crv: &'static str,
    /// The public key, base64url of its 32 bytes.
    pub x: String,
    /// The key id tokens name in their header: the RFC 7638 thumbprint.
    pub kid: String,
    /// `EdDSA`.
    pub alg: &'static str,
    /// `sig`.
    #[serde(rename = "use")]
    pub use_: &'static str,
}

/// The public keys agent tokens verify with: the one the backend signs with
/// first, then the others it still accepts (the previous one after a
/// rotation, the next one before it).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Jwks {
    pub keys: Vec<Jwk>,
}

/// A public key that verifies agent tokens.
struct Verifier {
    kid: String,
    public: [u8; 32],
    decoding: DecodingKey,
}

/// The signing key of agent tokens and the public keys they verify with.
pub struct AgentTokenKeys {
    kid: String,
    encoding: EncodingKey,
    /// The signing key's public half first.
    verifiers: Vec<Verifier>,
    validation: Validation,
}

impl std::fmt::Debug for AgentTokenKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentTokenKeys")
            .field("kid", &self.kid)
            .finish_non_exhaustive()
    }
}

impl AgentTokenKeys {
    /// Signs with the Ed25519 key of `seed`; verifies with its public half
    /// and with every key of `also_verify`.
    pub fn new(seed: &[u8; 32], also_verify: &[[u8; 32]]) -> anyhow::Result<Self> {
        let pair = Ed25519KeyPair::from_seed_unchecked(seed)
            .map_err(|_| anyhow::anyhow!("the agent token signing seed is not usable"))?;
        let pkcs8 = pair
            .to_pkcs8()
            .map_err(|_| anyhow::anyhow!("the agent token signing key cannot be encoded"))?;
        let own = public_key_of(seed)?;
        let mut verifiers: Vec<Verifier> = Vec::with_capacity(1 + also_verify.len());
        for public in std::iter::once(&own).chain(also_verify) {
            if verifiers.iter().any(|v| v.public == *public) {
                continue;
            }
            verifiers.push(Verifier {
                kid: key_id(public),
                public: *public,
                decoding: DecodingKey::from_ed_der(public),
            });
        }
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.set_issuer(&[ISSUER]);
        validation.set_audience(&[AUDIENCE]);
        validation.set_required_spec_claims(&["exp", "nbf", "iss", "aud", "sub"]);
        validation.validate_nbf = true;
        validation.leeway = 5;
        Ok(AgentTokenKeys {
            kid: key_id(&own),
            encoding: EncodingKey::from_ed_der(pkcs8.as_ref()),
            verifiers,
            validation,
        })
    }

    /// The id of the key tokens are signed with now.
    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// The public half of the signing key.
    pub fn public_key(&self) -> [u8; 32] {
        self.verifiers[0].public
    }

    /// Every key a token may name, the signing key first.
    pub fn jwks(&self) -> Jwks {
        Jwks {
            keys: self
                .verifiers
                .iter()
                .map(|v| Jwk {
                    kty: "OKP",
                    crv: "Ed25519",
                    x: URL_SAFE_NO_PAD.encode(v.public),
                    kid: v.kid.clone(),
                    alg: "EdDSA",
                    use_: "sig",
                })
                .collect(),
        }
    }

    /// Issues a token for `grant` that is valid for `ttl` from now.
    pub fn issue(&self, grant: &AgentGrant<'_>, ttl: Duration) -> anyhow::Result<Issued> {
        let now = chrono::Utc::now().timestamp();
        let claims = AgentClaims {
            iss: ISSUER.into(),
            aud: AUDIENCE.into(),
            sub: grant.principal.to_owned(),
            jti: Uuid::now_v7(),
            iat: now,
            nbf: now,
            exp: now + ttl.as_secs() as i64,
            run_id: grant.run_id,
            node_id: grant.node_id,
            on_behalf_of: grant.on_behalf_of,
            agent_id: grant.agent_id,
            workspace_id: grant.workspace_id,
            depth: 0,
            policy_version: grant.policy_version,
            capabilities: grant.capabilities.clone(),
            rules: grant.rules.clone(),
            labels: grant.labels.to_vec(),
            recording: grant.recording,
        };
        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some(self.kid.clone());
        Ok(Issued {
            token: encode(&header, &claims, &self.encoding)?,
            jti: claims.jti,
            kid: self.kid.clone(),
            exp: claims.exp,
        })
    }

    /// Verifies the algorithm (EdDSA only), the key the header's `kid` names
    /// (one of ours, else refused), the signature, issuer, audience and time
    /// claims.
    pub fn verify(&self, token: &str) -> Option<AgentClaims> {
        let header = decode_header(token).ok()?;
        if header.alg != Algorithm::EdDSA {
            return None;
        }
        let kid = header.kid?;
        let verifier = self.verifiers.iter().find(|v| v.kid == kid)?;
        decode::<AgentClaims>(token, &verifier.decoding, &self.validation)
            .ok()
            .map(|d| d.claims)
    }
}

#[cfg(test)]
mod tests {
    use base64::engine::general_purpose::STANDARD;

    use super::*;
    use crate::domain::user::Role;
    use crate::security::jwt::JwtKeys;

    const SEED: [u8; 32] = [7; 32];
    const RUNTIME_TOKEN: &str = "test-runtime-token-0123456789-abcdefghij";

    fn grant<'a>(caps: &'a Capabilities, run: Uuid, node: Uuid) -> AgentGrant<'a> {
        AgentGrant {
            principal: "agent://ws/Researcher",
            on_behalf_of: OnBehalfOf {
                user_id: Uuid::nil(),
                workspace_role: Some(WorkspaceRole::Admin),
            },
            run_id: run,
            node_id: node,
            agent_id: Some(Uuid::now_v7()),
            workspace_id: Some(Uuid::now_v7()),
            policy_version: 3,
            capabilities: caps,
            rules: &RULES,
            labels: &[DataLabel::Internal],
            recording: RecordingClaim {
                prompts: true,
                ..RecordingClaim::default()
            },
        }
    }

    static RULES: std::sync::LazyLock<Rules> = std::sync::LazyLock::new(Rules::default);

    fn claims_now(caps: &Capabilities) -> AgentClaims {
        let now = chrono::Utc::now().timestamp();
        AgentClaims {
            iss: ISSUER.into(),
            aud: AUDIENCE.into(),
            sub: "agent://ws/Researcher".into(),
            jti: Uuid::now_v7(),
            iat: now,
            nbf: now,
            exp: now + 600,
            run_id: Uuid::now_v7(),
            node_id: Uuid::now_v7(),
            on_behalf_of: OnBehalfOf {
                user_id: Uuid::nil(),
                workspace_role: Some(WorkspaceRole::Owner),
            },
            agent_id: None,
            workspace_id: None,
            depth: 0,
            policy_version: 0,
            capabilities: caps.clone(),
            rules: Rules::default(),
            labels: Vec::new(),
            recording: RecordingClaim::default(),
        }
    }

    /// The HS256 key the backend and the runtime used to share (removed): the
    /// runtime could rebuild it from its own bearer secret.
    fn old_shared_key(runtime_token: &str) -> [u8; 32] {
        let mut material = b"nexc-agent-token-v1:".to_vec();
        material.extend_from_slice(runtime_token.as_bytes());
        crate::kernel::sha256(&material)
    }

    #[test]
    fn issues_and_verifies_with_the_capabilities_and_the_human_inside() {
        let keys = AgentTokenKeys::new(&SEED, &[]).unwrap();
        let caps = Capabilities {
            tools: Some(vec!["write_file".into()]),
            ..Capabilities::default()
        };
        let (run, node) = (Uuid::now_v7(), Uuid::now_v7());
        let issued = keys
            .issue(&grant(&caps, run, node), Duration::from_secs(660))
            .unwrap();
        let header = decode_header(&issued.token).unwrap();
        assert_eq!(header.alg, Algorithm::EdDSA);
        assert_eq!(header.kid.as_deref(), Some(keys.kid()));
        assert_eq!(issued.kid, keys.kid());
        let claims = keys.verify(&issued.token).unwrap();
        assert_eq!(claims.jti, issued.jti);
        assert_eq!(claims.exp, issued.exp);
        assert_eq!(claims.sub, "agent://ws/Researcher");
        assert_eq!(
            claims.on_behalf_of,
            OnBehalfOf {
                user_id: Uuid::nil(),
                workspace_role: Some(WorkspaceRole::Admin)
            }
        );
        assert_eq!((claims.run_id, claims.node_id), (run, node));
        assert_eq!(claims.capabilities, caps);
        assert_eq!(claims.rules, Rules::default());
        assert_eq!(claims.labels, vec![DataLabel::Internal]);
        assert!(claims.recording.prompts && !claims.recording.tool_io);
        assert_eq!((claims.policy_version, claims.depth), (3, 0));
        assert_eq!(claims.exp - claims.iat, 660);
        // The claim is on the wire as the runtime and external verifiers read it.
        let payload = issued.token.split('.').nth(1).unwrap();
        let raw: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).unwrap()).unwrap();
        assert_eq!(
            raw["on_behalf_of"],
            serde_json::json!({"user_id": Uuid::nil(), "workspace_role": "admin"})
        );
    }

    #[test]
    fn the_key_id_is_the_rfc_7638_thumbprint_and_the_set_lists_every_key() {
        // RFC 8037 appendix A.3: the thumbprint of its example Ed25519 key.
        let x = parse_public_key("11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo").unwrap();
        assert_eq!(key_id(&x), "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k");
        assert_eq!(
            parse_public_key(&STANDARD.encode(x)),
            Some(x),
            "standard base64 too"
        );
        assert_eq!(parse_public_key("short"), None);

        let previous = public_key_of(&[9; 32]).unwrap();
        let keys = AgentTokenKeys::new(&SEED, &[previous, previous]).unwrap();
        let set = serde_json::to_value(keys.jwks()).unwrap();
        let listed = set["keys"].as_array().unwrap();
        assert_eq!(listed.len(), 2, "duplicates listed once: {set}");
        assert_eq!(listed[0]["kid"], keys.kid());
        assert_eq!(listed[1]["kid"], key_id(&previous));
        for k in listed {
            assert_eq!(
                (&k["kty"], &k["crv"], &k["alg"], &k["use"]),
                (
                    &serde_json::json!("OKP"),
                    &serde_json::json!("Ed25519"),
                    &serde_json::json!("EdDSA"),
                    &serde_json::json!("sig")
                )
            );
            // Any JWK library reads the key back and computes the same id.
            let jwk: jsonwebtoken::jwk::Jwk = serde_json::from_value(k.clone()).unwrap();
            assert!(DecodingKey::from_jwk(&jwk).is_ok());
            assert_eq!(
                key_id(&parse_public_key(k["x"].as_str().unwrap()).unwrap()),
                k["kid"]
            );
        }
        assert_eq!(keys.public_key(), public_key_of(&SEED).unwrap());
        assert_ne!(derive_seed(b"a"), derive_seed(b"b"));
        assert_ne!(
            derive_seed(b"master"),
            crate::security::signing::derive_seed(b"master"),
            "not the firewall's key"
        );
    }

    #[test]
    fn tokens_verify_by_kid_across_a_rotation() {
        let old_seed = [1u8; 32];
        let new_seed = [2u8; 32];
        let caps = Capabilities::default();
        let before = AgentTokenKeys::new(&old_seed, &[]).unwrap();
        let minted_before = before
            .issue(
                &grant(&caps, Uuid::now_v7(), Uuid::now_v7()),
                Duration::from_secs(60),
            )
            .unwrap();
        // Step 1 (every replica): the next key is published and accepted, nothing signs with it yet.
        let next = public_key_of(&new_seed).unwrap();
        let announcing = AgentTokenKeys::new(&old_seed, &[next]).unwrap();
        assert_eq!(announcing.kid(), before.kid());
        assert!(announcing.verify(&minted_before.token).is_some());
        // Step 2: the new key signs; the old one still verifies what it signed.
        let old = public_key_of(&old_seed).unwrap();
        let after = AgentTokenKeys::new(&new_seed, &[old]).unwrap();
        assert_ne!(after.kid(), before.kid());
        let minted_after = after
            .issue(
                &grant(&caps, Uuid::now_v7(), Uuid::now_v7()),
                Duration::from_secs(60),
            )
            .unwrap();
        assert_eq!(minted_after.kid, key_id(&next));
        for keys in [&announcing, &after] {
            assert!(keys.verify(&minted_before.token).is_some());
            assert!(keys.verify(&minted_after.token).is_some());
        }
        // Step 3: the old key is retired; what it signed no longer verifies.
        let retired = AgentTokenKeys::new(&new_seed, &[]).unwrap();
        assert!(retired.verify(&minted_before.token).is_none());
        assert!(retired.verify(&minted_after.token).is_some());
        // A token whose kid names a listed key but was signed by another is refused.
        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some(after.kid().to_owned());
        let stranger = AgentTokenKeys::new(&[3; 32], &[]).unwrap();
        let mislabelled = encode(&header, &claims_now(&caps), &stranger.encoding).unwrap();
        assert!(after.verify(&mislabelled).is_none());
        // And one without a kid at all.
        let unnamed = encode(
            &Header::new(Algorithm::EdDSA),
            &claims_now(&caps),
            &after.encoding,
        )
        .unwrap();
        assert!(after.verify(&unnamed).is_none());
    }

    #[test]
    fn refuses_tokens_signed_with_the_runtimes_secret_or_any_key_but_ours() {
        let keys = AgentTokenKeys::new(&SEED, &[]).unwrap();
        let caps = Capabilities::default();
        let good = keys
            .issue(
                &grant(&caps, Uuid::now_v7(), Uuid::now_v7()),
                Duration::from_secs(60),
            )
            .unwrap()
            .token;
        assert!(keys.verify(&good).is_some());
        let claims = claims_now(&caps);
        // What a compromised runtime could mint before: HS256 under the key derived from its
        // bearer secret, or under the bearer secret itself, with or without our kid.
        for secret in [
            old_shared_key(RUNTIME_TOKEN).to_vec(),
            RUNTIME_TOKEN.as_bytes().to_vec(),
        ] {
            for kid in [None, Some(keys.kid().to_owned())] {
                let mut header = Header::new(Algorithm::HS256);
                header.kid = kid;
                let forged = encode(&header, &claims, &EncodingKey::from_secret(&secret)).unwrap();
                assert!(keys.verify(&forged).is_none());
            }
        }
        // HS256 keyed with our public key (the classic algorithm confusion).
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some(keys.kid().to_owned());
        let confused = encode(
            &header,
            &claims,
            &EncodingKey::from_secret(&keys.public_key()),
        )
        .unwrap();
        assert!(keys.verify(&confused).is_none());
        // Another Ed25519 key, under its own kid.
        let other = AgentTokenKeys::new(&[8; 32], &[]).unwrap();
        let foreign = other
            .issue(
                &grant(&caps, Uuid::now_v7(), Uuid::now_v7()),
                Duration::from_secs(60),
            )
            .unwrap()
            .token;
        assert!(keys.verify(&foreign).is_none());
        // The payload changed (a wider capabilities snapshot) under the same signature.
        let mut parts: Vec<&str> = good.split('.').collect();
        let mut widened = claims.clone();
        widened.capabilities.tools = None;
        let widened = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&widened).unwrap());
        parts[1] = &widened;
        assert!(keys.verify(&parts.join(".")).is_none());
        let mut tampered = good.clone();
        tampered.push('x');
        assert!(keys.verify(&tampered).is_none());
        // Expired.
        let now = chrono::Utc::now().timestamp();
        let expired = AgentClaims {
            iat: now - 600,
            nbf: now - 600,
            exp: now - 60,
            ..claims.clone()
        };
        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some(keys.kid().to_owned());
        assert!(
            keys.verify(&encode(&header, &expired, &keys.encoding).unwrap())
                .is_none()
        );
        // Without the human it acts for.
        let mut bare = serde_json::to_value(&claims).unwrap();
        bare.as_object_mut().unwrap().remove("on_behalf_of");
        assert!(
            keys.verify(&encode(&header, &bare, &keys.encoding).unwrap())
                .is_none()
        );
        // A user's access token has the wrong algorithm and audience.
        let user = JwtKeys::new(&SEED, Duration::from_secs(900))
            .issue(Uuid::now_v7(), Role::User, 0)
            .unwrap();
        assert!(keys.verify(&user).is_none());
        assert!(keys.verify("not.a.jwt").is_none());
        let none = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJzdWIiOiJ4In0.";
        assert!(keys.verify(none).is_none());
    }
}
