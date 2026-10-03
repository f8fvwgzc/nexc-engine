//! Argon2id password hashing with OWASP-recommended parameters
//! (19 MiB memory, 2 iterations, 1 lane).

use std::sync::OnceLock;

use argon2::{Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version};

use super::random::random_token;

fn hasher() -> Argon2<'static> {
    let params = Params::new(19 * 1024, 2, 1, None).expect("static argon2 params are valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Hashes `password` into a PHC string with a random salt.
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    hasher()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("password hashing failed: {e}"))
}

/// Verifies `password` against a PHC hash. Malformed hashes never verify.
pub fn verify_password(password: &str, phc: &str) -> bool {
    hasher().verify_password(password.as_bytes(), phc).is_ok()
}

/// Spends the same time as a real verification. Called when the user does
/// not exist so that login timing does not reveal registered e-mails.
pub fn verify_dummy(password: &str) {
    static DUMMY: OnceLock<String> = OnceLock::new();
    // A random password nobody knows, hashed once with the real parameters.
    let phc = DUMMY.get_or_init(|| hash_password(&random_token()).unwrap_or_default());
    let _ = verify_password(password, phc);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_verifies() {
        let phc = hash_password("correct horse battery staple").unwrap();
        assert!(phc.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert!(verify_password("correct horse battery staple", &phc));
        assert!(!verify_password("wrong password!!", &phc));
        assert!(!verify_password("x", "not a phc string"));
        verify_dummy("anything");
    }
}
