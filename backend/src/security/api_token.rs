//! API tokens of external agents (the Agent Firewall API): random, shown
//! once, kept as a SHA-256 hash. `nexc_agt_` followed by 32 random bytes in
//! base64url, so a token found in a log is recognisable and revocable.

use base64::Engine as _;

use super::random::random_bytes;

/// Every token starts with this.
pub const PREFIX: &str = "nexc_agt_";
/// Characters of the token shown in the console to tell tokens apart.
pub const SHOWN: usize = PREFIX.len() + 6;

/// A fresh token.
pub fn generate() -> String {
    let bytes = random_bytes::<32>();
    format!(
        "{PREFIX}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

/// What is stored of a token.
pub fn hash(token: &str) -> [u8; 32] {
    crate::kernel::sha256(token.as_bytes())
}

/// The leading characters shown in the console.
pub fn prefix_of(token: &str) -> String {
    token.chars().take(SHOWN).collect()
}

/// Whether `presented` has the shape of one of our tokens (before the lookup).
pub fn looks_like_token(presented: &str) -> bool {
    presented.len() > PREFIX.len()
        && presented.len() <= 128
        && presented.starts_with(PREFIX)
        && presented
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_prefixed_and_hashed() {
        let a = generate();
        let b = generate();
        assert_ne!(a, b);
        assert!(a.starts_with(PREFIX) && looks_like_token(&a));
        assert_eq!(prefix_of(&a).len(), SHOWN);
        assert_ne!(hash(&a), hash(&b));
        assert!(!looks_like_token("nexc_agt_"));
        assert!(!looks_like_token("Bearer x"));
        assert!(!looks_like_token(&format!("{PREFIX}a b")));
    }
}
