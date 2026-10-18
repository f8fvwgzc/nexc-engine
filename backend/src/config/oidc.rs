//! Single sign-on with an OpenID Connect provider (`NEXC_OIDC_*`).
//!
//! `NEXC_OIDC_ISSUER` turns it on; unset, every other `NEXC_OIDC_*` variable
//! is ignored and only passwords sign in.

use uuid::Uuid;

use super::Secret;
use crate::domain::user;
use crate::domain::validation::FieldErrors;
use crate::domain::workspace::WorkspaceRole;

/// The path the provider sends the browser back to, under the public address.
pub const CALLBACK_PATH: &str = "/api/v1/auth/oidc/callback";
/// Scopes asked for when `NEXC_OIDC_SCOPES` is unset.
pub const DEFAULT_SCOPES: &str = "openid email profile";
/// Longest name the login page shows on its button.
pub const DISPLAY_NAME_MAX: usize = 40;
/// Longest group name a mapping may name.
pub const GROUP_MAX: usize = 256;

/// A workspace named by a group mapping: its id, or its slug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceRef {
    Id(Uuid),
    Slug(String),
}

impl std::fmt::Display for WorkspaceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceRef::Id(id) => write!(f, "{id}"),
            WorkspaceRef::Slug(slug) => f.write_str(slug),
        }
    }
}

/// Members of `group` get `role` in `workspace` (`NEXC_OIDC_WORKSPACE_ROLES`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRole {
    pub group: String,
    pub workspace: WorkspaceRef,
    pub role: WorkspaceRole,
}

/// Everything single sign-on is configured with.
#[derive(Debug, Clone)]
pub struct OidcSettings {
    /// The issuer identifier, exactly as the provider's tokens carry it in `iss`.
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Secret<String>,
    /// Where the provider sends the browser back (`{NEXC_PUBLIC_URL}/api/v1/auth/oidc/callback`).
    pub redirect_url: String,
    /// Always holds `openid`.
    pub scopes: Vec<String>,
    /// What the login page's button says after "Continue with".
    pub display_name: String,
    /// The claim of the ID token (or userinfo) that lists the person's groups.
    pub groups_claim: String,
    /// Members of any of these groups administer the platform.
    pub admin_groups: Vec<String>,
    /// Workspace roles given by group, applied on every sign-in.
    pub workspace_roles: Vec<GroupRole>,
    /// Whether passwords still sign in. When false only the break-glass
    /// administrator does.
    pub allow_local_login: bool,
    /// The one platform administrator who signs in with a password when
    /// local login is off.
    pub break_glass_email: Option<String>,
    /// Refuse an identity whose provider does not vouch for its address.
    pub require_verified_email: bool,
}

impl OidcSettings {
    /// The scopes as the authorization request carries them.
    pub fn scope(&self) -> String {
        self.scopes.join(" ")
    }

    /// Whether `email` (normalised) may sign in with a password while local
    /// login is off. The account must also administer the platform.
    pub fn is_break_glass(&self, email: &str) -> bool {
        self.break_glass_email.as_deref() == Some(email)
    }
}

/// Splits a list on commas, trimming, without empty entries.
fn list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Scopes separated by spaces or commas; `openid` is added when missing.
pub fn parse_scopes(raw: &str) -> Result<Vec<String>, String> {
    let mut scopes: Vec<String> = Vec::new();
    for scope in raw.split([' ', ',']).filter(|s| !s.is_empty()) {
        // RFC 6749 §3.3: scope-token = 1*( %x21 / %x23-5B / %x5D-7E )
        let valid = scope
            .bytes()
            .all(|b| b == 0x21 || (0x23..=0x5b).contains(&b) || (0x5d..=0x7e).contains(&b));
        if !valid {
            return Err(format!("`{scope}` is not a valid scope"));
        }
        if !scopes.iter().any(|s| s == scope) {
            scopes.push(scope.to_owned());
        }
    }
    if !scopes.iter().any(|s| s == "openid") {
        scopes.insert(0, "openid".to_owned());
    }
    Ok(scopes)
}

/// One workspace reference: a UUID, or a workspace slug.
fn parse_workspace(raw: &str) -> Result<WorkspaceRef, String> {
    if let Ok(id) = raw.parse::<Uuid>() {
        return Ok(WorkspaceRef::Id(id));
    }
    let slug_ok = raw.len() >= 2
        && raw.len() <= crate::domain::workspace::SLUG_MAX
        && raw
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !raw.starts_with('-');
    if slug_ok {
        Ok(WorkspaceRef::Slug(raw.to_owned()))
    } else {
        Err(format!(
            "`{raw}` is neither a workspace id nor a workspace slug"
        ))
    }
}

/// `group=workspace:role` entries separated by commas. The group ends at the
/// last `=`, the workspace at the last `:`.
pub fn parse_workspace_roles(raw: &str) -> Result<Vec<GroupRole>, String> {
    let mut out = Vec::new();
    for entry in list(raw) {
        let Some((group, target)) = entry.rsplit_once('=') else {
            return Err(format!("`{entry}` is not `group=workspace:role`"));
        };
        let Some((workspace, role)) = target.rsplit_once(':') else {
            return Err(format!("`{entry}` is not `group=workspace:role`"));
        };
        let group = group.trim();
        if group.is_empty() || group.chars().count() > GROUP_MAX {
            return Err(format!(
                "`{entry}`: the group must be 1-{GROUP_MAX} characters"
            ));
        }
        let role: WorkspaceRole = role
            .trim()
            .parse()
            .map_err(|_| format!("`{entry}`: the role must be owner, admin, member or guest"))?;
        out.push(GroupRole {
            group: group.to_owned(),
            workspace: parse_workspace(workspace.trim())?,
            role,
        });
    }
    Ok(out)
}

/// An `http(s)` URL without credentials or a fragment; `https` when `secure`.
fn check_url(name: &str, raw: &str, secure: bool, errors: &mut Vec<String>) {
    match reqwest::Url::parse(raw) {
        Ok(url) => {
            let scheme_ok = url.scheme() == "https" || (!secure && url.scheme() == "http");
            if !scheme_ok {
                errors.push(if secure {
                    format!("{name} must be an https:// URL in production")
                } else {
                    format!("{name} must be an http:// or https:// URL")
                });
            } else if url.host_str().is_none_or(str::is_empty)
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
            {
                errors.push(format!(
                    "{name} must name a host, and carry no credentials and no fragment"
                ));
            }
        }
        Err(_) => errors.push(format!("{name}: `{raw}` is not a URL")),
    }
}

/// Reads the `NEXC_OIDC_*` variables through `lookup`. `None` when
/// `NEXC_OIDC_ISSUER` is unset; problems are appended to `errors`.
pub fn read(
    lookup: &dyn Fn(&str) -> Option<String>,
    production: bool,
    public_url: &str,
    errors: &mut Vec<String>,
) -> Option<OidcSettings> {
    let get = |key: &str| {
        lookup(key)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let issuer = get("NEXC_OIDC_ISSUER")?;
    check_url("NEXC_OIDC_ISSUER", &issuer, production, errors);
    if issuer.contains('?') {
        errors.push("NEXC_OIDC_ISSUER must not carry a query".into());
    }
    let client_id = get("NEXC_OIDC_CLIENT_ID").unwrap_or_else(|| {
        errors.push("NEXC_OIDC_CLIENT_ID is required with NEXC_OIDC_ISSUER".into());
        String::new()
    });
    let client_secret = get("NEXC_OIDC_CLIENT_SECRET").unwrap_or_else(|| {
        errors.push("NEXC_OIDC_CLIENT_SECRET is required with NEXC_OIDC_ISSUER".into());
        String::new()
    });
    let redirect_url = get("NEXC_OIDC_REDIRECT_URL")
        .unwrap_or_else(|| format!("{}{CALLBACK_PATH}", public_url.trim_end_matches('/')));
    check_url("NEXC_OIDC_REDIRECT_URL", &redirect_url, production, errors);
    let scopes = parse_scopes(&get("NEXC_OIDC_SCOPES").unwrap_or_else(|| DEFAULT_SCOPES.into()))
        .unwrap_or_else(|err| {
            errors.push(format!("NEXC_OIDC_SCOPES: {err}"));
            vec!["openid".to_owned()]
        });
    let display_name = get("NEXC_OIDC_DISPLAY_NAME").unwrap_or_else(|| "SSO".into());
    if display_name.chars().count() > DISPLAY_NAME_MAX || display_name.chars().any(char::is_control)
    {
        errors.push(format!(
            "NEXC_OIDC_DISPLAY_NAME must be at most {DISPLAY_NAME_MAX} printable characters"
        ));
    }
    let groups_claim = get("NEXC_OIDC_GROUPS_CLAIM").unwrap_or_else(|| "groups".into());
    if groups_claim.len() > 100
        || groups_claim
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        errors
            .push("NEXC_OIDC_GROUPS_CLAIM must be one claim name of at most 100 characters".into());
    }
    let admin_groups = list(&get("NEXC_OIDC_ADMIN_GROUPS").unwrap_or_default());
    if admin_groups.iter().any(|g| g.chars().count() > GROUP_MAX) {
        errors.push(format!(
            "NEXC_OIDC_ADMIN_GROUPS: a group is at most {GROUP_MAX} characters"
        ));
    }
    let workspace_roles =
        parse_workspace_roles(&get("NEXC_OIDC_WORKSPACE_ROLES").unwrap_or_default())
            .unwrap_or_else(|err| {
                errors.push(format!("NEXC_OIDC_WORKSPACE_ROLES: {err}"));
                Vec::new()
            });
    let mut flag = |key: &str, default: bool| match get(key) {
        None => default,
        Some(raw) => raw.parse().unwrap_or_else(|_| {
            errors.push(format!("{key}: cannot parse `{raw}` (true or false)"));
            default
        }),
    };
    let allow_local_login = flag("NEXC_OIDC_ALLOW_LOCAL_LOGIN", true);
    let require_verified_email = flag("NEXC_OIDC_REQUIRE_VERIFIED_EMAIL", true);
    let break_glass_email = get("NEXC_OIDC_BREAK_GLASS_EMAIL").map(|e| user::normalize_email(&e));
    if let Some(email) = &break_glass_email {
        let mut fe = FieldErrors::default();
        user::check_email(&mut fe, email);
        if !fe.is_empty() {
            errors.push("NEXC_OIDC_BREAK_GLASS_EMAIL is not an e-mail address".into());
        }
    }
    Some(OidcSettings {
        issuer,
        client_id,
        client_secret: Secret::new(client_secret),
        redirect_url,
        scopes,
        display_name,
        groups_claim,
        admin_groups,
        workspace_roles,
        allow_local_login,
        break_glass_email,
        require_verified_email,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn load(vars: &[(&str, &str)], production: bool) -> (Option<OidcSettings>, Vec<String>) {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        let mut errors = Vec::new();
        let lookup = |k: &str| map.get(k).cloned();
        let settings = read(
            &lookup,
            production,
            "https://nexc.example.com/",
            &mut errors,
        );
        (settings, errors)
    }

    const BASE: &[(&str, &str)] = &[
        ("NEXC_OIDC_ISSUER", "https://idp.example.com/realms/acme"),
        ("NEXC_OIDC_CLIENT_ID", "nexc"),
        ("NEXC_OIDC_CLIENT_SECRET", "s3cret-from-the-provider"),
    ];

    #[test]
    fn off_without_an_issuer() {
        let (settings, errors) = load(&[("NEXC_OIDC_CLIENT_ID", "nexc")], true);
        assert!(settings.is_none() && errors.is_empty());
    }

    #[test]
    fn defaults() {
        let (settings, errors) = load(BASE, true);
        assert!(errors.is_empty(), "{errors:?}");
        let s = settings.unwrap();
        assert_eq!(
            s.redirect_url,
            "https://nexc.example.com/api/v1/auth/oidc/callback"
        );
        assert_eq!(s.scope(), "openid email profile");
        assert_eq!(
            (s.display_name.as_str(), s.groups_claim.as_str()),
            ("SSO", "groups")
        );
        assert!(s.allow_local_login && s.require_verified_email);
        assert!(s.admin_groups.is_empty() && s.workspace_roles.is_empty());
        assert!(
            !format!("{s:?}").contains("s3cret"),
            "the secret is redacted"
        );
    }

    #[test]
    fn reads_every_setting() {
        let mut vars = BASE.to_vec();
        vars.extend([
            ("NEXC_OIDC_SCOPES", "email,groups"),
            ("NEXC_OIDC_DISPLAY_NAME", "Acme SSO"),
            ("NEXC_OIDC_GROUPS_CLAIM", "roles"),
            ("NEXC_OIDC_ADMIN_GROUPS", " nexc-admins , ops "),
            (
                "NEXC_OIDC_WORKSPACE_ROLES",
                "eng=acme:member, leads=a=b=0190f3a2-7b1c-7d2e-8f00-0123456789ab:admin",
            ),
            ("NEXC_OIDC_ALLOW_LOCAL_LOGIN", "false"),
            ("NEXC_OIDC_BREAK_GLASS_EMAIL", " Root@Example.com "),
            ("NEXC_OIDC_REQUIRE_VERIFIED_EMAIL", "false"),
            ("NEXC_OIDC_REDIRECT_URL", "https://sso.example.com/cb"),
        ]);
        let (settings, errors) = load(&vars, true);
        assert!(errors.is_empty(), "{errors:?}");
        let s = settings.unwrap();
        assert_eq!(s.scope(), "openid email groups");
        assert_eq!(s.display_name, "Acme SSO");
        assert_eq!(s.groups_claim, "roles");
        assert_eq!(s.admin_groups, ["nexc-admins", "ops"]);
        assert_eq!(
            s.workspace_roles,
            [
                GroupRole {
                    group: "eng".into(),
                    workspace: WorkspaceRef::Slug("acme".into()),
                    role: WorkspaceRole::Member,
                },
                GroupRole {
                    group: "leads=a=b".into(),
                    workspace: WorkspaceRef::Id(
                        "0190f3a2-7b1c-7d2e-8f00-0123456789ab".parse().unwrap()
                    ),
                    role: WorkspaceRole::Admin,
                },
            ]
        );
        assert!(!s.allow_local_login && !s.require_verified_email);
        assert!(s.is_break_glass("root@example.com"));
        assert!(!s.is_break_glass("other@example.com"));
        assert_eq!(s.redirect_url, "https://sso.example.com/cb");
    }

    #[test]
    fn reports_every_problem() {
        let (_, errors) = load(
            &[
                ("NEXC_OIDC_ISSUER", "http://idp.example.com"),
                ("NEXC_OIDC_SCOPES", "openid \"bad"),
                ("NEXC_OIDC_DISPLAY_NAME", &"x".repeat(41)),
                ("NEXC_OIDC_WORKSPACE_ROLES", "eng=acme:superuser"),
                ("NEXC_OIDC_ALLOW_LOCAL_LOGIN", "maybe"),
                ("NEXC_OIDC_BREAK_GLASS_EMAIL", "root"),
                ("NEXC_OIDC_GROUPS_CLAIM", "two words"),
            ],
            true,
        );
        // https in production, the client id and secret, the scope, the name, the mapping,
        // the flag, the address and the claim.
        assert_eq!(errors.len(), 9, "{errors:?}");
        let (_, errors) = load(
            &[
                ("NEXC_OIDC_ISSUER", "http://localhost:8081/realms/dev"),
                ("NEXC_OIDC_CLIENT_ID", "nexc"),
                ("NEXC_OIDC_CLIENT_SECRET", "dev"),
            ],
            false,
        );
        assert!(errors.is_empty(), "plain http in development: {errors:?}");
    }

    #[test]
    fn workspace_references() {
        assert_eq!(
            parse_workspace("acme-eu"),
            Ok(WorkspaceRef::Slug("acme-eu".into()))
        );
        for bad in ["Acme", "-acme", "a", "acme_eu", "../x"] {
            assert!(parse_workspace(bad).is_err(), "{bad}");
        }
        assert!(parse_workspace_roles("eng").is_err());
        assert!(parse_workspace_roles("eng=acme").is_err());
        assert!(parse_workspace_roles("=acme:member").is_err());
        // A comma ends an entry, so a group holding one is split and refused, not misread.
        assert!(parse_workspace_roles("cn=leads,ou=groups=acme:admin").is_err());
        assert_eq!(parse_workspace_roles(" , ").unwrap(), []);
    }
}
