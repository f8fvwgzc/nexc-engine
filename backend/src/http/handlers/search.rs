//! Searching a workspace from the command palette.
//!
//! One request looks through issues, projects, graphs, documents, teams and
//! members. Each kind goes through the same rule as its own list, so a
//! search never shows what the lists would not: a private team's issues
//! stay with the team, and guests see neither documents nor the roster.

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use super::workspaces::member_of;
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::search::{self, PER_KIND_MAX, SearchHit, SearchKind};
use crate::domain::workspace::{TeamAccess, WorkspaceAction, can};
use crate::http::extract::{AuthUser, Path, Query};
use crate::http::problem::Problem;
use crate::repo;
use crate::repo::issues::IssueFilter;

/// Query of `GET /workspaces/{wid}/search`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct SearchQuery {
    /// What to look for: part of a title, a name, an e-mail, or an issue's
    /// identifier (`ENG-12`). At least 2 characters.
    pub q: String,
    /// Most hits of each kind, 1-10 (default 5).
    pub limit: Option<i64>,
}

/// Searches the workspace for what the caller may see: issues (by title or
/// identifier), projects, graphs, documents, teams and members (by name or
/// e-mail), in that order, with up to `limit` hits of each kind. Fewer than
/// two characters match nothing.
#[utoipa::path(get, path = "/workspaces/{wid}/search", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), SearchQuery),
    responses((status = 200, body = [SearchHit]), (status = 404, body = Problem)))]
pub async fn search(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<SearchHit>>, AppError> {
    let workspace = member_of(&state, auth, wid).await?;
    let Some(q) = search::query(&query.q) else {
        return Ok(Json(Vec::new()));
    };
    let limit = query.limit.unwrap_or(5).clamp(1, PER_KIND_MAX);
    let take = usize::try_from(limit).unwrap_or(5);
    let needle = q.to_lowercase();
    let db = &state.db;
    // Guests have no documents and no roster, as on the pages themselves.
    let member = workspace.role.is_member();
    let roster = can(workspace.role, WorkspaceAction::ViewMembers);

    let filter = IssueFilter {
        q: Some(q.clone()),
        limit,
        ..IssueFilter::default()
    };
    let (issues, projects, graphs, documents, teams, members) = tokio::try_join!(
        repo::issues::list(db, auth.id, wid, &filter),
        repo::issues::projects(db, auth.id, wid),
        repo::graphs::search(db, auth.id, wid, &q, limit),
        async {
            if member {
                repo::knowledge::list(db, wid, Some(&q), limit, 0).await
            } else {
                Ok(Vec::new())
            }
        },
        repo::teams::list(db, auth.id, wid),
        async {
            if roster {
                repo::workspaces::members(db, wid).await
            } else {
                Ok(Vec::new())
            }
        },
    )?;

    let hit = |kind, id, title: String, subtitle: String| SearchHit {
        kind,
        id,
        title,
        subtitle,
    };
    let mut hits: Vec<SearchHit> = issues
        .into_iter()
        .map(|i| {
            let title = format!("{} {}", i.identifier, i.title);
            hit(SearchKind::Issue, i.id, title, i.state.name)
        })
        .collect();
    hits.extend(
        projects
            .into_iter()
            .filter(|p| search::matches(&p.name, &needle))
            .take(take)
            .map(|p| hit(SearchKind::Project, p.id, p.name, p.status.to_string())),
    );
    hits.extend(
        graphs
            .into_iter()
            .map(|(id, name, goal)| hit(SearchKind::Graph, id, name, goal)),
    );
    hits.extend(
        documents
            .into_iter()
            .map(|d| hit(SearchKind::Document, d.id, d.name, d.status.to_string())),
    );
    hits.extend(
        teams
            .into_iter()
            .filter(|t| {
                TeamAccess {
                    workspace_role: workspace.role,
                    team_role: t.role,
                    private: t.private,
                }
                .can_view()
            })
            .filter(|t| search::matches(&t.name, &needle) || search::matches(&t.key, &needle))
            .take(take)
            .map(|t| hit(SearchKind::Team, t.id, t.name, t.key)),
    );
    hits.extend(
        members
            .into_iter()
            .filter(|m| search::matches(&m.name, &needle) || search::matches(&m.email, &needle))
            .take(take)
            .map(|m| hit(SearchKind::Member, m.user_id, m.name, m.email)),
    );
    Ok(Json(hits))
}
