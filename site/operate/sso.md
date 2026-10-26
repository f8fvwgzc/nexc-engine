# Single sign-on

This page connects nexc-engine to an OpenID Connect identity provider, so that people sign in with the account their organisation already manages, get their platform and workspace roles from its groups, and, if you choose, no longer sign in with a password at all.

::: warning Not yet tested against these providers
Single sign-on is tested end to end against a provider in process (`backend/tests/oidc.rs`): discovery, a rotating key set, the token endpoint with PKCE, userinfo, and every way a token can fail to verify. The provider examples below follow each provider's documentation; they have **not yet been tried against Keycloak, Microsoft Entra ID, Okta or Google**. Try a sign-in on a staging installation before you turn password sign-in off.
:::

## How a sign-in goes

<!--@include: ../../docs/CONTRACT.md#sso-->

## Settings

Single sign-on is off until `NEXC_OIDC_ISSUER` is set. The other variables are read only then.

| Variable                           | Default                                         | What it does                                                                                                                                                                                                                                |
| ---------------------------------- | ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `NEXC_OIDC_ISSUER`                 | — (off)                                         | The issuer identifier, exactly as the provider's tokens carry it in `iss` (a trailing slash matters). `https://` in production. Its `/.well-known/openid-configuration` is read.                                                              |
| `NEXC_OIDC_CLIENT_ID`              | —                                               | The client registered at the provider: a confidential web client with the authorization code flow.                                                                                                                                          |
| `NEXC_OIDC_CLIENT_SECRET`          | —                                               | Its secret. Keep it in the secret store (`.env`, the Kubernetes Secret), never in the ConfigMap.                                                                                                                                            |
| `NEXC_OIDC_REDIRECT_URL`           | `NEXC_PUBLIC_URL` + `/api/v1/auth/oidc/callback` | Where the provider sends the browser back. Register exactly this at the provider. It must be the console's own address followed by `/api/v1/auth/oidc/callback`: the cookie that binds a sign-in to its browser is only sent there, so another path is refused at start-up and another host never completes a sign-in. Leave it unset and set `NEXC_PUBLIC_URL` to the console's public address. |
| `NEXC_OIDC_SCOPES`                 | `openid email profile`                          | Scopes asked for, separated by spaces (quote the value in `.env`) or commas. `openid` is always asked for. Add `groups` where the provider wants a scope for groups.                                                                          |
| `NEXC_OIDC_DISPLAY_NAME`           | `SSO`                                           | The login page's button reads "Continue with" this.                                                                                                                                                                                         |
| `NEXC_OIDC_GROUPS_CLAIM`           | `groups`                                        | The claim listing the person's groups, in the ID token or the userinfo answer: an array of strings, or one string. A dotted path reads a nested claim (`realm_access.roles`).                                                                 |
| `NEXC_OIDC_ADMIN_GROUPS`           | —                                               | Comma separated groups whose members administer the platform.                                                                                                                                                                              |
| `NEXC_OIDC_WORKSPACE_ROLES`        | —                                               | Comma separated `group=workspace:role`: the workspace by its id (open it in the platform console's Workspaces), the role `owner`, `admin`, `member` or `guest`. A slug is refused: whoever creates a workspace of that name, before yours exists or after it is deleted, would get the group's members. A group name with a comma cannot be mapped.                                                                       |
| `NEXC_OIDC_ALLOW_LOCAL_LOGIN`      | `true`                                          | `false` hides the password form and refuses password sign-in and registration, except for the break-glass administrator.                                                                                                                    |
| `NEXC_OIDC_BREAK_GLASS_EMAIL`      | —                                               | The one platform administrator who still signs in with a password when local login is off, at `/login?break_glass=1`.                                                                                                                       |
| `NEXC_OIDC_REQUIRE_VERIFIED_EMAIL` | `true`                                          | Refuse an identity whose provider does not say (`email_verified`) that its address is verified. Linking to an existing account always needs a verified address, whatever this says.                                                         |

Check a configuration before the first start: `nexc doctor` (or `make doctor`) fetches the discovery document and the signing keys, checks that the document is the issuer's own and signs with RS256 or ES256, counts the keys a sign-in would verify with (RS256, and ES256 on P-256; it fails when there is none), prints the redirect URL to register (and warns when its host is not `NEXC_PUBLIC_URL`'s, or is this machine outside development), warns when password sign-in is off without a break-glass administrator, fails when the break-glass address names no account, a suspended one, one without a password or one that is not a platform administrator's (the server logs the same at start-up), and checks that every mapped workspace exists. When the provider cannot be reached it says why: the name that did not resolve, the connection refused, the certificate.

**Trying it locally.** Under `make dev` the console is the Vite dev server on port 5173, which hands `/api` to the backend: set `NEXC_PUBLIC_URL=http://localhost:5173` and register `http://localhost:5173/api/v1/auth/oidc/callback`, so that the provider sends the browser back through the dev server and the sign-in ends on the console.

### Roles from groups

At every sign-in the person's groups are applied:

- **Platform administrators.** A member of one of `NEXC_OIDC_ADMIN_GROUPS` becomes a platform administrator, and stops being one at the first sign-in without the group. A platform role given in the console is never taken back by a sign-in. An account that is the only owner of a workspace other people work in is not made an administrator (a platform administrator works in no workspace); the server logs it. Every role the groups give or take back is in the platform console's Activity, by `single sign-on (<display name>)`, naming the group, as well as in the account's own activity.
- **Workspace roles.** Each workspace named in `NEXC_OIDC_WORKSPACE_ROLES` gets the highest role of the person's groups. The mapping raises a role and never lowers one it did not give. Once no group gives a role any more, the role the mapping gave is taken back: the account goes back to the role it had before, or leaves the workspace. A role someone changed by hand since is left alone, and a workspace never loses its last owner this way. Every change is in the workspace's audit log, by `system`, naming the group.
- **No groups claim.** A sign-in whose ID token and userinfo answer carry no `NEXC_OIDC_GROUPS_CLAIM` says nothing about groups, so it gives and takes back nothing: every role stays as it is, and the server logs a warning naming the claim. Only a claim that is there, even an empty list, gives and takes back. Check the claim name and the provider's mapper when roles never change.

### Password sign-in, and the break-glass administrator

With `NEXC_OIDC_ALLOW_LOCAL_LOGIN=false` the login page shows only the single sign-on button, `POST /auth/login` answers 403 and registration is closed. One platform administrator, `NEXC_OIDC_BREAK_GLASS_EMAIL`, still signs in with a password: open `/login?break_glass=1`. Keep that account's password and second factor somewhere safe; it is how you get in while the provider is down or misconfigured. Bootstrap it with `NEXC_ADMIN_EMAIL` and `NEXC_ADMIN_PASSWORD`, or `nexc user create --admin`.

### What else to know

- **Second factor.** People who sign in through the provider are not asked for the local authenticator code: the provider authenticates them, so enforce a second factor there. That is why an account with a local second factor is only ever linked by its holder (below).
- **Accounts the provider creates** have no password. Their holders see the linked identity on their profile, and no password or two-factor forms (the provider checks both; setting up the local second factor is refused); deleting such an account is done by a platform administrator (Accounts, Erase), since deleting one's own account asks for a password.
- **Who may sign in** is the provider's decision: anyone it lets through gets an account at the first sign-in. Assign the application to the people or groups who should use it.
- **An account someone signed up for is never linked by address.** A sign-up does not prove an address, so whoever signed up with it first would share the account with the person the provider vouches for. The sign-in is refused (`link_required`); the holder signs in with their password and links the provider on their profile page (below). Only an account without a password (the provider created it, or a workspace transfer brought it) is linked at a first sign-in by a verified address, when it has no other identity of the provider. Otherwise the sign-in is refused with a message, never merged.
- **Platform administrators, the break-glass account and accounts with two-factor sign-in are never linked by address.** Anyone the provider vouches for with that address (an administrator of the provider, a mis-scoped tenant, self-service e-mail) would otherwise get the account, without its second factor. The sign-in is refused (`link_required`); the holder signs in with their password and links the provider on their profile page (Single sign-on, *Link*), which asks for the password and then for the provider's sign-in. Link the administrators' accounts that way **before** turning password sign-in off: afterwards only the break-glass administrator signs in with a password. The break-glass account is never linked, nor created by the provider.
- **Unlinking.** In the platform console, Accounts, the account's menu, *Single sign-on identities* lists who the account is at the provider (its subject) and unlinks one. The account is signed out everywhere, and the person's next sign-in through the provider links again by a verified address (an account without a password), or is refused. Use it when the provider re-created someone (a new subject is refused with `email_taken` until the old one is unlinked), linked the wrong person, or must stop signing someone in (then stop the provider vouching for the address too). It is in the platform's Activity.
- **Deleting an account** erases its linked identities; the person's next sign-in through the provider creates a new account.

## Provider examples

Each example assumes the console is at `https://nexc.example.com`, so the redirect URL is `https://nexc.example.com/api/v1/auth/oidc/callback`.

### Keycloak

In the realm, create a client: type *OpenID Connect*, *Client authentication* on, *Standard flow* on, the redirect URL under *Valid redirect URIs*, and *Proof Key for Code Exchange Code Challenge Method* `S256` under *Advanced*. The secret is on the *Credentials* tab. For groups, add a *Group Membership* mapper to the client's dedicated scope: token claim name `groups`, *Full group path* off, *Add to ID token* on.

```bash
NEXC_OIDC_ISSUER=https://keycloak.example.com/realms/acme
NEXC_OIDC_CLIENT_ID=nexc
NEXC_OIDC_CLIENT_SECRET=<the client's secret>
NEXC_OIDC_DISPLAY_NAME=Keycloak
NEXC_OIDC_ADMIN_GROUPS=nexc-admins
NEXC_OIDC_WORKSPACE_ROLES=engineering=<workspace id>:member,engineering-leads=<workspace id>:admin
```

Realm roles instead of groups: `NEXC_OIDC_GROUPS_CLAIM=realm_access.roles`.

### Microsoft Entra ID

Register an application (single tenant) with a *Web* redirect URI, and create a client secret under *Certificates & secrets*. Under *Token configuration*, add the optional claim `email` to the ID token, and a groups claim (*Security groups*, emitted in the ID token as group object IDs, so the mapping names object IDs). A person in more than 200 groups gets an overage claim (`_claim_names`) instead, which is not read: their roles then stay as they are at every sign-in, and the server logs a warning. Limit the claim to *Groups assigned to the application*.

```bash
NEXC_OIDC_ISSUER=https://login.microsoftonline.com/<tenant id>/v2.0
NEXC_OIDC_CLIENT_ID=<application (client) id>
NEXC_OIDC_CLIENT_SECRET=<client secret value>
NEXC_OIDC_DISPLAY_NAME=Microsoft
NEXC_OIDC_ADMIN_GROUPS=<object id of the admins group>
NEXC_OIDC_WORKSPACE_ROLES=<object id of a group>=<workspace id>:member
# Entra ID does not send email_verified: without this, every new person is refused, and existing
# accounts are never linked by address (they need a verified one).
NEXC_OIDC_REQUIRE_VERIFIED_EMAIL=false
```

### Okta

Create an app integration: *OIDC*, *Web Application*, grant type *Authorization Code*, the redirect URL as a *Sign-in redirect URI*; copy the client id and secret. The example uses the `default` custom authorization server, whose issuer ends in `/oauth2/default`. Its tokens carry groups only once the server has a claim for them: under *Security*, *API*, *Authorization Servers*, `default`, *Claims*, add a claim named `groups`, included in the *ID Token*, *Always*, value type *Groups*, with a filter such as *Starts with* `nexc-`.

```bash
NEXC_OIDC_ISSUER=https://acme.okta.com/oauth2/default
NEXC_OIDC_CLIENT_ID=<client id>
NEXC_OIDC_CLIENT_SECRET=<client secret>
NEXC_OIDC_DISPLAY_NAME=Okta
NEXC_OIDC_ADMIN_GROUPS=nexc-admins
NEXC_OIDC_WORKSPACE_ROLES=nexc-engineering=<workspace id>:member
```

With the org authorization server instead, the issuer is `https://acme.okta.com`, and the groups claim is set on the app's *Sign On* tab (*Groups claim*: name `groups`, the same filter); that tab does nothing for a custom authorization server. Either way, a sign-in whose ID token carries no `groups` claim changes no role, and the server's log says so.

### Google

Create an OAuth client id of type *Web application* in the Google Cloud console, with the redirect URL as an *Authorized redirect URI*. Google's ID tokens carry no groups, so the admin groups and the workspace mapping do not apply: give roles in nexc. Google lets **any** Google account sign in unless the OAuth consent screen is *Internal* (Google Workspace): make it internal, or every Google account can create an account here.

```bash
NEXC_OIDC_ISSUER=https://accounts.google.com
NEXC_OIDC_CLIENT_ID=<client id>.apps.googleusercontent.com
NEXC_OIDC_CLIENT_SECRET=<client secret>
NEXC_OIDC_DISPLAY_NAME=Google
```

## Helm chart

The chart's `oidc` block writes the settings into the ConfigMap and reads the client secret from a Secret of your own:

```bash
kubectl -n nexc create secret generic nexc-oidc --from-literal=client-secret='<the client secret>'
```

```yaml
config:
  NEXC_PUBLIC_URL: https://nexc.example.com # the redirect URL is derived from it
oidc:
  enabled: true
  issuer: https://keycloak.example.com/realms/acme
  clientId: nexc
  existingSecret: nexc-oidc
  existingSecretKey: client-secret
  displayName: Keycloak
  adminGroups: [nexc-admins]
  workspaceRoles:
    - { group: engineering, workspace: 0190f3a2-7b1c-7d2e-8f00-00000000ac3e, role: member }
  allowLocalLogin: true
  breakGlassEmail: ""
  requireVerifiedEmail: true
```

Every value is on [Helm chart](/operate/helm).

## When it does not work

The login page says what went wrong in words; the server's log says why, under `single sign-on`: at `warn` for what the provider or the browser sent and for a refused account (with whether an address was given and what `email_verified` said, never the address), at `error` for a failure on this side, and at `info` for a sign-in declined at the provider.

| The login page says                        | Look at                                                                                                                                       |
| ------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------- |
| … could not be reached                     | `nexc doctor`: the issuer, the network path from the backend to the provider, the discovery document.                                          |
| … took too long or was already used        | A sign-in older than ten minutes, or a page reloaded at the callback: a sign-in works once.                                                    |
| … sent back did not check out              | The log names the check: the redirect URL registered at the provider, the client id (`aud`), the clock (`iat`, `exp`), the signing algorithm. "The browser that came back is not the one that left": cookies blocked for the console, the sign-in finished in another browser, or a redirect URL on another host than the console (`nexc doctor` warns). |
| … has not verified your e-mail address     | The provider sends no `email_verified: true`; see Entra ID above.                                                                             |
| … is linked to another … identity          | The provider re-created the person (a new subject) or another identity was linked: unlink the old one in the platform console (Accounts, *Single sign-on identities*). |
| … does not open by itself                   | The account has a password, administers the platform, has two-factor sign-in or is the break-glass account: its holder links the provider from their profile page. |
| That … identity is linked to another account | Linking from the profile page: the identity is another account's, or this account has another of the provider's; unlink one in the platform console. |
