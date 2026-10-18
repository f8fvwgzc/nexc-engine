//! Single sign-on: the ways to sign in the login page offers, the identities
//! an account is linked to, why a sign-in through the identity provider was
//! refused, and where the browser may be sent once it succeeded.

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;

/// Where the browser goes after signing in when it named nowhere (or
/// somewhere it may not go).
pub const DEFAULT_RETURN_TO: &str = "/app";
/// Where a sign-in started to link an identity from the account page
/// (`POST /auth/oidc/link`) comes back to, signed in or refused.
pub const LINK_RETURN_TO: &str = "/app/settings/profile";
/// Longest path the browser may ask to come back to.
pub const RETURN_TO_MAX: usize = 512;

/// A single sign-on provider the login page offers.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SsoProvider {
    /// `oidc`.
    pub id: String,
    /// What the button says after "Continue with".
    pub display_name: String,
    /// Where the button sends the browser (`GET`, a navigation), with
    /// `?return_to=<path of this site>`.
    pub login_url: String,
}

/// The ways to sign in to this installation.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LoginProviders {
    /// E-mail and password (`POST /auth/login`). `false` when only single
    /// sign-on is allowed: the login page hides its form, which the
    /// break-glass administrator opens at `/login?break_glass=1`.
    pub password: bool,
    /// Whether `POST /auth/register` creates accounts.
    pub signup: bool,
    /// Single sign-on providers; empty while single sign-on is off.
    pub sso: Vec<SsoProvider>,
}

/// An identity of a single sign-on provider that signs in to the account.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LinkedIdentity {
    /// The provider's name on the login page (its issuer when it is no longer the one configured).
    pub provider: String,
    pub issuer: String,
    /// Whether it is an identity of the provider configured now (the one the login page offers).
    pub current: bool,
    /// The address the provider gave at the last sign-in.
    pub email: String,
    pub linked_at: DateTime<Utc>,
    #[schema(required = true)]
    pub last_login_at: Option<DateTime<Utc>>,
}

/// An identity linked to an account, as a platform administrator sees it
/// (`GET /admin/users/{uid}/identities`).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AccountIdentity {
    pub id: uuid::Uuid,
    /// The provider's name on the login page (its issuer when it is no longer the one configured).
    pub provider: String,
    pub issuer: String,
    /// Who the person is at the provider: what the provider's ID tokens carry in `sub`.
    pub subject: String,
    /// The address the provider gave at the last sign-in.
    pub email: String,
    pub linked_at: DateTime<Utc>,
    #[schema(required = true)]
    pub last_login_at: Option<DateTime<Utc>>,
}

/// `POST /auth/oidc/link`: where to send the browser to link the provider's
/// identity to the signed-in account.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LinkStart {
    /// The provider's authorization endpoint, with this sign-in's `state`,
    /// `nonce` and PKCE challenge. The answer also sets the `nexc_sso` cookie.
    pub authorization_url: String,
}

/// How an account signs in.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SignInMethods {
    /// Whether the account has a password; one single sign-on created has none.
    pub password: bool,
    /// The provider identities linked to the account, oldest first.
    pub identities: Vec<LinkedIdentity>,
}

/// Why a sign-in through the provider did not happen. The callback sends the
/// browser to `/login?sso_error=<code>`; the code is all the address carries,
/// the details go to the server's log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The provider could not be reached or answered something unusable.
    Unavailable,
    /// The person (or the provider) declined at the provider.
    Denied,
    /// The sign-in was unknown, already used, or took longer than ten minutes.
    Expired,
    /// What came back does not verify: state, signature, issuer, audience, time, nonce.
    Invalid,
    /// The provider gave no usable e-mail address.
    NoEmail,
    /// The provider does not vouch for the address, which this installation requires,
    /// or which linking it to an existing account needs.
    UnverifiedEmail,
    /// An account has this address and is linked to another identity of the provider.
    EmailTaken,
    /// An account has this address but is not linked by it: it administers the platform, is the
    /// break-glass account, or has two-factor sign-in. Its holder links the identity from the
    /// account page (`POST /auth/oidc/link`).
    LinkRequired,
    /// Linking from the account page: the identity is another account's, or the account has
    /// another identity of the provider.
    IdentityTaken,
    /// The account is suspended.
    Suspended,
    /// Something failed on this side; the log says what.
    Failed,
}

impl Refusal {
    /// Every code, as the login page knows them.
    pub const ALL: [Refusal; 11] = [
        Refusal::Unavailable,
        Refusal::Denied,
        Refusal::Expired,
        Refusal::Invalid,
        Refusal::NoEmail,
        Refusal::UnverifiedEmail,
        Refusal::EmailTaken,
        Refusal::LinkRequired,
        Refusal::IdentityTaken,
        Refusal::Suspended,
        Refusal::Failed,
    ];

    /// The code in `/login?sso_error=`.
    pub fn code(self) -> &'static str {
        match self {
            Refusal::Unavailable => "unavailable",
            Refusal::Denied => "denied",
            Refusal::Expired => "expired",
            Refusal::Invalid => "invalid",
            Refusal::NoEmail => "no_email",
            Refusal::UnverifiedEmail => "unverified_email",
            Refusal::EmailTaken => "email_taken",
            Refusal::LinkRequired => "link_required",
            Refusal::IdentityTaken => "identity_taken",
            Refusal::Suspended => "suspended",
            Refusal::Failed => "failed",
        }
    }

    /// Where the browser is sent: the login page, or the account page when
    /// the sign-in was started there to link an identity.
    pub fn location(self, linking: bool) -> String {
        let page = if linking { LINK_RETURN_TO } else { "/login" };
        format!("{page}?sso_error={}", self.code())
    }
}

/// The path to send the browser to once signed in: `requested` when it is a
/// path of this site, else [`DEFAULT_RETURN_TO`]. Nothing that a browser
/// could read as another site passes: not `//host`, not `/\host`, not an
/// absolute URL, not one with a space, a control character or anything
/// outside printable ASCII, and not one whose encoded form starts a second
/// slash or backslash.
pub fn safe_return_to(requested: Option<&str>) -> String {
    let Some(path) = requested else {
        return DEFAULT_RETURN_TO.to_owned();
    };
    let lower = path.to_ascii_lowercase();
    let same_origin = path.len() <= RETURN_TO_MAX
        && path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains('\\')
        && path.bytes().all(|b| b.is_ascii_graphic())
        && !lower.starts_with("/%2f")
        && !lower.starts_with("/%5c")
        && !lower.contains("%0d")
        && !lower.contains("%0a")
        && !lower.starts_with("/api/");
    if same_origin {
        path.to_owned()
    } else {
        DEFAULT_RETURN_TO.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_paths_of_this_site_are_returned_to() {
        for good in [
            "/app",
            "/app/graphs/0190?tab=runs#top",
            "/app/platform/users",
        ] {
            assert_eq!(safe_return_to(Some(good)), good);
        }
        for bad in [
            "https://evil.example",
            "//evil.example",
            "///evil.example",
            "/\\evil.example",
            "\\\\evil.example",
            "/%2F%2Fevil.example",
            "/%5cevil.example",
            "javascript:alert(1)",
            "evil.example",
            "/app\r\nSet-Cookie: x=1",
            "/app%0d%0aSet-Cookie:x",
            "/app with space",
            "/app/ünïcode",
            "",
            "/api/v1/auth/logout",
        ] {
            assert_eq!(safe_return_to(Some(bad)), DEFAULT_RETURN_TO, "{bad:?}");
        }
        assert_eq!(safe_return_to(None), DEFAULT_RETURN_TO);
        assert_eq!(
            safe_return_to(Some(&format!("/{}", "a".repeat(600)))),
            DEFAULT_RETURN_TO
        );
    }

    #[test]
    fn codes_are_distinct_and_url_safe() {
        let codes: std::collections::HashSet<&str> =
            Refusal::ALL.iter().map(|r| r.code()).collect();
        assert_eq!(codes.len(), Refusal::ALL.len());
        assert!(
            codes
                .iter()
                .all(|c| c.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
        );
        assert_eq!(Refusal::Expired.location(false), "/login?sso_error=expired");
        assert_eq!(
            Refusal::IdentityTaken.location(true),
            "/app/settings/profile?sso_error=identity_taken"
        );
    }
}
