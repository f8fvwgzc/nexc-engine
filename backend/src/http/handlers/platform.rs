//! The platform console: what whoever runs this installation sees across
//! all workspaces. It shows who registered and which workspaces exist, with
//! counts, never a workspace's content. Platform administrators are the
//! accounts whose role is `admin`; everyone else is a `user`, whatever they
//! are inside their own workspaces.

use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::infrastructure::require_instance_admin;
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::user::Role;
use crate::domain::validation::{FieldErrors, Validate};
use crate::http::extract::{AuthUser, Path, Query, ValidatedJson};
use crate::http::problem::Problem;

/// Query of the platform lists.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct PlatformQuery {
    /// Part of a name or e-mail address.
    pub q: Option<String>,
    /// 1-100, default 25.
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl PlatformQuery {
    fn parts(&self) -> (Option<String>, i64, i64) {
        let q = self
            .q
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(|q| q.chars().take(100).collect());
        (
            q,
            self.limit.unwrap_or(25).clamp(1, 100),
            self.offset.unwrap_or(0).clamp(0, 1_000_000),
        )
    }
}

/// A workspace as the platform sees it: who owns it and how big it is.
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct PlatformWorkspace {
    pub id: Uuid,
    pub name: String,
    /// Its first owner, by name and e-mail; `null` when the account is gone.
    #[schema(required = true)]
    pub owner_name: Option<String>,
    #[schema(required = true)]
    pub owner_email: Option<String>,
    pub member_count: i64,
    pub team_count: i64,
    pub issue_count: i64,
    pub graph_count: i64,
    pub created_at: DateTime<Utc>,
}

/// Every workspace of the installation, newest first (platform administrators).
#[utoipa::path(get, path = "/admin/workspaces", tag = "admin", security(("bearer" = [])), params(PlatformQuery),
    responses((status = 200, body = [PlatformWorkspace]), (status = 403, body = Problem)))]
pub async fn workspaces(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<PlatformQuery>,
) -> Result<Json<Vec<PlatformWorkspace>>, AppError> {
    require_instance_admin(auth)?;
    let (q, limit, offset) = query.parts();
    let rows = sqlx::query_as(
        "SELECT w.id, w.name, o.name AS owner_name, o.email AS owner_email,
                (SELECT count(*) FROM workspace_members m WHERE m.workspace_id = w.id) AS member_count,
                (SELECT count(*) FROM teams t WHERE t.workspace_id = w.id) AS team_count,
                (SELECT count(*) FROM issues i WHERE i.workspace_id = w.id) AS issue_count,
                (SELECT count(*) FROM graphs g WHERE g.workspace_id = w.id) AS graph_count,
                w.created_at
         FROM workspaces w
         LEFT JOIN LATERAL (
             SELECT u.name, u.email FROM workspace_members m JOIN users u ON u.id = m.user_id
             WHERE m.workspace_id = w.id AND m.role = 'owner'
             ORDER BY m.created_at, u.id LIMIT 1) o ON true
         WHERE $1::text IS NULL OR w.name ILIKE '%' || $1 || '%'
            OR o.email ILIKE '%' || $1 || '%' OR o.name ILIKE '%' || $1 || '%'
         ORDER BY w.created_at DESC, w.id LIMIT $2 OFFSET $3",
    )
    .bind(q)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

/// An account as the platform sees it.
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct PlatformUser {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    /// `admin` administers the platform; `user` is everyone else.
    pub role: String,
    /// Workspaces the account belongs to, and how many of them it owns.
    pub workspace_count: i64,
    pub owned_count: i64,
    /// Whether sign-in is locked right now (too many failed attempts).
    pub locked: bool,
    pub created_at: DateTime<Utc>,
}

const USER_COLUMNS: &str = "u.id, u.email, u.name, u.role,
    (SELECT count(*) FROM workspace_members m WHERE m.user_id = u.id) AS workspace_count,
    (SELECT count(*) FROM workspace_members m WHERE m.user_id = u.id AND m.role = 'owner')
        AS owned_count,
    COALESCE(u.locked_until > now(), false) AS locked, u.created_at";

/// Every account of the installation, newest first (platform administrators).
#[utoipa::path(get, path = "/admin/users", tag = "admin", security(("bearer" = [])), params(PlatformQuery),
    responses((status = 200, body = [PlatformUser]), (status = 403, body = Problem)))]
pub async fn users(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<PlatformQuery>,
) -> Result<Json<Vec<PlatformUser>>, AppError> {
    require_instance_admin(auth)?;
    let (q, limit, offset) = query.parts();
    let rows = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {USER_COLUMNS} FROM users u
         WHERE $1::text IS NULL OR u.email ILIKE '%' || $1 || '%' OR u.name ILIKE '%' || $1 || '%'
         ORDER BY u.created_at DESC, u.id LIMIT $2 OFFSET $3"
    )))
    .bind(q)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

/// `PATCH /admin/users/{uid}` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePlatformUser {
    /// `admin` lets the account administer the platform; `user` takes that away.
    pub role: Role,
}

impl Validate for UpdatePlatformUser {
    fn validate(&self, _errors: &mut FieldErrors) {}
}

/// Makes an account a platform administrator, or an ordinary user again.
/// Nobody changes their own role, so the platform always keeps the
/// administrator who is acting.
#[utoipa::path(patch, path = "/admin/users/{uid}", tag = "admin", security(("bearer" = [])),
    params(("uid" = Uuid, Path, description = "User id")), request_body = UpdatePlatformUser,
    responses((status = 200, body = PlatformUser), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Your own role", body = Problem)))]
pub async fn update_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(uid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<UpdatePlatformUser>,
) -> Result<Json<PlatformUser>, AppError> {
    require_instance_admin(auth)?;
    if uid == auth.id {
        return Err(AppError::Conflict(
            "you cannot change your own platform role; ask another administrator".into(),
        ));
    }
    let changed = sqlx::query("UPDATE users SET role = $2, updated_at = now() WHERE id = $1")
        .bind(uid)
        .bind(req.role.as_str())
        .execute(&state.db)
        .await?
        .rows_affected();
    if changed == 0 {
        return Err(AppError::NotFound("user"));
    }
    let user = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {USER_COLUMNS} FROM users u WHERE u.id = $1"
    )))
    .bind(uid)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(user))
}
