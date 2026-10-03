//! Cryptographically secure randomness and token helpers.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::TryRng;
use rand::rngs::SysRng;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// `N` bytes from the operating system CSPRNG.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    SysRng
        .try_fill_bytes(&mut buf)
        .expect("operating system RNG is available");
    buf
}

/// A 256-bit random token, base64url encoded (43 characters).
pub fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes::<32>())
}

/// Hex SHA-256 of a token; only this digest is ever stored.
pub fn token_digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Constant-time equality of two byte strings (length may leak, content does not).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.ct_eq(b).into()
}

/// A uniformly distributed value in `[0, upper)` (0 when `upper` is 0); used for jitter.
pub fn random_below(upper: u64) -> u64 {
    if upper == 0 {
        return 0;
    }
    u64::from_le_bytes(random_bytes::<8>()) % upper
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique_and_url_safe() {
        let (a, b) = (random_token(), random_token());
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert!(
            a.bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        );
        assert_eq!(token_digest("abc").len(), 64);
    }

    #[test]
    fn constant_time_compare() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secret!"));
        assert!(random_below(10) < 10);
        assert_eq!(random_below(0), 0);
    }
}
