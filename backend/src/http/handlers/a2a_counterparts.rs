//! The A2A counterparts of a workspace: remote agents its agents may call
//! as `a2a:<name>` and that may send its exposed agents tasks. Registered,
//! checked and removed by admins; listed to every member. Each counterpart
//! is also an external agent principal of the workspace, whose API tokens
//! are what its inbound calls carry.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use uuid::Uuid;

use super::workspaces::{audit, member_of, require};
use crate::a2a::client;
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::a2a::{A2aCheck, A2aCounterpart, A2aCounterpartInput, RegisteredA2aCounterpart};
use crate::domain::agent::{AgentRuntime, AgentStatus, IssuedAgentApiToken};
use crate::domain::alert_routing::check_url;
use crate::domain::audit::AuditAction;
use crate::domain::capabilities::{Capabilities, Delegation};
use crate::domain::policy::{Action, decide_with};
use crate::domain::validation::{FieldErrors, Validate};
use crate::domain::workspace::WorkspaceAction;
use crate::engine::policy::{levels_for, resolve};
use crate::gateway::fetch::is_private_host;
use crate::http::extract::{AuthUser, Path, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo;
use crate::repo::a2a::NewCounterpart;
use crate::repo::agents::AgentFields;
use crate::repo::audit::Subject;
use crate::security::api_token;

/// Most counterparts one workspace registers.
pub const COUNTERPARTS_MAX: i64 = 64;

impl Validate for A2aCounterpartInput {
    fn validate(&self, errors: &mut FieldErrors) {
        self.check(errors);
    }
}

/// The host of a card URL, checked as the gateway checks an egress host:
/// http(s) without credentials, not private or loopback unless the server
/// allows it in development, not denied by the workspace's policy.
async fn checked_host(state: &AppState, wid: Uuid, url: &str) -> Result<String, AppError> {
    let host = check_url(url).map_err(|problem| AppError::field("card_url", problem))?;
    if is_private_host(&host) && !state.settings.a2a_allow_private_hosts {
        return Err(AppError::field(
            "card_url",
            format!(
                "{host} is on this machine or a private network; such a counterpart is not reached (NEXC_A2A_ALLOW_PRIVATE_HOSTS=true allows it in development)"
            ),
        ));
    }
    let policies = repo::policies::latest_all(&state.db, wid).await?;
    let resolved = resolve(&levels_for(&policies, None, None), None);
    let decision = decide_with(
        &resolved.capabilities,
        &resolved.rules,
        &[],
        &Action::Egress(&host),
    );
    if !decision.allowed {
        return Err(AppError::field(
            "card_url",
            format!(
                "{host} is refused by the workspace's policy ({}): {}",
                decision.rule, decision.reason
            ),
        ));
    }
    Ok(host)
}

/// The A2A counterparts of a workspace (members and above).
#[utoipa::path(get, path = "/workspaces/{wid}/a2a-counterparts", tag = "a2a", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = [A2aCounterpart]), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<Vec<A2aCounterpart>>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::ViewMembers,
    )?;
    Ok(Json(
        repo::a2a::list_counterparts(&state.db, wid)
            .await?
            .into_iter()
            .map(|row| row.counterpart)
            .collect(),
    ))
}

/// Registers a remote A2A agent (admins): its card is fetched and validated
/// at once (a card that does not answer is still registered, with the
/// failure on it), an external agent principal `a2a:<name>` is made for its
/// inbound calls, and the first API token of that principal is in the answer
/// once. Audited as `a2a_counterpart_added`.
#[utoipa::path(post, path = "/workspaces/{wid}/a2a-counterparts", tag = "a2a", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = A2aCounterpartInput,
    responses((status = 201, body = RegisteredA2aCounterpart), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, body = Problem), (status = 422, body = Problem)))]
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<A2aCounterpartInput>,
) -> Result<(StatusCode, Json<RegisteredA2aCounterpart>), AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    if repo::a2a::count_counterparts(&state.db, wid).await? >= COUNTERPARTS_MAX {
        return Err(AppError::Unprocessable(format!(
            "a workspace registers at most {COUNTERPARTS_MAX} A2A counterparts"
        )));
    }
    let name = req.name.trim().to_owned();
    if repo::a2a::find_counterpart_by_name(&state.db, wid, &name)
        .await?
        .is_some()
    {
        return Err(AppError::Conflict(format!(
            "an A2A counterpart named {name} is already registered"
        )));
    }
    let card_url = req.card_url.trim().to_owned();
    let host = checked_host(&state, wid, &card_url).await?;
    let agent_name = format!("a2a:{name}");
    let agents = repo::agents::list(&state.db, wid).await?;
    if agents.iter().any(|a| a.name == agent_name) {
        return Err(AppError::Conflict(format!(
            "an agent named {agent_name} already exists in the workspace"
        )));
    }
    for id in &req.allowed_agents {
        if !agents.iter().any(|a| a.id == *id) {
            return Err(AppError::field(
                "allowed_agents",
                format!("{id} is not an agent of the workspace"),
            ));
        }
    }
    let id = Uuid::now_v7();
    let sealed = match req
        .auth_header
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty())
    {
        Some(header) => Some(
            state
                .secret_box
                .seal(header.as_bytes(), id.as_bytes())
                .map_err(AppError::Internal)?,
        ),
        None => None,
    };
    // The principal its calls act as: an external agent that does nothing inside nexc itself.
    let fields = AgentFields {
        name: agent_name.clone(),
        role: "counterpart".into(),
        title: format!("A2A counterpart at {host}"),
        model: state.settings.llm_model.clone(),
        system_prompt: String::new(),
        reports_to: None,
        budget_tokens: 0,
        runtime: AgentRuntime::External,
        status: AgentStatus::Active,
        owner_user_id: Some(auth.id),
        team_id: None,
        purpose: format!(
            "The remote A2A agent registered as counterpart {name} ({host}); its calls act as this principal."
        ),
        capabilities: Capabilities {
            tools: Some(Vec::new()),
            egress_hosts: Some(Vec::new()),
            counterparts: Some(Vec::new()),
            delegation: Delegation {
                max_depth: 0,
                max_fan_out: 0,
            },
            ..Capabilities::default()
        },
        a2a_exposed: false,
    };
    let secret = api_token::generate();
    let mut tx = state.db.begin().await?;
    let agent = repo::agents::create(&mut *tx, auth.id, wid, &fields).await?;
    let token = repo::agent_tokens::insert(
        &mut *tx,
        agent.id,
        &api_token::hash(&secret),
        &api_token::prefix_of(&secret),
        "a2a",
        auth.id,
    )
    .await?;
    repo::a2a::insert_counterpart(
        &mut *tx,
        id,
        &NewCounterpart {
            workspace_id: wid,
            name: &name,
            card_url: &card_url,
            auth_header: sealed.as_deref(),
            allowed_agents: &req.allowed_agents,
            max_label: req.max_label,
            agent_id: agent.id,
            created_by: auth.id,
        },
    )
    .await?;
    tx.commit().await?;
    let row = repo::a2a::find_counterpart(&state.db, wid, id)
        .await?
        .ok_or(AppError::NotFound("a2a counterpart"))?;
    let check = client::fetch_card(&state, &row).await;
    audit(
        &state,
        wid,
        auth.id,
        AuditAction::A2aCounterpartAdded,
        Subject::Text(&format!("a2a:{name}")),
        &format!("{host}; principal {}; {}", agent.principal_id, check.detail),
    )
    .await;
    audit(
        &state,
        wid,
        auth.id,
        AuditAction::AgentTokenIssued,
        Subject::Text(&agent.principal_id),
        &format!("token {} ({})", token.prefix, token.name),
    )
    .await;
    let counterpart = repo::a2a::find_counterpart(&state.db, wid, id)
        .await?
        .map(|row| row.counterpart)
        .ok_or(AppError::NotFound("a2a counterpart"))?;
    Ok((
        StatusCode::CREATED,
        Json(RegisteredA2aCounterpart {
            counterpart,
            token: IssuedAgentApiToken { token, secret },
        }),
    ))
}

/// One counterpart of the workspace.
#[utoipa::path(get, path = "/workspaces/{wid}/a2a-counterparts/{cid}", tag = "a2a", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("cid" = Uuid, Path, description = "Counterpart id")),
    responses((status = 200, body = A2aCounterpart), (status = 404, body = Problem)))]
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, cid)): Path<(Uuid, Uuid)>,
) -> Result<Json<A2aCounterpart>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::ViewMembers,
    )?;
    let row = repo::a2a::find_counterpart(&state.db, wid, cid)
        .await?
        .ok_or(AppError::NotFound("a2a counterpart"))?;
    Ok(Json(row.counterpart))
}

/// Fetches the counterpart's card again and records what it says (admins).
#[utoipa::path(post, path = "/workspaces/{wid}/a2a-counterparts/{cid}/check", tag = "a2a", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("cid" = Uuid, Path, description = "Counterpart id")),
    responses((status = 200, body = A2aCheck), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn check(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, cid)): Path<(Uuid, Uuid)>,
) -> Result<Json<A2aCheck>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    let row = repo::a2a::find_counterpart(&state.db, wid, cid)
        .await?
        .ok_or(AppError::NotFound("a2a counterpart"))?;
    Ok(Json(client::fetch_card(&state, &row).await))
}

/// Removes a counterpart with its principal (admins): its tool stops being
/// offered at the next run and its tokens stop working at once. Audited as
/// `a2a_counterpart_removed`.
#[utoipa::path(delete, path = "/workspaces/{wid}/a2a-counterparts/{cid}", tag = "a2a", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("cid" = Uuid, Path, description = "Counterpart id")),
    responses((status = 204, description = "Removed"), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, cid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    let row = repo::a2a::find_counterpart(&state.db, wid, cid)
        .await?
        .ok_or(AppError::NotFound("a2a counterpart"))?;
    let mut tx = state.db.begin().await?;
    if !repo::a2a::delete_counterpart(&mut tx, wid, cid).await? {
        return Err(AppError::NotFound("a2a counterpart"));
    }
    tx.commit().await?;
    audit(
        &state,
        wid,
        auth.id,
        AuditAction::A2aCounterpartRemoved,
        Subject::Text(&format!("a2a:{}", row.counterpart.name)),
        &format!(
            "{}; principal {} removed",
            row.counterpart.host, row.counterpart.principal_id
        ),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
