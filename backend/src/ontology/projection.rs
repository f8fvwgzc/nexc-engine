//! The workspace projected into onto-intelli: teams, people, projects,
//! cycles, issues, agents, graphs, documents and runs become entities and
//! links in the workspace's tenant, so onto-intelli's own UI renders the
//! workspace map and its reasoning (paths, impact, root cause) covers it.
//!
//! Only ids, names, states and links leave the database: no issue
//! description, no document text, no e-mail address. Private teams, and
//! what belongs to them, are `restricted` to the team's members there; a
//! confidential or restricted document is shown by its id only.
//!
//! Changes reach onto-intelli through the export outbox the tables fill
//! (`onto_export_queue`, the same worker as the audit's facts); a workspace
//! is backfilled whole once. `onto_projection` remembers what was put there
//! and the links stated from each entity, so a link that no longer holds is
//! taken back and only entities that were put there are deleted.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde_json::{Map, Value, json};
use uuid::Uuid;

use super::facts::{ADMINS, agent_id, short};
use super::{OntoError, OntologyBackend, Principal, graph_entity, person_id};
use crate::app::AppState;
use crate::domain::ontology::Ontology;
use crate::repo::onto_projection as repo;

/// What the projection covers, in the order a backfill walks them
/// (containers first, so links mostly point at what is there already).
pub const KINDS: [&str; 9] = [
    "team",
    "person",
    "project",
    "cycle",
    "graph",
    "document",
    "issue",
    "agent",
    "run_started",
];
/// Ids per backfill page.
const PAGE: i64 = 500;

/// The relations the projection states (see `deploy/onto/nexc-workspace.toml`).
pub mod rel {
    pub const MEMBER_OF: &str = "member_of";
    pub const BELONGS_TO: &str = "belongs_to";
    pub const ASSIGNED_TO: &str = "assigned_to";
    pub const REPORTED_BY: &str = "reported_by";
    pub const PART_OF: &str = "part_of";
    pub const IN_CYCLE: &str = "in_cycle";
    pub const PLANNED_BY: &str = "planned_by";
    pub const LED_BY: &str = "led_by";
    pub const CYCLE_OF: &str = "cycle_of";
    pub const IN_TEAM: &str = "in_team";
    pub const CREATED_BY: &str = "created_by";
    pub const UPLOADED_BY: &str = "uploaded_by";
    pub const RUN_OF: &str = "run_of";
    pub const STARTED_BY: &str = "started_by";
}

pub fn team_id(id: Uuid) -> String {
    format!("team:{id}")
}

pub fn project_id(id: Uuid) -> String {
    format!("project:{id}")
}

pub fn cycle_id(id: Uuid) -> String {
    format!("cycle:{id}")
}

pub fn issue_id(id: Uuid) -> String {
    format!("issue:{id}")
}

/// One entity as nexc states it, with the links from it.
#[derive(Debug, Clone, PartialEq)]
pub struct Thing {
    pub id: String,
    pub kind: &'static str,
    pub title: String,
    pub state: Option<&'static str>,
    pub props: Value,
    /// `(rel, to)`.
    pub links: Vec<(&'static str, String)>,
    /// False when another writer owns the entity (agents: the audit's
    /// facts) and the projection only states its links.
    pub owned: bool,
}

impl Thing {
    fn new(id: String, kind: &'static str, title: String, props: Value) -> Self {
        Thing {
            id,
            kind,
            title,
            state: None,
            props,
            links: Vec::new(),
            owned: true,
        }
    }

    fn link(&mut self, rel: &'static str, to: Option<String>) {
        if let Some(to) = to {
            self.links.push((rel, to));
        }
    }

    /// The links as the tracking table keeps them (`rel to`).
    fn link_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .links
            .iter()
            .map(|(rel, to)| format!("{rel} {to}"))
            .collect();
        keys.sort();
        keys.dedup();
        keys
    }
}

/// Restricted to the members of `team` when it is private (and to the
/// workspace's owners and admins, who see every team in nexc); `null`s clear
/// a restriction that no longer holds.
fn visibility(props: &mut Value, private_team: Option<Uuid>) {
    if let Value::Object(map) = props {
        match private_team {
            Some(team) => {
                map.insert("visibility".into(), json!("restricted"));
                map.insert("acl".into(), json!([team_id(team), ADMINS]));
            }
            None => {
                map.insert("visibility".into(), Value::Null);
                map.insert("acl".into(), Value::Null);
            }
        }
    }
}

/// A project's status as onto-intelli's `project` states.
pub fn project_state(status: &str) -> &'static str {
    match status {
        "started" => "active",
        "completed" => "done",
        "canceled" => "cancelled",
        // paused and planned: not moving.
        _ => "planned",
    }
}

/// An issue's state category as onto-intelli's `issue` states.
pub fn issue_state(category: &str) -> &'static str {
    match category {
        "unstarted" => "triaged",
        "started" => "in_progress",
        "completed" => "resolved",
        "canceled" => "closed",
        _ => "open",
    }
}

/// nexc's priority (1 urgent … 4 low, 0 none) as onto-intelli's.
fn priority(value: i16) -> Value {
    match value {
        1 => json!("critical"),
        2 => json!("high"),
        3 => json!("medium"),
        4 => json!("low"),
        _ => Value::Null,
    }
}

/// The things of `kind` with these ids, and the entity ids of those that are gone.
async fn load(
    state: &AppState,
    workspace_id: Uuid,
    kind: &str,
    ids: &[Uuid],
) -> Result<(Vec<Thing>, Vec<String>), sqlx::Error> {
    let db = &state.db;
    let mut found: HashSet<Uuid> = HashSet::new();
    let mut things = Vec::new();
    let entity_of: fn(Uuid) -> String = match kind {
        "team" => team_id,
        "project" => project_id,
        "cycle" => cycle_id,
        "issue" => issue_id,
        "graph" => graph_entity,
        "document" => super::facts::document_id,
        "person" => person_id,
        "run_started" => super::facts::run_id,
        _ => |_| String::new(),
    };
    match kind {
        "team" => {
            for t in repo::teams(db, workspace_id, ids).await? {
                found.insert(t.id);
                let mut props = json!({"key": t.key, "private": t.private, "nexc_id": t.id});
                visibility(&mut props, t.private.then_some(t.id));
                things.push(Thing::new(team_id(t.id), "team", t.name, props));
            }
        }
        "person" => {
            let mut admins = false;
            for p in repo::people(db, workspace_id, ids).await? {
                found.insert(p.id);
                let mut thing = Thing::new(
                    person_id(p.id),
                    "person",
                    p.name,
                    json!({"role": p.role, "nexc_id": p.id}),
                );
                for team in p.teams {
                    thing.link(rel::MEMBER_OF, Some(team_id(team)));
                }
                if matches!(p.role.as_str(), "owner" | "admin") {
                    admins = true;
                    thing.link(rel::MEMBER_OF, Some(ADMINS.to_owned()));
                }
                things.push(thing);
            }
            if admins {
                // Shown to its own members only: nobody else can see it, so nobody else can
                // link themselves to it and read what it is on the acl of.
                things.push(Thing::new(
                    ADMINS.to_owned(),
                    "team",
                    "Workspace admins".to_owned(),
                    json!({"key": "ADMINS", "private": true, "group": "owners and admins",
                           "visibility": "restricted", "acl": [ADMINS]}),
                ));
            }
        }
        "project" => {
            for p in repo::projects(db, workspace_id, ids).await? {
                found.insert(p.id);
                let mut props = json!({"status": p.status, "nexc_id": p.id});
                visibility(&mut props, p.team_id.filter(|_| p.private));
                let mut thing = Thing::new(project_id(p.id), "project", p.name, props);
                thing.state = Some(project_state(&p.status));
                thing.link(rel::LED_BY, p.lead_id.map(person_id));
                thing.link(rel::BELONGS_TO, p.team_id.map(team_id));
                things.push(thing);
            }
        }
        "cycle" => {
            for c in repo::cycles(db, workspace_id, ids).await? {
                found.insert(c.id);
                let title = if c.name.trim().is_empty() {
                    format!("Cycle {}", c.number)
                } else {
                    c.name
                };
                let mut props = json!({
                    "number": c.number, "starts_on": c.starts_on, "ends_on": c.ends_on,
                    "nexc_id": c.id,
                });
                visibility(&mut props, c.private.then_some(c.team_id));
                let mut thing = Thing::new(cycle_id(c.id), "cycle", title, props);
                thing.link(rel::CYCLE_OF, Some(team_id(c.team_id)));
                things.push(thing);
            }
        }
        "issue" => {
            for i in repo::issues(db, workspace_id, ids).await? {
                found.insert(i.id);
                let identifier = format!("{}-{}", i.team_key, i.number);
                let mut props = json!({
                    "identifier": identifier, "status": i.state_name,
                    "priority": priority(i.priority), "nexc_id": i.id,
                });
                visibility(&mut props, i.private.then_some(i.team_id));
                let mut thing = Thing::new(
                    issue_id(i.id),
                    "issue",
                    format!("{identifier} {}", i.title),
                    props,
                );
                thing.state = Some(issue_state(&i.state_category));
                thing.link(rel::BELONGS_TO, Some(team_id(i.team_id)));
                thing.link(rel::BELONGS_TO, i.project_id.map(project_id));
                thing.link(rel::ASSIGNED_TO, i.assignee_id.map(person_id));
                thing.link(rel::ASSIGNED_TO, i.agent_principal.as_deref().map(agent_id));
                thing.link(rel::REPORTED_BY, i.creator_id.map(person_id));
                thing.link(rel::PART_OF, i.parent_id.map(issue_id));
                thing.link(rel::IN_CYCLE, i.cycle_id.map(cycle_id));
                thing.link(rel::PLANNED_BY, i.graph_id.map(graph_entity));
                things.push(thing);
            }
        }
        "graph" => {
            for g in repo::graphs(db, workspace_id, ids).await? {
                found.insert(g.id);
                let mut props = json!({"nexc_id": g.id});
                visibility(&mut props, g.team_id.filter(|_| g.private));
                let mut thing = Thing::new(graph_entity(g.id), "execution_graph", g.name, props);
                thing.link(rel::IN_TEAM, g.team_id.map(team_id));
                thing.link(rel::CREATED_BY, Some(person_id(g.owner_id)));
                things.push(thing);
            }
        }
        "document" => {
            for d in repo::documents(db, workspace_id, ids).await? {
                found.insert(d.id);
                let cleared = matches!(d.label.as_str(), "public" | "internal");
                let title = if cleared {
                    d.name
                } else {
                    format!("document {}", short(d.id))
                };
                let mut props = json!({"status": d.status, "label": d.label, "nexc_id": d.id});
                let restricted = d.label == "restricted";
                props["visibility"] = if restricted {
                    json!("restricted")
                } else {
                    Value::Null
                };
                props["acl"] = if restricted {
                    json!([ADMINS])
                } else {
                    Value::Null
                };
                let mut thing =
                    Thing::new(super::facts::document_id(d.id), "document", title, props);
                thing.link(rel::UPLOADED_BY, d.uploaded_by.map(person_id));
                things.push(thing);
            }
        }
        "run_started" => {
            for r in repo::runs(db, workspace_id, ids).await? {
                found.insert(r.id);
                let mut props =
                    json!({"graph_id": r.graph_id, "nexc_id": r.id, "started_at": r.started_at});
                visibility(&mut props, r.team_id.filter(|_| r.private));
                let mut thing = Thing::new(
                    super::facts::run_id(r.id),
                    "run",
                    format!("run {}", short(r.id)),
                    props,
                );
                thing.state = Some(match r.status.as_str() {
                    "running" => "running",
                    "succeeded" => "succeeded",
                    "failed" => "failed",
                    "cancelled" => "cancelled",
                    _ => "queued",
                });
                thing.link(rel::RUN_OF, Some(graph_entity(r.graph_id)));
                thing.link(rel::STARTED_BY, Some(person_id(r.owner_id)));
                things.push(thing);
            }
        }
        "agent" => {
            // The agent itself is the audit's (facts::Batch::agent); only its team is stated here.
            for a in repo::agents(db, workspace_id, ids).await? {
                found.insert(a.id);
                let mut thing =
                    Thing::new(agent_id(&a.principal_id), "agent", String::new(), json!({}));
                thing.owned = false;
                thing.link(rel::MEMBER_OF, a.team_id.map(team_id));
                things.push(thing);
            }
            return Ok((things, Vec::new()));
        }
        _ => return Ok((Vec::new(), Vec::new())),
    }
    let gone = ids
        .iter()
        .filter(|id| !found.contains(id))
        .map(|id| entity_of(*id))
        .collect();
    Ok((things, gone))
}

/// The records that state `things` and take back what no longer holds:
/// upserts first, then a bare upsert for every link target not stated in
/// the batch (so a link never points at nothing; it fills nothing that is
/// there), then the links, then the links that no longer hold.
pub fn records(
    workspace: &str,
    things: &[Thing],
    tracked: &std::collections::HashMap<String, Vec<String>>,
) -> Vec<Value> {
    let mut upserts = Vec::new();
    let mut stated: BTreeSet<&str> = BTreeSet::new();
    for thing in things.iter().filter(|t| t.owned) {
        let mut record = Map::new();
        record.insert("op".into(), json!("upsert"));
        record.insert("id".into(), json!(thing.id));
        record.insert("kind".into(), json!(thing.kind));
        record.insert("workspace".into(), json!(workspace));
        record.insert("title".into(), json!(thing.title));
        if let Some(state) = thing.state {
            record.insert("state".into(), json!(state));
        }
        record.insert("props".into(), thing.props.clone());
        upserts.push(Value::Object(record));
        stated.insert(&thing.id);
    }
    let mut ensures: BTreeMap<&str, Value> = BTreeMap::new();
    let mut links = Vec::new();
    let mut unlinks = Vec::new();
    for thing in things {
        let wanted = thing.link_keys();
        for (rel, to) in &thing.links {
            if !stated.contains(to.as_str()) && !ensures.contains_key(to.as_str()) {
                let kind = to.split_once(':').map(|(k, _)| k).unwrap_or_default();
                ensures.insert(
                    to,
                    json!({"op": "upsert", "id": to, "kind": kind, "workspace": workspace,
                           "fill_only": true}),
                );
            }
            links.push(json!({"op": "link", "from": thing.id, "rel": rel, "to": to,
                              "workspace": workspace}));
        }
        for old in tracked.get(&thing.id).into_iter().flatten() {
            if wanted.contains(old) {
                continue;
            }
            if let Some((rel, to)) = old.split_once(' ') {
                unlinks.push(
                    json!({"op": "unlink", "from": thing.id, "rel": rel, "to": to,
                                    "workspace": workspace}),
                );
            }
        }
    }
    // An agent stated only for its links must exist before them.
    for thing in things.iter().filter(|t| !t.owned) {
        if !thing.links.is_empty() && !ensures.contains_key(thing.id.as_str()) {
            ensures.insert(
                &thing.id,
                json!({"op": "upsert", "id": thing.id, "kind": thing.kind,
                       "workspace": workspace, "fill_only": true}),
            );
        }
    }
    let mut out = upserts;
    out.extend(ensures.into_values());
    out.extend(links);
    out.extend(unlinks);
    out
}

/// Projects `items` (`(kind, nexc id)`) of a workspace: what exists is
/// stated, what is gone is deleted where it was put. Returns the records sent.
pub async fn project(
    state: &AppState,
    backend: &dyn OntologyBackend,
    service: &Principal,
    workspace_id: Uuid,
    items: &[(String, Uuid)],
) -> anyhow::Result<usize> {
    let mut by_kind: BTreeMap<&str, Vec<Uuid>> = BTreeMap::new();
    for (kind, id) in items {
        by_kind.entry(kind.as_str()).or_default().push(*id);
    }
    let mut things = Vec::new();
    let mut gone = Vec::new();
    for kind in KINDS {
        let Some(ids) = by_kind.get_mut(kind) else {
            continue;
        };
        ids.sort_unstable();
        ids.dedup();
        let (found, missing) = load(state, workspace_id, kind, ids).await?;
        things.extend(found);
        gone.extend(missing);
    }
    send(state, backend, service, workspace_id, &things, &gone).await
}

async fn send(
    state: &AppState,
    backend: &dyn OntologyBackend,
    service: &Principal,
    workspace_id: Uuid,
    things: &[Thing],
    gone: &[String],
) -> anyhow::Result<usize> {
    let mut entities: Vec<String> = things.iter().map(|t| t.id.clone()).collect();
    entities.extend(gone.iter().cloned());
    let tracked = repo::tracked(&state.db, workspace_id, &entities).await?;
    let batch = records(&service.workspace, things, &tracked);
    let mut sent = batch.len();
    if !batch.is_empty() {
        backend.ingest(service, batch).await?;
    }
    let mut forgotten = Vec::new();
    for entity in gone.iter().filter(|g| tracked.contains_key(*g)) {
        sent += forget_in(backend, service, entity).await?;
        forgotten.push(entity.clone());
    }
    let kept: Vec<(String, Vec<String>)> = things
        .iter()
        .map(|t| (t.id.clone(), t.link_keys()))
        .collect();
    repo::record(&state.db, workspace_id, &kept).await?;
    repo::forget(&state.db, workspace_id, &forgotten).await?;
    Ok(sent)
}

/// Deletes an entity the projection put into onto-intelli (what it lost
/// already is not an error). A graph takes its vocabulary along: its node
/// and relation types are removed first (`SetVocabulary` with none), or they
/// would stay behind on their own. Returns the records sent.
async fn forget_in(
    backend: &dyn OntologyBackend,
    service: &Principal,
    entity: &str,
) -> Result<usize, OntoError> {
    let mut sent = 1;
    if let Some(graph) = entity.strip_prefix("execution_graph:") {
        match backend
            .set_vocabulary(service, graph, &Ontology::default())
            .await
        {
            Ok(_) | Err(OntoError::NotFound(_)) => sent += 1,
            Err(err) => return Err(err),
        }
    }
    match backend
        .ingest(service, vec![json!({"op": "delete", "id": entity})])
        .await
    {
        Ok(_) | Err(OntoError::NotFound(_)) => Ok(sent),
        Err(err) => Err(err),
    }
}

/// Entities of a retired tenant deleted per pass, so one large workspace
/// does not hold the export up.
const RETIRE_PER_PASS: usize = 500;

/// Clears the tenants of deleted workspaces (`onto_retired_tenants`): the
/// entities the projection put there and the vocabularies of their graphs,
/// as the backend's service principal, a bounded number per pass. What
/// remains in the tenant (the audit's facts and events, its schema and
/// proposals) goes when the operator drops its schema (`docs/ONTOLOGY.md`
/// §5). Returns the records sent.
pub async fn clear_retired(
    state: &AppState,
    backend: &dyn OntologyBackend,
) -> anyhow::Result<usize> {
    let prefix = &state.settings.onto_workspace_prefix;
    let mut sent = 0;
    let mut budget = RETIRE_PER_PASS;
    for retired in repo::retired(&state.db, 20).await? {
        let service = Principal::service(prefix, retired.workspace_id);
        let projected: HashSet<&str> = retired.entities.iter().map(String::as_str).collect();
        // The graphs first: deleting a graph's entity takes its vocabulary along.
        let mut entities: Vec<String> = retired
            .graphs
            .iter()
            .map(|g| graph_entity(*g))
            .filter(|g| !projected.contains(g.as_str()))
            .chain(retired.entities.iter().cloned())
            .collect();
        entities.sort_by_key(|e| !e.starts_with("execution_graph:"));
        let mut done = 0;
        let mut error = None;
        for entity in &entities {
            if budget == 0 {
                break;
            }
            match forget_in(backend, &service, entity).await {
                Ok(n) => sent += n,
                Err(err) => {
                    error = Some(err.to_string());
                    break;
                }
            }
            done += 1;
            budget -= 1;
        }
        if done == entities.len() {
            crate::repo::onto_export::drop_queue(&state.db, retired.workspace_id).await?;
            repo::mark_cleared(&state.db, retired.workspace_id).await?;
            tracing::info!(
                tenant = %super::tenant_of(retired.workspace_id),
                "a deleted workspace's entities were removed from onto-intelli; its schema (onto_{}) holds the audit's facts until the operator drops it",
                super::tenant_of(retired.workspace_id)
            );
        } else {
            let rest: HashSet<&str> = entities[done..].iter().map(String::as_str).collect();
            let left: Vec<String> = retired
                .entities
                .iter()
                .filter(|e| rest.contains(e.as_str()))
                .cloned()
                .collect();
            let graphs: Vec<Uuid> = retired
                .graphs
                .iter()
                .copied()
                .filter(|g| rest.contains(graph_entity(*g).as_str()))
                .collect();
            repo::retire_progress(
                &state.db,
                retired.workspace_id,
                &left,
                &graphs,
                error.as_deref(),
            )
            .await?;
            if let Some(error) = error {
                anyhow::bail!("clearing a deleted workspace's tenant failed: {error}");
            }
        }
        if budget == 0 {
            break;
        }
    }
    Ok(sent)
}

/// Projects a whole workspace, kind by kind, and records that it was.
pub async fn backfill(
    state: &AppState,
    backend: &dyn OntologyBackend,
    service: &Principal,
    workspace_id: Uuid,
) -> anyhow::Result<usize> {
    let mut sent = 0;
    for kind in KINDS {
        let mut after = None;
        loop {
            let ids = repo::ids(&state.db, workspace_id, kind, after, PAGE).await?;
            let Some(last) = ids.last().copied() else {
                break;
            };
            after = Some(last);
            let items: Vec<(String, Uuid)> = ids.iter().map(|id| (kind.to_owned(), *id)).collect();
            sent += project(state, backend, service, workspace_id, &items).await?;
            if (ids.len() as i64) < PAGE {
                break;
            }
        }
    }
    repo::mark_projected(&state.db, workspace_id).await?;
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn states_map_onto_the_enterprise_kinds() {
        assert_eq!(project_state("started"), "active");
        assert_eq!(project_state("paused"), "planned");
        assert_eq!(project_state("canceled"), "cancelled");
        assert_eq!(issue_state("backlog"), "open");
        assert_eq!(issue_state("started"), "in_progress");
        assert_eq!(issue_state("completed"), "resolved");
        assert_eq!(priority(1), json!("critical"));
        assert_eq!(priority(0), Value::Null);
    }

    #[test]
    fn records_state_links_after_their_ends_and_take_back_old_ones() {
        let mut issue = Thing::new(
            "issue:1".into(),
            "issue",
            "ENG-1 Fix".into(),
            json!({"nexc_id": "1"}),
        );
        issue.state = Some("open");
        issue.link(rel::ASSIGNED_TO, Some("person:a".into()));
        issue.link(rel::BELONGS_TO, Some("team:t".into()));
        let team = Thing::new("team:t".into(), "team", "Eng".into(), json!({}));
        let mut agent = Thing::new("agent:w/a".into(), "agent", String::new(), json!({}));
        agent.owned = false;
        agent.link(rel::MEMBER_OF, Some("team:t".into()));
        let tracked = HashMap::from([(
            "issue:1".to_owned(),
            vec![
                "assigned_to person:b".to_owned(),
                "belongs_to team:t".to_owned(),
            ],
        )]);
        let out = records("ws", &[issue, team, agent], &tracked);
        let ops: Vec<(&str, &str)> = out
            .iter()
            .map(|r| {
                (
                    r["op"].as_str().unwrap(),
                    r["id"].as_str().or(r["to"].as_str()).unwrap(),
                )
            })
            .collect();
        assert_eq!(
            ops,
            [
                ("upsert", "issue:1"),
                ("upsert", "team:t"),
                ("upsert", "agent:w/a"),
                ("upsert", "person:a"),
                ("link", "person:a"),
                ("link", "team:t"),
                ("link", "team:t"),
                ("unlink", "person:b"),
            ]
        );
        assert_eq!(out[3]["fill_only"], true, "a target is never overwritten");
        assert_eq!(out[2]["fill_only"], true, "the audit owns its agents");
        assert!(out[0].get("props").is_some() && out[0]["state"] == "open");
    }
}
