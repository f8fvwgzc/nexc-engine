//! Signing in through the single sign-on provider: which account a verified
//! identity is, linking or creating it at its first sign-in, and the
//! platform and workspace roles the provider's groups give, applied at every
//! sign-in.
//!
//! An identity is its `(issuer, subject)` pair. The first time one signs in
//! it is linked to the account with its e-mail address, but only when the
//! provider says the address is verified and that account has no identity of
//! this provider yet; with no such account one is created. Anything else is
//! refused: an address cannot belong to two accounts.
//!
//! The group mapping only ever takes back what it gave: a workspace role is
//! recorded as the mapping's (with the role it replaced), and a sign-in
//! without the group restores what was there before, unless someone changed
//! the role by hand since. A platform role given in the console is never
//! touched; one the admin groups gave is taken back without them. A sign-in
//! whose provider says nothing about groups (no groups claim in the ID token
//! nor in the userinfo answer) is not a sign-in without them: nothing is
//! known, so no role is given or taken back.

use std::collections::HashMap;
use std::net::IpAddr;

use uuid::Uuid;

use crate::app::AppState;
use crate::config::oidc::OidcSettings;
use crate::domain::AppError;
use crate::domain::account::AccountEventKind;
use crate::domain::audit::AuditAction;
use crate::domain::sso::Refusal;
use crate::domain::user::{self, Role, User};
use crate::domain::validation::FieldErrors;
use crate::domain::workspace::WorkspaceRole;
use crate::engine::account::note;
use crate::repo;
use crate::repo::OrNotFound;
use crate::repo::sso::RoleGrant;
use crate::security::oidc::Identity;

/// How a sign-in through the provider ended.
#[derive(Debug)]
pub enum Resolution {
    /// Signed in as this account (with the roles the groups give applied).
    Account(User),
    Refused(Refusal),
}

/// What looking the identity up came to.
enum Found {
    Account(Uuid),
    Refused(Refusal),
    /// Another sign-in created the account or the link at the same moment.
    Raced,
}

/// Resolves a verified identity to an account and applies the group mapping.
pub async fn sign_in(
    state: &AppState,
    settings: &OidcSettings,
    identity: &Identity,
    ip: IpAddr,
) -> Result<Resolution, AppError> {
    let mut found = find_or_create(state, settings, identity, ip).await?;
    if matches!(found, Found::Raced) {
        found = find_or_create(state, settings, identity, ip).await?;
    }
    let id = match found {
        Found::Account(id) => id,
        Found::Refused(refusal) => return Ok(Resolution::Refused(refusal)),
        Found::Raced => return Ok(Resolution::Refused(Refusal::Failed)),
    };
    let Some(account) = repo::sso::account(&state.db, id).await? else {
        return Ok(Resolution::Refused(Refusal::Suspended));
    };
    if account.suspended {
        let detail = "single sign-on: the account is suspended";
        note(state, id, AccountEventKind::SignInFailed, Some(ip), detail).await;
        return Ok(Resolution::Refused(Refusal::Suspended));
    }
    let groups = identity.profile.groups.as_deref();
    let role = match groups {
        Some(groups) => platform_role(state, settings, &account, groups).await?,
        None => {
            if !settings.admin_groups.is_empty() || !settings.workspace_roles.is_empty() {
                tracing::warn!(user_id = %id, claim = %settings.groups_claim,
                    "single sign-on: neither the ID token nor the userinfo answer carries the groups claim; \
                     the account's roles are left as they are");
            }
            account.user.role
        }
    };
    workspace_roles(state, settings, &account.user, role, groups).await?;
    let user = repo::users::find(&state.db, id)
        .await
        .or_not_found("user")?;
    Ok(Resolution::Account(user))
}

/// The account of a linked identity; else links the account with the
/// identity's verified address; else creates one.
async fn find_or_create(
    state: &AppState,
    settings: &OidcSettings,
    identity: &Identity,
    ip: IpAddr,
) -> Result<Found, AppError> {
    let issuer = settings.issuer.as_str();
    let profile = &identity.profile;
    let given = profile.email.clone().unwrap_or_default();
    if let Some(id) = repo::sso::linked_user(&state.db, issuer, &identity.subject).await? {
        repo::sso::touch(&state.db, issuer, &identity.subject, &given).await?;
        return Ok(Found::Account(id));
    }
    let email = user::normalize_email(&given);
    let mut errors = FieldErrors::default();
    user::check_email(&mut errors, &email);
    if !errors.is_empty() {
        return Ok(Found::Refused(Refusal::NoEmail));
    }
    let verified = profile.email_verified == Some(true);
    if let Some(existing) = repo::users::credentials(&state.db, &email).await? {
        let id = existing.user.id;
        if !verified {
            return Ok(Found::Refused(Refusal::UnverifiedEmail));
        }
        if repo::sso::has_identity_at(&state.db, id, issuer).await? {
            return Ok(Found::Refused(Refusal::EmailTaken));
        }
        if existing.suspended {
            let detail = "single sign-on: the account is suspended";
            note(state, id, AccountEventKind::SignInFailed, Some(ip), detail).await;
            return Ok(Found::Refused(Refusal::Suspended));
        }
        if !repo::sso::link(&state.db, id, issuer, &identity.subject, &given).await? {
            return Ok(Found::Raced);
        }
        let detail = format!("{} ({issuer})", settings.display_name);
        note(state, id, AccountEventKind::SsoLinked, Some(ip), &detail).await;
        tracing::info!(user_id = %id, issuer, "single sign-on identity linked by verified e-mail");
        return Ok(Found::Account(id));
    }
    if settings.require_verified_email && !verified {
        return Ok(Found::Refused(Refusal::UnverifiedEmail));
    }
    provision(state, settings, identity, &email, verified, ip).await
}

/// Creates the account of an identity's first sign-in. A platform
/// administrator from the start when the groups say so. Invitations to the
/// address are accepted only when the provider vouches for it.
async fn provision(
    state: &AppState,
    settings: &OidcSettings,
    identity: &Identity,
    email: &str,
    verified: bool,
    ip: IpAddr,
) -> Result<Found, AppError> {
    let groups = identity.profile.groups.as_deref().unwrap_or_default();
    let admin = in_admin_group(settings, groups);
    let role = if admin { Role::Admin } else { Role::User };
    let name = identity
        .profile
        .name
        .clone()
        .filter(|n| {
            let mut errors = FieldErrors::default();
            user::check_name(&mut errors, n);
            errors.is_empty()
        })
        .unwrap_or_else(|| email.split('@').next().unwrap_or("Member").to_owned());
    let mut tx = state.db.begin().await?;
    // No password: "!" is no hash, so nothing ever matches it.
    let created = match repo::users::create(&mut *tx, email, &name, role, "!").await {
        Ok(created) => created,
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => return Ok(Found::Raced),
        Err(err) => return Err(err.into()),
    };
    if admin {
        sqlx::query("UPDATE users SET admin_by_sso = true WHERE id = $1")
            .bind(created.id)
            .execute(&mut *tx)
            .await?;
    }
    let given = identity.profile.email.as_deref().unwrap_or_default();
    if !repo::sso::link(
        &mut *tx,
        created.id,
        &settings.issuer,
        &identity.subject,
        given,
    )
    .await?
    {
        return Ok(Found::Raced);
    }
    if verified && !admin {
        repo::workspaces::accept_invites(&mut tx, created.id, email).await?;
    }
    tx.commit().await?;
    let detail = format!("{} ({})", settings.display_name, settings.issuer);
    let kind = AccountEventKind::SsoProvisioned;
    note(state, created.id, kind, Some(ip), &detail).await;
    tracing::info!(user_id = %created.id, %role, "account created at its first single sign-on");
    Ok(Found::Account(created.id))
}

fn in_admin_group(settings: &OidcSettings, groups: &[String]) -> bool {
    groups.iter().any(|g| settings.admin_groups.contains(g))
}

/// Gives or takes back the platform role the admin groups decide. Returns
/// the role the account has afterwards.
async fn platform_role(
    state: &AppState,
    settings: &OidcSettings,
    account: &repo::sso::SsoAccount,
    groups: &[String],
) -> Result<Role, AppError> {
    let id = account.user.id;
    let wants = in_admin_group(settings, groups);
    match account.user.role {
        Role::User if wants => {
            let held = repo::platform::sole_owner_of(&state.db, id).await?;
            if !held.is_empty() {
                // A platform administrator works in no workspace: those would lose their owner.
                tracing::warn!(user_id = %id, workspaces = ?held,
                    "single sign-on admin group not applied: the account is the only owner of shared workspaces");
                return Ok(Role::User);
            }
            if let Some(epoch) =
                repo::sso::set_platform_role(&state.db, id, Role::Admin, true).await?
            {
                state.sessions.raise(id, epoch);
                let detail = "user → admin, by a single sign-on admin group";
                note(state, id, AccountEventKind::RoleChanged, None, detail).await;
            }
            Ok(Role::Admin)
        }
        Role::Admin if !wants && account.admin_by_sso => {
            if let Some(epoch) =
                repo::sso::set_platform_role(&state.db, id, Role::User, false).await?
            {
                state.sessions.raise(id, epoch);
                let detail = "admin → user: no single sign-on admin group any more";
                note(state, id, AccountEventKind::RoleChanged, None, detail).await;
            }
            Ok(Role::User)
        }
        role => Ok(role),
    }
}

/// One change the mapping made in a workspace, for its audit log.
struct Change {
    workspace: Uuid,
    action: AuditAction,
    detail: String,
}

/// Applies the workspace role mapping: what the groups give is given, what
/// the mapping gave and the groups no longer give is taken back; with no
/// groups known (`None`) nothing is given or taken back. A platform
/// administrator works in no workspace, so nothing changes for one. An
/// account left without any workspace gets a personal one.
async fn workspace_roles(
    state: &AppState,
    settings: &OidcSettings,
    account: &User,
    role: Role,
    groups: Option<&[String]>,
) -> Result<(), AppError> {
    if role == Role::Admin {
        return Ok(());
    }
    let id = account.id;
    // The highest role the person's groups give, per workspace that exists.
    let mut wanted: HashMap<Uuid, (WorkspaceRole, &str)> = HashMap::new();
    for mapping in settings
        .workspace_roles
        .iter()
        .filter(|m| groups.is_some_and(|groups| groups.contains(&m.group)))
    {
        let Some((workspace, _)) = repo::workspaces::by_ref(&state.db, &mapping.workspace).await?
        else {
            tracing::warn!(workspace = %mapping.workspace, group = %mapping.group,
                "single sign-on group mapping names no workspace; skipped");
            continue;
        };
        let entry = wanted
            .entry(workspace)
            .or_insert((mapping.role, mapping.group.as_str()));
        if mapping.role.rank() > entry.0.rank() {
            *entry = (mapping.role, mapping.group.as_str());
        }
    }
    let mut tx = state.db.begin().await?;
    // One sign-in of an account applies the mapping at a time.
    repo::sso::account(&mut *tx, id).await?;
    let grants = match groups {
        Some(_) => repo::sso::lock_grants(&mut tx, id).await?,
        // Unknown groups take nothing back.
        None => Vec::new(),
    };
    let mut changes = Vec::new();
    for (&workspace, &(want, group)) in &wanted {
        let current = repo::workspaces::role_of(&mut *tx, workspace, id).await?;
        let grant = grants.iter().find(|g| g.workspace_id == workspace);
        // The mapping's own role, which nobody changed since.
        let ours = grant.filter(|g| Some(g.role) == current);
        let by = format!("by single sign-on (group `{group}`)");
        match (current, ours) {
            (None, _) => {
                repo::workspaces::add_member(&mut *tx, workspace, id, want).await?;
                let grant = RoleGrant {
                    workspace_id: workspace,
                    role: want,
                    prior_role: None,
                };
                repo::sso::put_grant(&mut tx, id, &grant).await?;
                changes.push(Change {
                    workspace,
                    action: AuditAction::MemberAdded,
                    detail: format!("joined as {want}, {by}"),
                });
            }
            (Some(current), Some(ours)) => {
                if current == want {
                    continue;
                }
                if current == WorkspaceRole::Owner && !other_owner(&mut tx, workspace, id).await? {
                    continue;
                }
                repo::workspaces::set_role(&mut *tx, workspace, id, want).await?;
                let grant = RoleGrant {
                    workspace_id: workspace,
                    role: want,
                    prior_role: ours.prior_role,
                };
                repo::sso::put_grant(&mut tx, id, &grant).await?;
                changes.push(Change {
                    workspace,
                    action: AuditAction::MemberRoleChanged,
                    detail: format!("{current} -> {want}, {by}"),
                });
            }
            // A role the mapping did not give, or one someone changed by hand since: the
            // mapping raises it to what the groups give and never lowers it.
            (Some(current), None) => {
                if want.rank() > current.rank() {
                    repo::workspaces::set_role(&mut *tx, workspace, id, want).await?;
                    let grant = RoleGrant {
                        workspace_id: workspace,
                        role: want,
                        prior_role: Some(current),
                    };
                    repo::sso::put_grant(&mut tx, id, &grant).await?;
                    changes.push(Change {
                        workspace,
                        action: AuditAction::MemberRoleChanged,
                        detail: format!("{current} -> {want}, {by}"),
                    });
                } else if grant.is_some() {
                    repo::sso::drop_grant(&mut tx, id, workspace).await?;
                }
            }
        }
    }
    for grant in grants
        .iter()
        .filter(|g| !wanted.contains_key(&g.workspace_id))
    {
        let workspace = grant.workspace_id;
        let current = repo::workspaces::role_of(&mut *tx, workspace, id).await?;
        if current != Some(grant.role) {
            // Gone, or changed by hand since: what is there now is not the mapping's.
            repo::sso::drop_grant(&mut tx, id, workspace).await?;
            continue;
        }
        let losing_owner =
            grant.role == WorkspaceRole::Owner && grant.prior_role != Some(WorkspaceRole::Owner);
        if losing_owner && !other_owner(&mut tx, workspace, id).await? {
            tracing::warn!(user_id = %id, workspace_id = %workspace,
                "single sign-on group no longer applies, but the account stays the workspace's only owner");
            continue;
        }
        let why = "the single sign-on group no longer applies";
        match grant.prior_role {
            Some(prior) => {
                repo::workspaces::set_role(&mut *tx, workspace, id, prior).await?;
                changes.push(Change {
                    workspace,
                    action: AuditAction::MemberRoleChanged,
                    detail: format!("{} -> {prior}: {why}", grant.role),
                });
            }
            None => {
                repo::workspaces::remove_member(&mut tx, workspace, id).await?;
                changes.push(Change {
                    workspace,
                    action: AuditAction::MemberRemoved,
                    detail: format!("{why} (was {})", grant.role),
                });
            }
        }
        repo::sso::drop_grant(&mut tx, id, workspace).await?;
    }
    if repo::workspaces::count_for(&mut *tx, id).await? == 0 {
        crate::http::handlers::workspaces::create_owned(
            &mut tx,
            id,
            &crate::http::handlers::auth::personal_workspace_name(&account.name),
            &state.settings.llm_model,
        )
        .await?;
    }
    tx.commit().await?;
    let subject = format!("{} <{}>", account.name, account.email);
    for change in changes {
        if let Err(err) = repo::audit::record_system(
            &state.db,
            change.workspace,
            change.action,
            &subject,
            &change.detail,
        )
        .await
        {
            tracing::error!(workspace_id = %change.workspace, error = %err, "audit entry was not written");
        }
    }
    Ok(())
}

/// Whether the workspace keeps another owner who can sign in if `user` stops
/// being one (their rows are locked until the transaction ends).
async fn other_owner(
    tx: &mut sqlx::PgConnection,
    workspace: Uuid,
    user: Uuid,
) -> Result<bool, AppError> {
    let owners = repo::workspaces::lock_owners(&mut *tx, workspace).await?;
    Ok(owners.iter().any(|owner| *owner != user))
}
