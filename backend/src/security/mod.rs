//! Authentication and cryptography: password hashing, JWTs, refresh-token
//! sessions, single sign-on (OpenID Connect), the API-key secret box, agent
//! API tokens, the firewall's signatures and secure randomness.
#![forbid(unsafe_code)]

pub mod agent_token;
pub mod api_token;
pub mod gate;
pub mod jwt;
pub mod oidc;
pub mod password;
pub mod random;
pub mod secret_box;
pub mod session;
pub mod signing;
pub mod totp;
