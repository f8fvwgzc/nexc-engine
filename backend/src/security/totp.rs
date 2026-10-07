//! Time-based one-time passwords (RFC 6238 over RFC 4226): six digits from
//! HMAC-SHA1 of the 30-second time step, which is what authenticator apps
//! compute. HMAC and base32 are written out here; they are a few lines each
//! and that keeps the dependency list as it was.

use sha1::{Digest, Sha1};

use super::random::random_bytes;

/// Seconds a code is valid for.
pub const PERIOD: i64 = 30;
/// Digits of a code.
pub const DIGITS: usize = 6;
const BLOCK: usize = 64;

/// A new secret: 160 bits, the size RFC 4226 recommends.
pub fn generate_secret() -> [u8; 20] {
    random_bytes::<20>()
}

fn hmac_sha1(key: &[u8], message: &[u8]) -> [u8; 20] {
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        block[..20].copy_from_slice(&Sha1::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let inner = Sha1::new()
        .chain_update(block.map(|b| b ^ 0x36))
        .chain_update(message)
        .finalize();
    Sha1::new()
        .chain_update(block.map(|b| b ^ 0x5c))
        .chain_update(inner)
        .finalize()
        .into()
}

/// The code of one time step.
pub fn code(secret: &[u8], step: i64) -> String {
    let mac = hmac_sha1(secret, &step.to_be_bytes());
    let at = usize::from(mac[19] & 0x0f);
    let value = u32::from_be_bytes([mac[at] & 0x7f, mac[at + 1], mac[at + 2], mac[at + 3]]);
    format!("{:0width$}", value % 1_000_000, width = DIGITS)
}

/// The time step of `given` if it is the code of now, of the step before or
/// of the step after (clocks drift), and later than `after`, the step of the
/// last code that was accepted. Spaces in what was typed are ignored.
pub fn verify(secret: &[u8], given: &str, now: i64, after: Option<i64>) -> Option<i64> {
    let given: String = given.chars().filter(|c| !c.is_whitespace()).collect();
    if given.len() != DIGITS || !given.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let current = now.div_euclid(PERIOD);
    (current - 1..=current + 1)
        .filter(|step| after.is_none_or(|last| *step > last))
        .find(|step| {
            super::random::constant_time_eq(code(secret, *step).as_bytes(), given.as_bytes())
        })
}

/// RFC 4648 base32 without padding: how a secret is typed into an app.
pub fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::new();
    let (mut buffer, mut bits) = (0u32, 0u32);
    for byte in bytes {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(ALPHABET[((buffer >> bits) & 31) as usize]));
        }
    }
    if bits > 0 {
        out.push(char::from(ALPHABET[((buffer << (5 - bits)) & 31) as usize]));
    }
    out
}

/// The `otpauth://` address an authenticator app adds an account from.
pub fn uri(issuer: &str, account: &str, secret: &[u8]) -> String {
    let escape = |text: &str| -> String {
        text.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' => {
                    char::from(b).to_string()
                }
                other => format!("%{other:02X}"),
            })
            .collect()
    };
    format!(
        "otpauth://totp/{}:{}?secret={}&issuer={}&algorithm=SHA1&digits={DIGITS}&period={PERIOD}",
        escape(issuer),
        escape(account),
        base32(secret),
        escape(issuer)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"12345678901234567890";

    #[test]
    fn matches_the_rfc_6238_vectors() {
        // The RFC lists eight digits; six are their last six.
        for (time, expected) in [
            (59, "287082"),
            (1_111_111_109, "081804"),
            (1_234_567_890, "005924"),
            (2_000_000_000, "279037"),
        ] {
            assert_eq!(code(SECRET, time / PERIOD), expected, "at {time}");
        }
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32(SECRET), "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
    }

    #[test]
    fn accepts_a_drifting_clock_and_nothing_twice() {
        let now = 1_234_567_890;
        let step = now / PERIOD;
        assert_eq!(verify(SECRET, "005924", now, None), Some(step));
        assert_eq!(
            verify(SECRET, " 005 924 ", now, None),
            Some(step),
            "typed with spaces"
        );
        assert_eq!(
            verify(SECRET, &code(SECRET, step - 1), now, None),
            Some(step - 1)
        );
        assert_eq!(
            verify(SECRET, &code(SECRET, step + 1), now, None),
            Some(step + 1)
        );
        assert_eq!(
            verify(SECRET, &code(SECRET, step - 2), now, None),
            None,
            "too old"
        );
        assert_eq!(
            verify(SECRET, "005924", now, Some(step)),
            None,
            "used already"
        );
        for wrong in ["000000", "00592", "0059240", "abcdef", ""] {
            assert_eq!(verify(SECRET, wrong, now, None), None, "{wrong:?}");
        }
    }

    #[test]
    fn an_app_can_read_the_address() {
        let uri = uri("Nexc", "ada@example.com", SECRET);
        assert_eq!(
            uri,
            "otpauth://totp/Nexc:ada%40example.com?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ\
             &issuer=Nexc&algorithm=SHA1&digits=6&period=30"
        );
    }
}
