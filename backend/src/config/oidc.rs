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

/// Members of `group` get `role` in `workspace` (`NEXC_OIDC_WORKSPACE_ROLES`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRole {
    pub group: String,
    /// The workspace's id. Never its slug: a slug goes to whoever creates a
    /// workspace of that name first, once the one that had it is gone.
    pub workspace: Uuid,
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

/// One workspace, by its id. A slug is refused: any signed-in person can
/// create a workspace and so take a slug nobody holds (before the mapped
/// workspace exists, or after it is deleted), and the mapping would hand
/// that workspace the group's members.
fn parse_workspace(raw: &str) -> Result<Uuid, String> {
    raw.parse::<Uuid>().map_err(|_| {
        format!(
            "`{raw}` is not a workspace id: name the workspace by its id (the platform console's \
             Workspaces shows it), never by its slug, which whoever creates a workspace of that \
             name could take"
        )
    })
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

/// What is wrong with `raw` as an `http(s)` URL without credentials or a
/// fragment (`https` when `secure`), said after the setting's name.
fn url_problem(raw: &str, secure: bool) -> Option<String> {
    let Ok(url) = reqwest::Url::parse(raw) else {
        return Some(format!("is not a URL: `{raw}`"));
    };
    let scheme_ok = url.scheme() == "https" || (!secure && url.scheme() == "http");
    if !scheme_ok {
        Some(if secure {
            "must be an https:// URL in production".to_owned()
        } else {
            "must be an http:// or https:// URL".to_owned()
        })
    } else if url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        Some("must name a host, and carry no credentials and no fragment".to_owned())
    } else {
        None
    }
}

fn check_url(name: &str, raw: &str, secure: bool, errors: &mut Vec<String>) {
    if let Some(problem) = url_problem(raw, secure) {
        errors.push(format!("{name} {problem}"));
    }
}

/// The redirect URL: `NEXC_OIDC_REDIRECT_URL`, else derived from the public
/// address. Either way its path must be [`CALLBACK_PATH`], where the
/// browser's binding cookie (`Path=/api/v1/auth/oidc`) is sent; a problem of
/// a derived one is said of `NEXC_PUBLIC_URL`, the setting to change.
fn redirect_url(
    explicit: Option<String>,
    public_url: &str,
    production: bool,
    errors: &mut Vec<String>,
) -> String {
    let derived = explicit.is_none();
    let url =
        explicit.unwrap_or_else(|| format!("{}{CALLBACK_PATH}", public_url.trim_end_matches('/')));
    let on_the_callback = || {
        reqwest::Url::parse(&url).is_ok_and(|u| u.path() == CALLBACK_PATH && u.query().is_none())
    };
    let because = format!(
        "the single sign-on redirect URL is derived from it (`{url}`): set it to the console's \
         public address, or set NEXC_OIDC_REDIRECT_URL"
    );
    if let Some(problem) = url_problem(&url, production) {
        errors.push(if derived {
            format!("NEXC_PUBLIC_URL {problem}; {because}")
        } else {
            format!("NEXC_OIDC_REDIRECT_URL {problem}")
        });
    } else if !on_the_callback() {
        errors.push(if derived {
            format!("NEXC_PUBLIC_URL must be the console's address without a path; {because}")
        } else {
            format!(
                "NEXC_OIDC_REDIRECT_URL must be the console's address followed by {CALLBACK_PATH}, \
                 with no query: the cookie that binds a sign-in to its browser is only sent there"
            )
        });
    }
    url
}

/// What may keep a redirect URL from ever signing anyone in, for `nexc
/// doctor`: another host than the console's (`NEXC_PUBLIC_URL`), whose
/// browsers would not carry the binding cookie there, and, outside
/// development, a host that is this machine.
pub fn redirect_concerns(redirect_url: &str, public_url: &str, development: bool) -> Vec<String> {
    let host = |raw: &str| {
        reqwest::Url::parse(raw)
            .ok()
            .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
    };
    let mut out = Vec::new();
    let (Some(redirect), public) = (host(redirect_url), host(public_url)) else {
        return out;
    };
    if public.as_deref().is_some_and(|public| public != redirect) {
        out.push(format!(
            "the redirect URL's host `{redirect}` is not NEXC_PUBLIC_URL's `{}`: the sign-in only \
             completes when the console is opened at `{redirect}`, the host its cookie is set on",
            public.unwrap_or_default()
        ));
    }
    let local = matches!(redirect.as_str(), "localhost" | "127.0.0.1" | "[::1]")
        || redirect.ends_with(".localhost");
    if local && !development {
        out.push(format!(
            "the redirect URL's host is `{redirect}`, this machine: set NEXC_PUBLIC_URL to the \
             console's public address"
        ));
    }
    out
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
    let redirect_url = redirect_url(
        get("NEXC_OIDC_REDIRECT_URL"),
        public_url,
        production,
        errors,
    );
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
        load_at(vars, production, "https://nexc.example.com/")
    }

    fn load_at(
        vars: &[(&str, &str)],
        production: bool,
        public_url: &str,
    ) -> (Option<OidcSettings>, Vec<String>) {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        let mut errors = Vec::new();
        let lookup = |k: &str| map.get(k).cloned();
        let settings = read(&lookup, production, public_url, &mut errors);
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
                "eng=0190f3a2-7b1c-7d2e-8f00-00000000ac3e:member, leads=a=b=0190f3a2-7b1c-7d2e-8f00-0123456789ab:admin",
            ),
            ("NEXC_OIDC_ALLOW_LOCAL_LOGIN", "false"),
            ("NEXC_OIDC_BREAK_GLASS_EMAIL", " Root@Example.com "),
            ("NEXC_OIDC_REQUIRE_VERIFIED_EMAIL", "false"),
            (
                "NEXC_OIDC_REDIRECT_URL",
                "https://console.example.com/api/v1/auth/oidc/callback",
            ),
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
                    workspace: "0190f3a2-7b1c-7d2e-8f00-00000000ac3e".parse().unwrap(),
                    role: WorkspaceRole::Member,
                },
                GroupRole {
                    group: "leads=a=b".into(),
                    workspace: "0190f3a2-7b1c-7d2e-8f00-0123456789ab".parse().unwrap(),
                    role: WorkspaceRole::Admin,
                },
            ]
        );
        assert!(!s.allow_local_login && !s.require_verified_email);
        assert!(s.is_break_glass("root@example.com"));
        assert!(!s.is_break_glass("other@example.com"));
        assert_eq!(
            s.redirect_url,
            "https://console.example.com/api/v1/auth/oidc/callback"
        );
    }

    #[test]
    fn the_redirect_url_is_the_callback_path() {
        let with = |redirect: &str| {
            let mut vars = BASE.to_vec();
            vars.push(("NEXC_OIDC_REDIRECT_URL", redirect));
            load(&vars, true).1
        };
        // The binding cookie never reaches another path, whatever a proxy would route.
        for elsewhere in [
            "https://nexc.example.com/sso/callback",
            "https://nexc.example.com/api/v1/auth/oidc/callback/",
            "https://nexc.example.com/api/v1/auth/oidc/callback?x=1",
            "https://nexc.example.com/",
        ] {
            let errors = with(elsewhere);
            assert_eq!(errors.len(), 1, "{elsewhere}: {errors:?}");
            assert!(errors[0].starts_with("NEXC_OIDC_REDIRECT_URL must be"));
        }
        assert!(with("http://nexc.example.com/api/v1/auth/oidc/callback")[0].contains("https"));
        // A derived one is NEXC_PUBLIC_URL's to fix, and says so.
        let (_, errors) = load_at(BASE, true, "http://localhost:8080");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].starts_with("NEXC_PUBLIC_URL must be an https:// URL in production")
                && errors[0].contains("http://localhost:8080/api/v1/auth/oidc/callback"),
            "{errors:?}"
        );
        let (_, errors) = load_at(BASE, true, "https://example.com/nexc");
        assert!(
            errors.len() == 1
                && errors[0]
                    .starts_with("NEXC_PUBLIC_URL must be the console's address without a path"),
            "{errors:?}"
        );
        let (settings, errors) = load_at(BASE, false, "http://localhost:5173");
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            settings.unwrap().redirect_url,
            "http://localhost:5173/api/v1/auth/oidc/callback"
        );
    }

    #[test]
    fn doctor_names_a_redirect_url_that_cannot_complete() {
        let callback = |origin: &str| format!("{origin}{CALLBACK_PATH}");
        let console = "https://nexc.example.com";
        assert!(redirect_concerns(&callback(console), console, false).is_empty());
        let other = redirect_concerns(&callback("https://sso.example.com"), console, false);
        assert_eq!(other.len(), 1, "{other:?}");
        assert!(other[0].contains("`sso.example.com`") && other[0].contains("`nexc.example.com`"));
        // Ports do not keep cookies apart: the dev server's proxy on :5173 is the same host.
        let dev = "http://localhost:5173";
        assert!(redirect_concerns(&callback(dev), "http://localhost:8080", true).is_empty());
        let local = redirect_concerns(&callback(dev), dev, false);
        assert!(
            local.len() == 1 && local[0].contains("this machine"),
            "{local:?}"
        );
    }

    #[test]
    fn reports_every_problem() {
        let (_, errors) = load(
            &[
                ("NEXC_OIDC_ISSUER", "http://idp.example.com"),
                ("NEXC_OIDC_SCOPES", "openid \"bad"),
                ("NEXC_OIDC_DISPLAY_NAME", &"x".repeat(41)),
                (
                    "NEXC_OIDC_WORKSPACE_ROLES",
                    "eng=0190f3a2-7b1c-7d2e-8f00-00000000ac3e:superuser",
                ),
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
        let id = "0190f3a2-7b1c-7d2e-8f00-00000000ac3e";
        assert_eq!(parse_workspace(id), Ok(id.parse().unwrap()));
        // A slug could be anybody's: whoever creates a workspace of that name.
        for bad in ["acme", "acme-eu", "Acme", "-acme", "a", "../x", ""] {
            let err = parse_workspace(bad).unwrap_err();
            assert!(err.contains("not a workspace id"), "{bad}: {err}");
        }
        assert!(parse_workspace_roles("eng").is_err());
        assert!(parse_workspace_roles(&format!("eng={id}")).is_err());
        assert!(parse_workspace_roles(&format!("={id}:member")).is_err());
        assert!(parse_workspace_roles("eng=acme:member").is_err());
        // A comma ends an entry, so a group holding one is split and refused, not misread.
        assert!(parse_workspace_roles(&format!("cn=leads,ou=groups={id}:admin")).is_err());
        assert_eq!(parse_workspace_roles(" , ").unwrap(), []);
    }
}
