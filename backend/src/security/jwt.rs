//! HS256 access tokens with `iss`, `aud`, `exp`, `nbf`, `iat` and `jti` claims,
//! plus the account's role and session epoch.

use std::time::Duration;

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::user::Role;

const ISSUER: &str = "nexc-engine";
const AUDIENCE: &str = "nexc-api";

/// Claims carried by an access token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Claims {
    /// User id.
    pub sub: Uuid,
    pub role: Role,
    pub iss: String,
    pub aud: String,
    pub exp: i64,
    pub nbf: i64,
    pub iat: i64,
    pub jti: Uuid,
    /// The account's session epoch when the token was issued (see `security::gate`).
    #[serde(default)]
    pub epoch: i32,
}

/// Signing / verification keys and the access token lifetime.
pub struct JwtKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
    validation: Validation,
    ttl: Duration,
}

impl std::fmt::Debug for JwtKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtKeys")
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

impl JwtKeys {
    /// Keys derived from `secret` (≥ 32 bytes, enforced by config).
    pub fn new(secret: &[u8], ttl: Duration) -> Self {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[ISSUER]);
        validation.set_audience(&[AUDIENCE]);
        validation.set_required_spec_claims(&["exp", "nbf", "iss", "aud", "sub"]);
        validation.validate_nbf = true;
        validation.leeway = 5;
        JwtKeys {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
            validation,
            ttl,
        }
    }

    /// Access token lifetime in seconds.
    pub fn ttl_secs(&self) -> i64 {
        self.ttl.as_secs() as i64
    }

    /// Issues an access token for `user_id` at its session `epoch`.
    pub fn issue(&self, user_id: Uuid, role: Role, epoch: i32) -> anyhow::Result<String> {
        let now = chrono::Utc::now().timestamp();
        let claims = Claims {
            sub: user_id,
            role,
            iss: ISSUER.into(),
            aud: AUDIENCE.into(),
            exp: now + self.ttl_secs(),
            nbf: now,
            iat: now,
            jti: Uuid::now_v7(),
            epoch,
        };
        Ok(encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &self.encoding,
        )?)
    }

    /// Verifies signature, algorithm, issuer, audience and time claims.
    pub fn verify(&self, token: &str) -> Option<Claims> {
        decode::<Claims>(token, &self.decoding, &self.validation)
            .ok()
            .map(|d| d.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";

    #[test]
    fn issues_and_verifies() {
        let keys = JwtKeys::new(SECRET, Duration::from_secs(900));
        let id = Uuid::now_v7();
        let token = keys.issue(id, Role::Admin, 3).unwrap();
        let claims = keys.verify(&token).unwrap();
        assert_eq!(
            (claims.sub, claims.role, claims.epoch),
            (id, Role::Admin, 3)
        );
        assert_eq!(claims.exp - claims.iat, 900);
    }

    #[test]
    fn rejects_foreign_or_tampered_tokens() {
        let keys = JwtKeys::new(SECRET, Duration::from_secs(900));
        let other = JwtKeys::new(
            b"ffffffffffffffffffffffffffffffff",
            Duration::from_secs(900),
        );
        let token = other.issue(Uuid::now_v7(), Role::User, 0).unwrap();
        assert!(keys.verify(&token).is_none());
        let mut tampered = keys.issue(Uuid::now_v7(), Role::User, 0).unwrap();
        tampered.push('x');
        assert!(keys.verify(&tampered).is_none());
        assert!(keys.verify("not.a.jwt").is_none());
        // `alg: none` tokens are rejected.
        let none = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJzdWIiOiJ4In0.";
        assert!(keys.verify(none).is_none());
    }

    #[test]
    fn rejects_expired_tokens() {
        let keys = JwtKeys::new(SECRET, Duration::from_secs(900));
        let now = chrono::Utc::now().timestamp();
        let claims = Claims {
            sub: Uuid::now_v7(),
            role: Role::User,
            iss: ISSUER.into(),
            aud: AUDIENCE.into(),
            exp: now - 60,
            nbf: now - 120,
            iat: now - 120,
            jti: Uuid::now_v7(),
            epoch: 0,
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(SECRET),
        )
        .unwrap();
        assert!(keys.verify(&token).is_none());
    }
}
