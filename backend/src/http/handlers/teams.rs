//! Teams of a workspace and their members.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::workspaces::{member_of, require};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::validation::{FieldErrors, Validate, check_max_len, check_text};
use crate::domain::workspace::{
    DESCRIPTION_MAX, NAME_MAX, Team, TeamAccess, TeamMember, TeamRole, Workspace, WorkspaceAction,
    is_valid_team_key, suggest_team_key,
};
use crate::http::extract::{AuthUser, Path, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo::{self, OrNotFound};

fn access(workspace: &Workspace, team: &Team) -> TeamAccess {
    TeamAccess {
        workspace_role: workspace.role,
        team_role: team.role,
        private: team.private,
    }
}

/// Loads a team the caller can see, with their standing towards it. A team
/// that is hidden from them is a 404, like one that does not exist.
pub async fn visible_team(
    state: &AppState,
    auth: AuthUser,
    wid: Uuid,
    tid: Uuid,
) -> Result<(Team, TeamAccess), AppError> {
    let workspace = member_of(state, auth, wid).await?;
    let team = repo::teams::find(&state.db, auth.id, wid, tid)
        .await
        .or_not_found("team")?;
    let access = access(&workspace, &team);
    if access.can_view() {
        Ok((team, access))
    } else {
        Err(AppError::NotFound("team"))
    }
}

fn require_manage(access: TeamAccess) -> Result<(), AppError> {
    if access.can_manage() {
        Ok(())
    } else {
        Err(AppError::Forbidden(
            "only team owners and workspace admins can manage this team".into(),
        ))
    }
}

/// `POST /workspaces/{wid}/teams` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTeam {
    pub name: String,
    /// Short identifier such as `ENG`; derived from the name when absent.
    pub key: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub private: bool,
}

impl Validate for CreateTeam {
    fn validate(&self, errors: &mut FieldErrors) {
        check_text(errors, "name", &self.name, NAME_MAX);
        check_max_len(errors, "description", &self.description, DESCRIPTION_MAX);
        match &self.key {
            Some(key) if !is_valid_team_key(key) => errors.add(
                "key",
                "must be an uppercase letter followed by up to 6 uppercase letters or digits",
            ),
            None if suggest_team_key(&self.name).is_empty() => {
                errors.add("key", "cannot be derived from this name; provide one");
            }
            _ => {}
        }
    }
}

/// `PATCH /workspaces/{wid}/teams/{tid}` body (any subset).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateTeam {
    pub name: Option<String>,
    pub description: Option<String>,
    pub private: Option<bool>,
}

impl Validate for UpdateTeam {
    fn validate(&self, errors: &mut FieldErrors) {
        if let Some(name) = &self.name {
            check_text(errors, "name", name, NAME_MAX);
        }
        if let Some(description) = &self.description {
            check_max_len(errors, "description", description, DESCRIPTION_MAX);
        }
    }
}

/// `PUT /workspaces/{wid}/teams/{tid}/members/{uid}` body.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetTeamMember {
    /// Defaults to `member`.
    pub role: Option<TeamRole>,
}

impl Validate for SetTeamMember {
    fn validate(&self, _errors: &mut FieldErrors) {}
}

/// The teams of a workspace that the caller can see.
#[utoipa::path(get, path = "/workspaces/{wid}/teams", tag = "teams", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = [Team]), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<Vec<Team>>, AppError> {
    let workspace = member_of(&state, auth, wid).await?;
    let mut teams = repo::teams::list(&state.db, auth.id, wid).await?;
    teams.retain(|team| access(&workspace, team).can_view());
    Ok(Json(teams))
}

/// Creates a team (members and above); the caller becomes its owner.
#[utoipa::path(post, path = "/workspaces/{wid}/teams", tag = "teams", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = CreateTeam,
    responses((status = 201, body = Team), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "The key is taken", body = Problem), (status = 422, body = Problem)))]
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<CreateTeam>,
) -> Result<(StatusCode, Json<Team>), AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::CreateTeam,
    )?;
    let key = req.key.unwrap_or_else(|| suggest_team_key(&req.name));
    let mut tx = state.db.begin().await?;
    let id = repo::teams::create(
        &mut *tx,
        wid,
        req.name.trim(),
        &key,
        req.description.trim(),
        req.private,
    )
    .await
    .map_err(|err| match &err {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            AppError::Conflict(format!("the key {key} is already used by another team"))
        }
        _ => err.into(),
    })?;
    repo::teams::upsert_member(&mut *tx, id, auth.id, TeamRole::Owner).await?;
    repo::issues::seed_states(&mut tx, id).await?;
    tx.commit().await?;
    let team = repo::teams::find(&state.db, auth.id, wid, id)
        .await
        .or_not_found("team")?;
    Ok((StatusCode::CREATED, Json(team)))
}

/// One team.
#[utoipa::path(get, path = "/workspaces/{wid}/teams/{tid}", tag = "teams", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("tid" = Uuid, Path, description = "Team id")),
    responses((status = 200, body = Team), (status = 404, body = Problem)))]
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, tid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Team>, AppError> {
    Ok(Json(visible_team(&state, auth, wid, tid).await?.0))
}

/// Updates a team (team owners and workspace admins).
#[utoipa::path(patch, path = "/workspaces/{wid}/teams/{tid}", tag = "teams", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("tid" = Uuid, Path, description = "Team id")),
    request_body = UpdateTeam,
    responses((status = 200, body = Team), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, tid)): Path<(Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<UpdateTeam>,
) -> Result<Json<Team>, AppError> {
    require_manage(visible_team(&state, auth, wid, tid).await?.1)?;
    repo::teams::update(
        &state.db,
        tid,
        req.name.as_deref().map(str::trim),
        req.description.as_deref().map(str::trim),
        req.private,
    )
    .await?;
    Ok(Json(visible_team(&state, auth, wid, tid).await?.0))
}

/// Deletes a team (team owners and workspace admins).
#[utoipa::path(delete, path = "/workspaces/{wid}/teams/{tid}", tag = "teams", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("tid" = Uuid, Path, description = "Team id")),
    responses((status = 204, description = "Deleted"), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "The team still has graphs", body = Problem)))]
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, tid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    require_manage(visible_team(&state, auth, wid, tid).await?.1)?;
    let graphs = repo::graphs::count_in_team(&state.db, tid).await?;
    if graphs > 0 {
        return Err(AppError::Conflict(format!(
            "this team still has {graphs} graph(s); move or delete them first"
        )));
    }
    repo::teams::delete(&state.db, tid).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Members of a team.
#[utoipa::path(get, path = "/workspaces/{wid}/teams/{tid}/members", tag = "teams", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("tid" = Uuid, Path, description = "Team id")),
    responses((status = 200, body = [TeamMember]), (status = 404, body = Problem)))]
pub async fn members(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, tid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<TeamMember>>, AppError> {
    visible_team(&state, auth, wid, tid).await?;
    Ok(Json(repo::teams::members(&state.db, tid).await?))
}

/// Adds a workspace member to the team or changes their team role. Anyone
/// may add themselves to a public team; everything else needs a team owner
/// or a workspace admin.
#[utoipa::path(put, path = "/workspaces/{wid}/teams/{tid}/members/{uid}", tag = "teams", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("tid" = Uuid, Path, description = "Team id"),
        ("uid" = Uuid, Path, description = "User id")),
    request_body = SetTeamMember,
    responses((status = 200, body = [TeamMember]), (status = 403, body = Problem),
        (status = 404, body = Problem), (status = 422, description = "Not a workspace member", body = Problem)))]
pub async fn set_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, tid, uid)): Path<(Uuid, Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<SetTeamMember>,
) -> Result<Json<Vec<TeamMember>>, AppError> {
    let (_, access) = visible_team(&state, auth, wid, tid).await?;
    let role = req.role.unwrap_or(TeamRole::Member);
    let joining = uid == auth.id && role == TeamRole::Member && access.can_join();
    if !joining {
        require_manage(access)?;
    }
    if repo::workspaces::role_of(&state.db, wid, uid)
        .await?
        .is_none()
    {
        return Err(AppError::field(
            "user_id",
            "only members of the workspace can be added to its teams",
        ));
    }
    repo::teams::upsert_member(&state.db, tid, uid, role).await?;
    Ok(Json(repo::teams::members(&state.db, tid).await?))
}

/// Removes a member from the team, or lets the caller leave it.
#[utoipa::path(delete, path = "/workspaces/{wid}/teams/{tid}/members/{uid}", tag = "teams", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("tid" = Uuid, Path, description = "Team id"),
        ("uid" = Uuid, Path, description = "User id")),
    responses((status = 204, description = "Removed"), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn remove_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, tid, uid)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let (_, access) = visible_team(&state, auth, wid, tid).await?;
    if uid != auth.id {
        require_manage(access)?;
    }
    if repo::teams::remove_member(&state.db, tid, uid).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound("member"))
    }
}
