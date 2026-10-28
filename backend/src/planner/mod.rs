//! The Planner (it was called the orchestrator): the agent organisation under
//! the Planner agent (the default organisation of a workspace and the expert
//! catalog, `docs/AGENT-CATALOG.md`), node → agent assignment, heartbeats and
//! the status report. The graph planning itself (goal → graph) lives in
//! `engine::planner`.
#![forbid(unsafe_code)]

pub mod health;

use sqlx::PgConnection;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::agent::{Agent, AgentRuntime, AgentStatus, DEFAULT_ORG};
use crate::domain::agent_template;
use crate::domain::audit::AuditAction;
use crate::domain::capabilities::Capabilities;
use crate::domain::graph::GraphNode;
use crate::domain::ontology::Ontology;
use crate::domain::policy_document::PolicyDocument;
use crate::domain::policy_document::{PolicyScope, diff_summary};
use crate::domain::skill;
use crate::domain::status::{Backends, PlannerStatus};
use crate::domain::validation::FieldErrors;
use crate::engine::credentials;
use crate::engine::policy::{self, Resolved};
use crate::repo;
use crate::repo::agents::AgentFields;
use crate::repo::audit::Subject;

/// Creates the default organisation of a workspace (planner at the top, the
/// specialists reporting to it). Must run inside a transaction.
pub async fn seed_default_org(
    conn: &mut PgConnection,
    owner: Uuid,
    workspace: Uuid,
    model: &str,
) -> Result<(), sqlx::Error> {
    let mut ids = std::collections::HashMap::new();
    for seed in DEFAULT_ORG {
        let fields = AgentFields {
            name: seed.name.into(),
            role: seed.role.into(),
            title: seed.title.into(),
            model: model.into(),
            system_prompt: seed.system_prompt.into(),
            reports_to: seed.reports_to.and_then(|role| ids.get(role).copied()),
            budget_tokens: seed.budget_tokens,
            runtime: seed.runtime,
            status: AgentStatus::Active,
            owner_user_id: Some(owner),
            team_id: None,
            purpose: seed.purpose.into(),
            capabilities: Capabilities::default(),
            a2a_exposed: false,
            skills: seed.skills.iter().map(|s| (*s).to_owned()).collect(),
            template_id: None,
        };
        let agent = repo::agents::create(&mut *conn, owner, workspace, &fields).await?;
        repo::agents::audit_created(&mut *conn, workspace, owner, &agent, "default organisation")
            .await?;
        ids.insert(seed.role, agent.id);
    }
    Ok(())
}

/// Brings agents in line with workspaces: agents created before workspaces
/// existed move into their creator's first workspace, then every workspace
/// that still has none gets the default organisation.
pub async fn seed_missing_orgs(state: &AppState) -> anyhow::Result<()> {
    let mut adopted = 0;
    // One owner at a time, so that two people's "Researcher" do not race for the same workspace.
    for owner in repo::agents::orphan_owners(&state.db).await? {
        adopted += repo::agents::adopt_orphans_of(&state.db, owner).await?;
    }
    if adopted > 0 {
        tracing::info!(adopted, "agents assigned to their creators' workspaces");
    }
    for (workspace, created_by) in repo::agents::workspaces_without_agents(&state.db).await? {
        // Agents record who created them; a workspace whose creator is gone has nobody to name.
        let Some(owner) = created_by else { continue };
        let mut tx = state.db.begin().await?;
        seed_default_org(&mut tx, owner, workspace, &state.settings.llm_model).await?;
        tx.commit().await?;
    }
    Ok(())
}

/// The catalog seed every workspace receives once (`workspace_catalog_seeds`).
pub const CATALOG_SEED: &str = "experts-v1";

/// What a catalog seed did in one workspace.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CatalogSeeded {
    /// Expert agents created.
    pub created: usize,
    /// Agents of the default organisation brought up to date.
    pub upgraded: usize,
    /// Experts skipped because the workspace has an agent of that name or role.
    pub skipped: usize,
}

/// Gives a workspace the expert catalog, once ([`CATALOG_SEED`]); `None`
/// when it had it already, or when another backend is seeding it right now.
/// Agents of the default organisation still on a prompt an earlier release
/// seeded get the current prompt (and the current purpose where theirs is
/// unchanged too); those without skills get theirs, and so does an agent made
/// from a template before skills existed. Every expert template becomes an
/// agent reporting to the Planner, unless the workspace has an agent of its
/// name or role: its capabilities (its knowledge topics bound where the
/// workspace has them), its skills, and its policy as the agent's version 1,
/// without the fields that would widen the workspace's policies (the
/// workspace's stricter value applies to those anyway; the audit names them).
/// Its model allow-list stays open (an agent made from a template by hand pins
/// it), and its competency questions are asked on request
/// (`POST /agents/{id}/evaluate`). Every change is audited, as `owner`, in the
/// caller's transaction; updates touch only the fields they change, so an edit
/// made meanwhile elsewhere is kept.
pub async fn seed_catalog(
    conn: &mut PgConnection,
    owner: Uuid,
    workspace: Uuid,
    model: &str,
) -> Result<Option<CatalogSeeded>, sqlx::Error> {
    if !repo::catalog_seeds::claim(&mut *conn, workspace, CATALOG_SEED).await? {
        return Ok(None);
    }
    let mut seeded = CatalogSeeded::default();
    let existing = repo::agents::list(&mut *conn, workspace).await?;
    for agent in existing
        .iter()
        .filter(|a| a.runtime != AgentRuntime::External)
    {
        let seed = DEFAULT_ORG.iter().find(|s| s.role == agent.role);
        let mut changes = Vec::new();
        if let Some(seed) = seed
            && repo::agents::upgrade_default_prompt(
                &mut *conn,
                workspace,
                agent.id,
                seed.system_prompt,
                seed.purpose,
                seed.former_prompts,
                seed.former_purposes,
            )
            .await?
        {
            changes.push("the current default prompt".to_owned());
        }
        // Skills for an agent that has none: the default organisation's, or its template's.
        let skills: Option<Vec<String>> = match (seed, agent.template_id.as_deref()) {
            (_, Some(template)) if agent_template::find(template).is_some() => {
                agent_template::find(template).map(|t| skill::normalize(&t.skills))
            }
            (Some(seed), _) => Some(seed.skills.iter().map(|s| (*s).to_owned()).collect()),
            _ => None,
        };
        if let Some(skills) = skills
            && agent.skills.is_empty()
            && repo::agents::set_skills_if_none(&mut *conn, workspace, agent.id, &skills).await?
        {
            changes.push(skill::describe_change(&[], &skills));
        }
        if changes.is_empty() {
            continue;
        }
        seeded.upgraded += 1;
        repo::audit::record(
            &mut *conn,
            workspace,
            owner,
            AuditAction::AgentSkillsChanged,
            Subject::Text(&agent.principal_id),
            &format!("catalog {CATALOG_SEED}: {}", changes.join("; ")),
        )
        .await?;
    }

    let planner = existing
        .iter()
        .filter(|a| a.role == "planner")
        .min_by_key(|a| a.created_at)
        .map(|a| a.id);
    let latest = repo::policies::latest_all(&mut *conn, workspace).await?;
    let above = policy::resolve(&policy::levels_for(&latest, None, None), None);
    let topics: Vec<(Uuid, Vec<String>)> = repo::knowledge::topics(&mut *conn, workspace)
        .await?
        .into_iter()
        .map(|t| {
            let mut terms = t.terms;
            terms.extend(t.label.split(" · ").map(str::to_owned));
            (t.id, terms)
        })
        .collect();
    for template in agent_template::experts() {
        let taken = existing
            .iter()
            .any(|a| a.role == template.role || a.name.eq_ignore_ascii_case(&template.name));
        if taken {
            seeded.skipped += 1;
            continue;
        }
        let mut capabilities = template.capabilities.clone();
        if let Some(bound) = template.bound_topics(&topics) {
            capabilities.data.topics = Some(bound);
        }
        let fields = AgentFields {
            name: template.name.clone(),
            role: template.role.clone(),
            title: template.title.clone(),
            model: model.into(),
            system_prompt: template.system_prompt.clone(),
            reports_to: planner,
            budget_tokens: template.budget_tokens,
            runtime: template.runtime,
            status: AgentStatus::Active,
            owner_user_id: Some(owner),
            team_id: None,
            purpose: template.purpose.clone(),
            capabilities,
            a2a_exposed: false,
            skills: skill::normalize(&template.skills),
            template_id: Some(template.id.clone()),
        };
        let agent = repo::agents::create(&mut *conn, owner, workspace, &fields).await?;
        repo::agents::audit_created(
            &mut *conn,
            workspace,
            owner,
            &agent,
            &format!(
                "from template {} (expert catalog {CATALOG_SEED})",
                template.id
            ),
        )
        .await?;
        let (document, left_out) = narrowed(&above, &template.policy);
        let version = repo::policies::insert(
            &mut *conn,
            workspace,
            PolicyScope::Agent,
            Some(agent.id),
            &document,
            owner,
        )
        .await?;
        let mut detail = format!(
            "from template {}: agent policy version {}: {}",
            template.id,
            version.version,
            diff_summary(&Default::default(), &document)
        );
        if !left_out.is_empty() {
            detail.push_str(&format!(
                "; left out, the workspace's stricter rule applies: {}",
                left_out.join(", ")
            ));
        }
        repo::audit::record(
            &mut *conn,
            workspace,
            owner,
            AuditAction::PolicyChanged,
            Subject::Text(&agent.principal_id),
            &detail,
        )
        .await?;
        seeded.created += 1;
    }
    Ok(Some(seeded))
}

/// `child` without the fields that would widen `parent` (each falls back to
/// the level above, whose value is the stricter one), and the paths it left
/// out. A document that cannot be narrowed field by field comes back empty.
fn narrowed(parent: &Resolved, child: &PolicyDocument) -> (PolicyDocument, Vec<String>) {
    let mut errors = FieldErrors::default();
    policy::check_narrowing(parent, child, &mut errors);
    if errors.is_empty() {
        return (child.clone(), Vec::new());
    }
    let paths: Vec<String> = errors
        .as_map()
        .keys()
        .map(|k| k.strip_prefix("document.").unwrap_or(k).to_owned())
        .collect();
    let mut value = serde_json::to_value(child).expect("a policy document serialises");
    for path in &paths {
        let mut node = &mut value;
        let mut parts = path.split('.').peekable();
        while let Some(part) = parts.next() {
            let Some(object) = node.as_object_mut() else {
                break;
            };
            if parts.peek().is_none() {
                object.insert(part.to_owned(), serde_json::Value::Null);
                break;
            }
            match object.get_mut(part) {
                Some(next) => node = next,
                None => break,
            }
        }
    }
    let document = serde_json::from_value::<PolicyDocument>(value)
        .ok()
        .filter(|d| {
            let mut again = FieldErrors::default();
            policy::check_narrowing(parent, d, &mut again);
            again.is_empty()
        })
        .unwrap_or_default();
    (document, paths)
}

/// Gives every workspace that has not had it the expert catalog
/// (`NEXC_SEED_EXPERT_AGENTS`, on by default), as one of its owners or admins
/// (its creator first). Runs in the background at start and every few
/// minutes, so that a workspace one backend could not seed, or one created by a
/// backend of an earlier release during a rolling update, gets it later. A
/// workspace with no owner or admin left is skipped.
pub async fn seed_catalog_everywhere(state: &AppState) -> anyhow::Result<()> {
    if !state.settings.seed_expert_agents {
        return Ok(());
    }
    let mut seeded_any = false;
    for (workspace, owner) in
        repo::catalog_seeds::workspaces_without(&state.db, CATALOG_SEED).await?
    {
        let Some(owner) = owner else { continue };
        let mut tx = state.db.begin().await?;
        match seed_catalog(&mut tx, owner, workspace, &state.settings.llm_model).await {
            Ok(Some(seeded)) => {
                tx.commit().await?;
                seeded_any = true;
                state
                    .caches
                    .invalidate(crate::app::caches::Key::Policies(workspace));
                tracing::info!(%workspace, created = seeded.created, upgraded = seeded.upgraded,
                    skipped = seeded.skipped, "the expert catalog was added to the workspace");
            }
            Ok(None) => tx.rollback().await?,
            Err(err) => {
                tx.rollback().await?;
                tracing::error!(%workspace, error = %err, "the expert catalog was not added; it is tried again later");
            }
        }
    }
    if seeded_any {
        state.caches.invalidate(crate::app::caches::Key::Agents);
    }
    Ok(())
}

/// The agent of the graph's workspace that is responsible for a node: by its
/// `agent_role`, else by the default role of its type in `ontology`.
pub async fn assign(
    state: &AppState,
    workspace: Option<Uuid>,
    node: &GraphNode,
    ontology: &Ontology,
) -> Result<Option<Agent>, sqlx::Error> {
    // A graph that no workspace adopted has no agents to draw on.
    let Some(workspace) = workspace else {
        return Ok(None);
    };
    let type_role = ontology.role_for(&node.kind);
    let role = node
        .agent_role
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .unwrap_or(type_role);
    match repo::agents::find_by_role(&state.db, workspace, role).await? {
        Some(agent) => Ok(Some(agent)),
        None => repo::agents::find_by_role(&state.db, workspace, type_role).await,
    }
}

/// One heartbeat tick: marks busy agents alive and flags exhausted budgets.
pub async fn heartbeat(state: &AppState) -> anyhow::Result<()> {
    let busy = state.engine.busy_agents();
    if !busy.is_empty() {
        repo::agents::heartbeat(&state.db, &busy).await?;
        for agent in &busy {
            state
                .caches
                .invalidate(crate::app::caches::Key::Agent(*agent));
        }
    }
    let flagged = repo::agents::flag_over_budget(&state.db).await?;
    if flagged > 0 {
        tracing::info!(flagged, "agents reached their token budget");
        state.caches.invalidate(crate::app::caches::Key::Agents);
    }
    Ok(())
}

/// Status report for `owner`.
pub async fn status(state: &AppState, owner: Uuid) -> Result<PlannerStatus, AppError> {
    let (queue_depth, running_nodes, active_runs) = repo::runs::activity(&state.db, owner).await?;
    let agents_active = repo::agents::count_active(&state.db, owner).await?;
    let llm = credentials::resolve(state, owner, None).await?;
    Ok(PlannerStatus {
        demo_mode: llm.is_demo(),
        queue_depth,
        running_nodes,
        active_runs,
        agents_active,
        backends: Backends {
            llm: health::llm(&llm),
            agent_runtime: state.health.agent_runtime(state).await,
            symphony: state.health.symphony(state).await,
        },
    })
}
