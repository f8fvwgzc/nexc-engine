//! Per-run agent tokens: what the backend hands the agent runtime with every
//! node, signed so that the runtime (which is not trusted) can only do what
//! the backend decided it may. HS256 like the access tokens, with its own
//! key, audience and claims: the principal, the run and node, a snapshot of
//! the capabilities and the policy version that governed them. The token
//! outlives the node's timeout by [`EXPIRY_SLACK`] and nothing else.

use std::time::Duration;

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::capabilities::Capabilities;

const ISSUER: &str = "nexc-engine";
const AUDIENCE: &str = "nexc-runtime";
/// Grace on top of the node's timeout before its token expires.
pub const EXPIRY_SLACK: Duration = Duration::from_secs(60);
/// Prefix of the key derivation from the runtime's shared secret.
pub const DERIVATION_TAG: &[u8] = b"nexc-agent-token-v1:";

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
}

/// What a token grants: everything but the times and the id.
#[derive(Debug, Clone)]
pub struct AgentGrant<'a> {
    pub principal: &'a str,
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub workspace_id: Option<Uuid>,
    pub policy_version: i32,
    pub capabilities: &'a Capabilities,
}

/// Signing and verification keys of agent tokens.
pub struct AgentTokenKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
    validation: Validation,
}

impl std::fmt::Debug for AgentTokenKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentTokenKeys").finish_non_exhaustive()
    }
}

/// The key the backend and the runtime share when no `NEXC_AGENT_TOKEN_KEY`
/// is configured: SHA-256 of a tag and the runtime's bearer secret, which
/// both already hold. The master key never leaves the backend, so it cannot
/// be the source.
pub fn derive_key(runtime_token: &str) -> [u8; 32] {
    let mut material = DERIVATION_TAG.to_vec();
    material.extend_from_slice(runtime_token.as_bytes());
    crate::kernel::sha256(&material)
}

impl AgentTokenKeys {
    /// Keys from `secret` (≥ 32 bytes, enforced by config).
    pub fn new(secret: &[u8]) -> Self {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[ISSUER]);
        validation.set_audience(&[AUDIENCE]);
        validation.set_required_spec_claims(&["exp", "nbf", "iss", "aud", "sub"]);
        validation.validate_nbf = true;
        validation.leeway = 5;
        AgentTokenKeys {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
            validation,
        }
    }

    /// Issues a token for `grant` that is valid for `ttl` from now.
    pub fn issue(&self, grant: &AgentGrant<'_>, ttl: Duration) -> anyhow::Result<String> {
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
            agent_id: grant.agent_id,
            workspace_id: grant.workspace_id,
            depth: 0,
            policy_version: grant.policy_version,
            capabilities: grant.capabilities.clone(),
        };
        Ok(encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &self.encoding,
        )?)
    }

    /// Verifies signature, algorithm, issuer, audience and time claims.
    pub fn verify(&self, token: &str) -> Option<AgentClaims> {
        decode::<AgentClaims>(token, &self.decoding, &self.validation)
            .ok()
            .map(|d| d.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::user::Role;
    use crate::security::jwt::JwtKeys;

    const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";

    fn grant<'a>(caps: &'a Capabilities, run: Uuid, node: Uuid) -> AgentGrant<'a> {
        AgentGrant {
            principal: "agent://ws/Researcher",
            run_id: run,
            node_id: node,
            agent_id: Some(Uuid::now_v7()),
            workspace_id: Some(Uuid::now_v7()),
            policy_version: 3,
            capabilities: caps,
        }
    }

    #[test]
    fn issues_and_verifies_with_the_capabilities_inside() {
        let keys = AgentTokenKeys::new(SECRET);
        let caps = Capabilities {
            tools: Some(vec!["write_file".into()]),
            ..Capabilities::default()
        };
        let (run, node) = (Uuid::now_v7(), Uuid::now_v7());
        let token = keys
            .issue(&grant(&caps, run, node), Duration::from_secs(660))
            .unwrap();
        let claims = keys.verify(&token).unwrap();
        assert_eq!(claims.sub, "agent://ws/Researcher");
        assert_eq!((claims.run_id, claims.node_id), (run, node));
        assert_eq!(claims.capabilities, caps);
        assert_eq!((claims.policy_version, claims.depth), (3, 0));
        assert_eq!(claims.exp - claims.iat, 660);
        assert_eq!(derive_key("abc").len(), 32);
        assert_ne!(derive_key("abc"), derive_key("abd"));
    }

    #[test]
    fn refuses_tampered_foreign_expired_and_user_tokens() {
        let keys = AgentTokenKeys::new(SECRET);
        let caps = Capabilities::default();
        let good = keys
            .issue(
                &grant(&caps, Uuid::now_v7(), Uuid::now_v7()),
                Duration::from_secs(60),
            )
            .unwrap();
        assert!(keys.verify(&good).is_some());
        // The payload changed (a wider capabilities snapshot) under the same signature.
        let mut parts: Vec<&str> = good.split('.').collect();
        let widened = base64url(br#"{"iss":"nexc-engine","aud":"nexc-runtime","sub":"agent://ws/Researcher","jti":"0199bc0e-0000-7000-8000-000000000000","iat":0,"nbf":0,"exp":9999999999,"run_id":"0199bc0e-0000-7000-8000-000000000000","node_id":"0199bc0e-0000-7000-8000-000000000000","capabilities":{}}"#);
        parts[1] = &widened;
        assert!(keys.verify(&parts.join(".")).is_none());
        let mut tampered = good.clone();
        tampered.push('x');
        assert!(keys.verify(&tampered).is_none());
        // Another key.
        let other = AgentTokenKeys::new(b"ffffffffffffffffffffffffffffffff");
        let foreign = other
            .issue(
                &grant(&caps, Uuid::now_v7(), Uuid::now_v7()),
                Duration::from_secs(60),
            )
            .unwrap();
        assert!(keys.verify(&foreign).is_none());
        // Expired.
        let now = chrono::Utc::now().timestamp();
        let expired = AgentClaims {
            iss: ISSUER.into(),
            aud: AUDIENCE.into(),
            sub: "system".into(),
            jti: Uuid::now_v7(),
            iat: now - 600,
            nbf: now - 600,
            exp: now - 60,
            run_id: Uuid::now_v7(),
            node_id: Uuid::now_v7(),
            agent_id: None,
            workspace_id: None,
            depth: 0,
            policy_version: 0,
            capabilities: caps.clone(),
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &expired,
            &EncodingKey::from_secret(SECRET),
        )
        .unwrap();
        assert!(keys.verify(&token).is_none());
        // A user's access token signed with the same bytes has the wrong audience.
        let user = JwtKeys::new(SECRET, Duration::from_secs(900))
            .issue(Uuid::now_v7(), Role::User, 0)
            .unwrap();
        assert!(keys.verify(&user).is_none());
        assert!(keys.verify("not.a.jwt").is_none());
        let none = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJzdWIiOiJ4In0.";
        assert!(keys.verify(none).is_none());
    }

    fn base64url(bytes: &[u8]) -> String {
        use base64::Engine as _;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }
}
