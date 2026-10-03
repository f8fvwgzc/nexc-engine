//! Users, roles and credential rules.

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::string_enum;
use super::validation::{FieldErrors, check_text};

string_enum!(
    /// Authorization role of a user.
    Role {
        Admin => "admin",
        User => "user",
    }
);

/// A registered user (never includes credentials).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub role: Role,
    pub created_at: DateTime<Utc>,
}

/// Returned by register / login / refresh. The refresh token travels only in
/// the `nexc_refresh` HttpOnly cookie.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AuthResponse {
    pub user: User,
    pub access_token: String,
    /// Access token lifetime in seconds.
    pub expires_in: i64,
}

/// Minimum password length (OWASP ASVS 2.1.1).
pub const PASSWORD_MIN: usize = 12;
/// Maximum password length; bounds Argon2 work for hostile inputs.
pub const PASSWORD_MAX: usize = 128;
/// Maximum e-mail length (RFC 5321).
pub const EMAIL_MAX: usize = 254;
/// Maximum display name length.
pub const NAME_MAX: usize = 100;
/// Failed logins after which the account is temporarily locked.
pub const LOCKOUT_THRESHOLD: i32 = 5;

/// Canonical form of an e-mail address (trimmed, lowercased).
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// Validates an already normalised e-mail address.
pub fn check_email(errors: &mut FieldErrors, email: &str) {
    let valid = email.len() <= EMAIL_MAX
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !email.chars().any(|c| c.is_whitespace() || c.is_control())
                && email.matches('@').count() == 1
        });
    if !valid {
        errors.add("email", "invalid email");
    }
}

/// Applies the password policy (length bounds, not all one character).
pub fn check_password(errors: &mut FieldErrors, password: &str) {
    let len = password.chars().count();
    if len < PASSWORD_MIN {
        errors.add(
            "password",
            format!("must be at least {PASSWORD_MIN} characters"),
        );
    } else if len > PASSWORD_MAX {
        errors.add(
            "password",
            format!("must be at most {PASSWORD_MAX} characters"),
        );
    } else if password.chars().all(|c| password.starts_with(c)) {
        errors.add("password", "must not repeat a single character");
    }
}

/// Validates a display name.
pub fn check_name(errors: &mut FieldErrors, name: &str) {
    check_text(errors, "name", name, NAME_MAX);
}

/// How long an account stays locked after `failed` consecutive failures:
/// none below the threshold, then 1, 2, 4 … minutes capped at one hour.
pub fn lockout_duration(failed: i32) -> Option<chrono::Duration> {
    if failed < LOCKOUT_THRESHOLD {
        return None;
    }
    let exp = (failed - LOCKOUT_THRESHOLD).min(6) as u32;
    Some(chrono::Duration::minutes(i64::from(2u32.pow(exp)).min(60)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn errors_for(f: impl Fn(&mut FieldErrors)) -> bool {
        let mut e = FieldErrors::default();
        f(&mut e);
        !e.is_empty()
    }

    #[test]
    fn email_rules() {
        assert!(!errors_for(|e| check_email(e, "ada@example.com")));
        for bad in [
            "",
            "ada",
            "ada@",
            "@example.com",
            "a b@example.com",
            "a@b@c.io",
            "a@io.",
        ] {
            assert!(errors_for(|e| check_email(e, bad)), "{bad}");
        }
        assert_eq!(normalize_email("  Ada@Example.COM "), "ada@example.com");
    }

    #[test]
    fn password_rules() {
        assert!(errors_for(|e| check_password(e, "short")));
        assert!(errors_for(|e| check_password(e, &"a".repeat(20))));
        assert!(errors_for(|e| check_password(e, &"ab".repeat(70))));
        assert!(!errors_for(|e| check_password(e, "correct horse battery")));
    }

    #[test]
    fn lockout_backs_off() {
        assert_eq!(lockout_duration(4), None);
        assert_eq!(lockout_duration(5), Some(chrono::Duration::minutes(1)));
        assert_eq!(lockout_duration(7), Some(chrono::Duration::minutes(4)));
        assert_eq!(lockout_duration(50), Some(chrono::Duration::minutes(60)));
    }
}
