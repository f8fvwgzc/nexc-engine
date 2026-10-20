//! An onto-intelli that lives in memory: the facts it is given, the links
//! between them and the events it was told about, answering `find`,
//! `traverse` and `events` the way the server does for the audit's
//! questions, and `ask` the way a test tells it to. It keeps every graph's
//! execution vocabulary per tenant (validated as the `nexc-execution`
//! package does, read back in the server's order, and hidden from whoever
//! the graph's entity is restricted from, as the server's visibility rule
//! hides it), a small live schema
//! with its proposals (agents propose, never apply), and mints sessions.
//! Every call is logged with the principal it was made as, so tests can see
//! who nexc said it was. Tests use it directly as the backend, or behind
//! [`server`], an in-process server of the same gRPC services and session
//! endpoint that the real client talks to.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

use chrono::Utc;
use serde_json::{Map, Value, json};

use super::{
    AskAnswer, AskRequest, AskStream, AskStreamEvent, Direction, Entity, EventQuery, FindQuery,
    IngestOutcome, OntoError, OntoEvent, OntoHealth, OntoSession, OntologyBackend, Principal,
    SchemaChange, SchemaProposalDraft, Subgraph, TraverseQuery, role,
};
use crate::domain::graph::Executor;
use crate::domain::ontology::Ontology;

/// Answers a question; tests set one with [`StubOntology::answer_with`].
pub type Answerer = Box<dyn Fn(&Principal, &AskRequest) -> AskAnswer + Send + Sync>;

/// The kinds the stub's schema starts with.
const KINDS: [&str; 14] = [
    "person",
    "team",
    "project",
    "cycle",
    "issue",
    "agent",
    "execution_graph",
    "node_type",
    "relation_type",
    "document",
    "run",
    "node",
    "tool",
    "policy",
];

/// A tenant's schema: its version, object types and link types.
type Schema = (u64, Map<String, Value>, Map<String, Value>);

#[derive(Default)]
struct State {
    /// workspace → entity id → entity.
    entities: BTreeMap<String, BTreeMap<String, Entity>>,
    /// (workspace, from, rel, to).
    links: BTreeSet<(String, String, String, String)>,
    events: Vec<(String, OntoEvent)>,
    /// Events before these (a batch is staged apart, then appended).
    event_offset: usize,
    /// Every record ingested, as given, with its workspace.
    records: Vec<(String, Value)>,
    ingests: u64,
    asks: Vec<(Principal, AskRequest)>,
    /// (tenant, graph key) → vocabulary.
    vocabularies: BTreeMap<(String, String), Ontology>,
    /// tenant → (schema version, extra object types and link types).
    schemas: BTreeMap<String, Schema>,
    /// tenant → proposals, oldest first.
    proposals: BTreeMap<String, Vec<Value>>,
    /// Every call: (method, principal).
    calls: Vec<(String, Principal)>,
    failure: Option<OntoError>,
    /// Failures for the next calls to one method: (method, calls left, error).
    failures_next: Vec<(String, usize, OntoError)>,
    answerer: Option<Answerer>,
}

/// The in-memory onto-intelli.
pub struct StubOntology {
    state: Mutex<State>,
}

impl Default for StubOntology {
    fn default() -> Self {
        Self::new()
    }
}

impl StubOntology {
    pub fn new() -> Self {
        StubOntology {
            state: Mutex::new(State::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Every call fails with `error` until called again with `None`.
    pub fn fail_with(&self, error: Option<OntoError>) {
        self.lock().failure = error;
    }

    /// The next `times` calls to `method` fail with `error`; the others go through.
    pub fn fail_next(&self, method: &str, times: usize, error: OntoError) {
        self.lock()
            .failures_next
            .push((method.to_owned(), times, error));
    }

    /// How `ask` answers from now on.
    pub fn answer_with(&self, answerer: Answerer) {
        self.lock().answerer = Some(answerer);
    }

    /// Everything ingested so far, as given.
    pub fn records(&self) -> Vec<Value> {
        self.lock().records.iter().map(|(_, r)| r.clone()).collect()
    }

    /// Ingest calls so far.
    pub fn ingests(&self) -> u64 {
        self.lock().ingests
    }

    /// The questions asked so far.
    pub fn asks(&self) -> Vec<(Principal, AskRequest)> {
        self.lock().asks.clone()
    }

    /// Every call so far, as `(method, principal)`.
    pub fn calls(&self) -> Vec<(String, Principal)> {
        self.lock().calls.clone()
    }

    /// The principals `method` was called as.
    pub fn calls_to(&self, method: &str) -> Vec<Principal> {
        self.lock()
            .calls
            .iter()
            .filter(|(m, _)| m == method)
            .map(|(_, p)| p.clone())
            .collect()
    }

    pub fn entity(&self, workspace: &str, id: &str) -> Option<Entity> {
        self.lock().entities.get(workspace)?.get(id).cloned()
    }

    /// Ids of the entities of a workspace.
    pub fn entity_ids(&self, workspace: &str) -> Vec<String> {
        self.lock()
            .entities
            .get(workspace)
            .map(|e| e.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn has_link(&self, workspace: &str, from: &str, rel: &str, to: &str) -> bool {
        self.lock()
            .links
            .contains(&(workspace.into(), from.into(), rel.into(), to.into()))
    }

    /// The links of a workspace as `(from, rel, to)`.
    pub fn links(&self, workspace: &str) -> Vec<(String, String, String)> {
        self.lock()
            .links
            .iter()
            .filter(|(ws, ..)| ws == workspace)
            .map(|(_, f, r, t)| (f.clone(), r.clone(), t.clone()))
            .collect()
    }

    pub fn events_of(&self, workspace: &str) -> Vec<OntoEvent> {
        self.lock()
            .events
            .iter()
            .filter(|(ws, _)| ws == workspace)
            .map(|(_, e)| e.clone())
            .collect()
    }

    /// The vocabulary a tenant holds for a graph, as stored.
    pub fn stored_vocabulary(&self, tenant: &str, graph: &str) -> Option<Ontology> {
        self.lock()
            .vocabularies
            .get(&(tenant.to_owned(), graph_key(graph).to_owned()))
            .cloned()
    }

    /// Writes a vocabulary as if someone had edited it in onto-intelli.
    pub fn put_vocabulary(&self, tenant: &str, graph: &str, vocabulary: Ontology) {
        self.lock().vocabularies.insert(
            (tenant.to_owned(), graph_key(graph).to_owned()),
            sorted(vocabulary),
        );
    }

    /// The proposals filed in a tenant, oldest first.
    pub fn proposals_of(&self, tenant: &str) -> Vec<Value> {
        self.lock()
            .proposals
            .get(tenant)
            .cloned()
            .unwrap_or_default()
    }

    /// Forgets everything (not the configured failure or answerer).
    pub fn clear(&self) {
        let mut state = self.lock();
        state.entities.clear();
        state.links.clear();
        state.events.clear();
        state.records.clear();
        state.vocabularies.clear();
        state.proposals.clear();
        state.schemas.clear();
        state.calls.clear();
    }

    fn has_entity(state: &State, workspace: &str, id: &str) -> bool {
        state
            .entities
            .get(workspace)
            .is_some_and(|e| e.contains_key(id))
    }

    fn check(state: &State) -> Result<(), OntoError> {
        match &state.failure {
            Some(err) => Err(err.clone()),
            None => Ok(()),
        }
    }

    /// Logs the call and fails it when a failure is set.
    fn enter(
        &self,
        method: &str,
        principal: &Principal,
    ) -> Result<std::sync::MutexGuard<'_, State>, OntoError> {
        let mut state = self.lock();
        state.calls.push((method.to_owned(), principal.clone()));
        Self::check(&state)?;
        if let Some((_, left, error)) = state
            .failures_next
            .iter_mut()
            .find(|(m, left, _)| m == method && *left > 0)
        {
            *left -= 1;
            return Err(error.clone());
        }
        Ok(state)
    }

    fn apply(state: &mut State, workspace: &str, record: &Value) -> Result<(), OntoError> {
        let op = record.get("op").and_then(Value::as_str);
        let has_identity = record.get("id").is_some() || record.get("kind").is_some();
        match op {
            Some("upsert") => Self::upsert(state, workspace, record),
            None if has_identity => Self::upsert(state, workspace, record),
            Some("link") => {
                let (from, rel, to) = link_of(record)?;
                state.links.insert((workspace.into(), from, rel, to));
                Ok(())
            }
            Some("unlink") => {
                let (from, rel, to) = link_of(record)?;
                state.links.remove(&(workspace.into(), from, rel, to));
                Ok(())
            }
            Some("delete") => {
                let id = text(record, "id")
                    .ok_or_else(|| OntoError::Invalid("delete without id".into()))?;
                let removed = state
                    .entities
                    .get_mut(workspace)
                    .and_then(|e| e.remove(&id));
                if removed.is_none() {
                    // As the server does: deleting what is not there fails the batch.
                    return Err(OntoError::NotFound(id));
                }
                state
                    .links
                    .retain(|(ws, from, _, to)| ws != workspace || (from != &id && to != &id));
                Ok(())
            }
            Some("event") => {
                let action = record
                    .get("action")
                    .and_then(Value::as_str)
                    .ok_or_else(|| OntoError::Invalid("event without action".into()))?;
                let n = state.event_offset + state.events.len() + 1;
                let event = OntoEvent {
                    id: format!("evt:{n}"),
                    ts: record
                        .get("ts")
                        .and_then(Value::as_i64)
                        .unwrap_or_else(|| Utc::now().timestamp_millis()),
                    actor: text(record, "actor").unwrap_or_default(),
                    action: action.to_owned(),
                    target: text(record, "target").unwrap_or_default(),
                    outcome: text(record, "outcome").unwrap_or_else(|| "success".into()),
                    payload: record.get("payload").cloned().unwrap_or(Value::Null),
                };
                state.events.push((workspace.into(), event));
                Ok(())
            }
            Some(other) => Err(OntoError::Invalid(format!("unknown op `{other}`"))),
            None => Err(OntoError::Invalid(
                "unrecognized record: expected a mutation (op) or an entity (id/kind)".into(),
            )),
        }
    }

    fn upsert(state: &mut State, workspace: &str, record: &Value) -> Result<(), OntoError> {
        let id = match text(record, "id") {
            Some(id) => id,
            None => {
                let kind = text(record, "kind")
                    .ok_or_else(|| OntoError::Invalid("entity without kind".into()))?;
                let key = text(record, "key")
                    .ok_or_else(|| OntoError::Invalid("entity without id or key".into()))?;
                format!("{kind}:{key}")
            }
        };
        let kind = text(record, "kind")
            .or_else(|| id.split_once(':').map(|(k, _)| k.to_owned()))
            .unwrap_or_default();
        let entities = state.entities.entry(workspace.into()).or_default();
        let existed = entities.contains_key(&id);
        // `fill_only`, as the server does: only what is empty is filled.
        let fill = existed
            && record
                .get("fill_only")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        let entity = entities.entry(id.clone()).or_insert_with(|| Entity {
            id: id.clone(),
            kind: kind.clone(),
            props: json!({}),
            ..Entity::default()
        });
        if let Some(title) = text(record, "title")
            && (!fill || entity.title.is_empty())
        {
            entity.title = title;
        }
        if let Some(state_) = text(record, "state")
            && (!fill || entity.state.is_empty())
        {
            entity.state = state_;
        }
        if let Some(Value::Object(given)) = record.get("props") {
            let replace = record
                .get("replace")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut props = if replace {
                Map::new()
            } else {
                entity.props.as_object().cloned().unwrap_or_default()
            };
            for (k, v) in given {
                if v.is_null() {
                    // A null takes the property away.
                    props.remove(k);
                } else if !fill || !props.contains_key(k) {
                    props.insert(k.clone(), v.clone());
                }
            }
            entity.props = Value::Object(props);
        }
        if let Some(Value::Array(links)) = record.get("links") {
            for spec in links {
                let rel = text(spec, "rel")
                    .ok_or_else(|| OntoError::Invalid("link without rel".into()))?;
                let to =
                    text(spec, "to").ok_or_else(|| OntoError::Invalid("link without to".into()))?;
                state.links.insert((workspace.into(), id.clone(), rel, to));
            }
        }
        Ok(())
    }

    fn schema_of<'a>(state: &'a mut State, tenant: &str) -> &'a mut Schema {
        state.schemas.entry(tenant.to_owned()).or_insert_with(|| {
            let kinds = KINDS
                .iter()
                .map(|k| ((*k).to_owned(), json!({ "label": k, "properties": {} })))
                .collect();
            let links = [
                ("member_of", "person", "team"),
                ("defined_in", "node_type", "execution_graph"),
                ("ran", "agent", "run"),
            ]
            .iter()
            .map(|(name, from, to)| {
                (
                    (*name).to_owned(),
                    json!({ "from": [from], "to": [to], "cardinality": "many" }),
                )
            })
            .collect();
            (1, kinds, links)
        })
    }

    /// The plan the server would give: every change needs an `op` and a
    /// name; deleting a kind that entities still have is breaking.
    fn plan(state: &mut State, tenant: &str, changes: &[Value]) -> Result<Value, OntoError> {
        let (version, kinds, links) = Self::schema_of(state, tenant).clone();
        let mut planned = Vec::new();
        let mut breaking = false;
        for change in changes {
            let op = change
                .get("op")
                .and_then(Value::as_str)
                .ok_or_else(|| OntoError::Invalid("every change needs an `op`".into()))?;
            let name = text(change, "name")
                .or_else(|| text(change, "object_type"))
                .or_else(|| text(change, "from"))
                .ok_or_else(|| OntoError::Invalid(format!("{op}: a name is required")))?;
            if !valid_key(&name) {
                return Err(OntoError::Invalid(format!(
                    "{op}: `{name}` must match [a-z][a-z0-9_]*"
                )));
            }
            let change_breaks = match op {
                "delete_object_type" => kinds.contains_key(&name),
                "delete_link_type" => links.contains_key(&name),
                "put_object_type" | "put_link_type" | "put_property" | "put_action_type"
                | "put_interface" | "put_rule" | "put_metric" | "put_glossary_term"
                | "delete_property" | "delete_action_type" | "rename_object_type"
                | "rename_property" | "rename_link_type" | "set_analysis" => false,
                other => return Err(OntoError::Invalid(format!("unknown op `{other}`"))),
            };
            breaking |= change_breaks;
            planned.push(json!({
                "op": op, "target": name, "summary": format!("{op} {name}"),
                "breaking": change_breaks, "entities": 0, "links": 0,
            }));
        }
        Ok(json!({
            "base_version": version, "valid": true, "breaking": breaking,
            "needs_migration": [], "warnings": [], "changes": planned,
            "entities_affected": 0, "links_affected": 0,
        }))
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn link_of(record: &Value) -> Result<(String, String, String), OntoError> {
    match (
        text(record, "from"),
        text(record, "rel"),
        text(record, "to"),
    ) {
        (Some(from), Some(rel), Some(to)) => Ok((from, rel, to)),
        _ => Err(OntoError::Invalid("link needs from, rel and to".into())),
    }
}

/// Whether every property of `wanted` is on `entity` with the same value.
fn props_match(entity: &Entity, wanted: &Value) -> bool {
    match wanted {
        Value::Object(map) => map.iter().all(|(k, v)| entity.props.get(k) == Some(v)),
        _ => true,
    }
}

/// `g1` for `g1` and `execution_graph:g1`.
fn graph_key(graph: &str) -> &str {
    graph.strip_prefix("execution_graph:").unwrap_or(graph)
}

fn valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// The server's order: node types by stage then key, relation types by key.
fn sorted(mut vocabulary: Ontology) -> Ontology {
    vocabulary
        .node_types
        .sort_by(|a, b| (a.stage, &a.key).cmp(&(b.stage, &b.key)));
    vocabulary.relation_types.sort_by(|a, b| a.key.cmp(&b.key));
    vocabulary
}

/// The `nexc-execution` package's rules: what the server refuses.
fn validate(vocabulary: &Ontology) -> Result<(), OntoError> {
    let mut keys = BTreeSet::new();
    for t in &vocabulary.node_types {
        let field = |rule: &str| OntoError::Invalid(format!("node_type.{rule} ({})", t.key));
        if !valid_key(&t.key) || t.key.len() > 40 {
            return Err(field(
                "key must match ^[a-z][a-z0-9_]*$, at most 40 characters",
            ));
        }
        if t.label.chars().count() > 60 || t.description.chars().count() > 500 {
            return Err(field("label ≤ 60, description ≤ 500 characters"));
        }
        if !t.color.is_empty()
            && !(t.color.len() == 7
                && t.color.starts_with('#')
                && t.color[1..].chars().all(|c| c.is_ascii_hexdigit()))
        {
            return Err(field("color must be #rrggbb"));
        }
        if !matches!(
            t.default_executor,
            Executor::Llm | Executor::Agent | Executor::Symphony
        ) {
            return Err(field("default_executor"));
        }
        if !keys.insert(("n", t.key.clone())) {
            return Err(OntoError::Invalid(format!(
                "node type key '{}' appears twice",
                t.key
            )));
        }
    }
    for t in &vocabulary.relation_types {
        if !valid_key(&t.key) || t.key.len() > 40 || t.label.chars().count() > 60 {
            return Err(OntoError::Invalid(format!("relation_type ({})", t.key)));
        }
        if !keys.insert(("r", t.key.clone())) {
            return Err(OntoError::Invalid(format!(
                "relation type key '{}' appears twice",
                t.key
            )));
        }
    }
    Ok(())
}

/// Whether `principal` sees the entity `id` of its workspace, as the server's
/// `policy::visible` decides: the backend's own principal (an admin) and
/// admins see everything; a `restricted` (or `private`) entity only shows to
/// whoever its `acl` names, directly or through a team they are `member_of`.
/// An entity that is not there restricts nothing.
fn sees(state: &State, principal: &Principal, id: &str) -> bool {
    if !principal.acts() || principal.has_role("admin") || principal.has_role("system") {
        return true;
    }
    let Some(entity) = state
        .entities
        .get(&principal.workspace)
        .and_then(|e| e.get(id))
    else {
        return true;
    };
    if !matches!(
        entity.prop_str("visibility"),
        Some("restricted" | "private")
    ) {
        return true;
    }
    let acl: Vec<&str> = entity
        .props
        .get("acl")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let me = principal.entity.as_deref().unwrap_or(&principal.id);
    acl.contains(&me)
        || acl.contains(&principal.id.as_str())
        || state.links.iter().any(|(ws, from, rel, to)| {
            ws == &principal.workspace
                && from == me
                && rel == "member_of"
                && acl.contains(&to.as_str())
        })
}

/// What the server's `Write` access needs: anything but a viewer acting.
fn may_write(principal: &Principal) -> Result<(), OntoError> {
    if principal.acts()
        && !principal
            .roles
            .iter()
            .any(|r| r == role::MEMBER || r == role::AGENT || r == "admin")
    {
        return Err(OntoError::Denied(format!(
            "{} may not write in {}",
            principal.id, principal.workspace
        )));
    }
    Ok(())
}

#[async_trait::async_trait]
impl OntologyBackend for StubOntology {
    fn location(&self) -> String {
        "stub://in-memory".into()
    }

    async fn health(&self) -> Result<OntoHealth, OntoError> {
        Self::check(&self.lock())?;
        Ok(OntoHealth {
            status: "ok".into(),
            version: "stub".into(),
            revision: 0,
            llm: "none".into(),
            llm_available: false,
        })
    }

    async fn ingest(
        &self,
        principal: &Principal,
        records: Vec<Value>,
    ) -> Result<IngestOutcome, OntoError> {
        let mut state = self.enter("ingest", principal)?;
        may_write(principal)?;
        state.ingests += 1;
        // Applied atomically: a record that fails leaves nothing of the batch.
        let mut staged = State {
            entities: state.entities.clone(),
            links: state.links.clone(),
            event_offset: state.events.len(),
            ..State::default()
        };
        for record in &records {
            Self::apply(&mut staged, &principal.workspace, record)?;
        }
        state.entities = staged.entities;
        state.links = staged.links;
        state.events.extend(staged.events);
        for record in &records {
            state
                .records
                .push((principal.workspace.clone(), record.clone()));
        }
        Ok(IngestOutcome {
            accepted: records.len() as u64,
            queued: false,
        })
    }

    async fn ask(&self, principal: &Principal, req: AskRequest) -> Result<AskAnswer, OntoError> {
        let mut state = self.enter("ask", principal)?;
        state.asks.push((principal.clone(), req.clone()));
        Ok(match &state.answerer {
            Some(answer) => answer(principal, &req),
            None => AskAnswer {
                answer: "The stub has no answer configured.".into(),
                mode: "deterministic".into(),
                intent: "lookup".into(),
                ..AskAnswer::default()
            },
        })
    }

    async fn ask_stream(
        &self,
        principal: &Principal,
        req: AskRequest,
    ) -> Result<AskStream, OntoError> {
        let answer = self.ask(principal, req).await?;
        let events = vec![
            AskStreamEvent::Stage("intent".into()),
            AskStreamEvent::Entities(answer.entities.clone()),
            AskStreamEvent::Delta(answer.answer.clone()),
            AskStreamEvent::Done(answer),
        ];
        Ok(Box::pin(futures::stream::iter(events)))
    }

    async fn find(&self, principal: &Principal, q: FindQuery) -> Result<Vec<Entity>, OntoError> {
        let state = self.enter("find", principal)?;
        let limit = if q.limit == 0 { 50 } else { q.limit as usize };
        Ok(state
            .entities
            .get(&principal.workspace)
            .into_iter()
            .flat_map(|e| e.values())
            .filter(|e| q.kind.is_empty() || e.kind == q.kind)
            .filter(|e| q.state.is_empty() || q.state.contains(&e.state))
            .filter(|e| props_match(e, &q.props))
            .take(limit)
            .cloned()
            .collect())
    }

    async fn traverse(
        &self,
        principal: &Principal,
        q: TraverseQuery,
    ) -> Result<Subgraph, OntoError> {
        let state = self.enter("traverse", principal)?;
        let ws = &principal.workspace;
        let limit = if q.limit == 0 { 100 } else { q.limit as usize };
        let mut seen: BTreeSet<String> = q.start.iter().cloned().collect();
        let mut queue: VecDeque<(String, u32)> = q.start.iter().map(|s| (s.clone(), 0)).collect();
        let mut edges: BTreeSet<(String, String, String)> = BTreeSet::new();
        let mut order: Vec<String> = q.start.clone();
        let mut truncated = false;
        while let Some((id, depth)) = queue.pop_front() {
            if depth >= q.depth {
                continue;
            }
            for (lws, from, rel, to) in &state.links {
                if lws != ws || (!q.rels.is_empty() && !q.rels.contains(rel)) {
                    continue;
                }
                let next = match q.direction {
                    Direction::Out if from == &id => to,
                    Direction::In if to == &id => from,
                    Direction::Both if from == &id => to,
                    Direction::Both if to == &id => from,
                    _ => continue,
                };
                edges.insert((from.clone(), rel.clone(), to.clone()));
                if seen.insert(next.clone()) {
                    if order.len() >= limit {
                        truncated = true;
                        continue;
                    }
                    order.push(next.clone());
                    queue.push_back((next.clone(), depth + 1));
                }
            }
        }
        let entities = state.entities.get(ws);
        let nodes = order
            .into_iter()
            .map(|id| {
                entities
                    .and_then(|e| e.get(&id).cloned())
                    .unwrap_or_else(|| Entity {
                        kind: id
                            .split_once(':')
                            .map(|(k, _)| k.to_owned())
                            .unwrap_or_default(),
                        id,
                        ..Entity::default()
                    })
            })
            .filter(|e| q.kinds.is_empty() || q.kinds.contains(&e.kind))
            .collect();
        Ok(Subgraph {
            nodes,
            edges: edges
                .into_iter()
                .map(|(from, rel, to)| super::Edge { from, rel, to })
                .collect(),
            truncated,
        })
    }

    async fn events(
        &self,
        principal: &Principal,
        q: EventQuery,
    ) -> Result<Vec<OntoEvent>, OntoError> {
        let state = self.enter("events", principal)?;
        let limit = if q.limit == 0 { 50 } else { q.limit as usize };
        let prefix = q
            .action
            .as_ref()
            .map(|a| format!("{}.", a.trim_end_matches('.')));
        let mut found: Vec<OntoEvent> = state
            .events
            .iter()
            .filter(|(ws, _)| ws == &principal.workspace)
            .map(|(_, e)| e)
            .filter(|e| q.actor.as_ref().is_none_or(|a| &e.actor == a))
            .filter(|e| q.target.as_ref().is_none_or(|t| &e.target == t))
            .filter(|e| {
                q.action.as_ref().is_none_or(|a| {
                    &e.action == a || prefix.as_ref().is_some_and(|p| e.action.starts_with(p))
                })
            })
            .filter(|e| q.since_ms.is_none_or(|s| e.ts >= s))
            .filter(|e| q.until_ms.is_none_or(|u| e.ts <= u))
            .cloned()
            .collect();
        found.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| b.id.cmp(&a.id)));
        found.truncate(limit);
        Ok(found)
    }

    async fn vocabulary(&self, principal: &Principal, graph: &str) -> Result<Ontology, OntoError> {
        let state = self.enter("vocabulary", principal)?;
        let entity = format!("execution_graph:{}", graph_key(graph));
        // As the server: a graph the principal may not see is not there for them.
        if !sees(&state, principal, &entity) {
            return Err(OntoError::NotFound(entity));
        }
        match state
            .vocabularies
            .get(&(principal.tenant.clone(), graph_key(graph).to_owned()))
        {
            Some(vocabulary) => Ok(vocabulary.clone()),
            // As the server: the graph's entity is there (the projection stated it), its
            // types are not.
            None if Self::has_entity(&state, &principal.workspace, &entity) => {
                Ok(Ontology::default())
            }
            None => Err(OntoError::NotFound(entity)),
        }
    }

    async fn set_vocabulary(
        &self,
        principal: &Principal,
        graph: &str,
        vocabulary: &Ontology,
    ) -> Result<Ontology, OntoError> {
        let mut state = self.enter("set_vocabulary", principal)?;
        may_write(principal)?;
        validate(vocabulary)?;
        let key = (principal.tenant.clone(), graph_key(graph).to_owned());
        let entity = format!("execution_graph:{}", graph_key(graph));
        let known = state.vocabularies.contains_key(&key)
            || Self::has_entity(&state, &principal.workspace, &entity);
        if !known || !sees(&state, principal, &entity) {
            return Err(OntoError::NotFound(format!(
                "execution_graph:{}",
                graph_key(graph)
            )));
        }
        let stored = sorted(vocabulary.clone());
        state.vocabularies.insert(key, stored.clone());
        Ok(stored)
    }

    async fn init_graph(
        &self,
        principal: &Principal,
        graph: &str,
        _title: &str,
        vocabulary: Option<&Ontology>,
    ) -> Result<Ontology, OntoError> {
        let mut state = self.enter("init_graph", principal)?;
        may_write(principal)?;
        if let Some(vocabulary) = vocabulary {
            validate(vocabulary)?;
        }
        let entity = format!("execution_graph:{}", graph_key(graph));
        if !sees(&state, principal, &entity) {
            return Err(OntoError::Denied(format!("{entity} is restricted")));
        }
        let key = (principal.tenant.clone(), graph_key(graph).to_owned());
        let current = state.vocabularies.entry(key).or_default();
        // Upserts the given types; types already there and not given stay.
        if let Some(vocabulary) = vocabulary {
            for t in &vocabulary.node_types {
                current.node_types.retain(|c| c.key != t.key);
                current.node_types.push(t.clone());
            }
            for t in &vocabulary.relation_types {
                current.relation_types.retain(|c| c.key != t.key);
                current.relation_types.push(t.clone());
            }
        }
        *current = sorted(current.clone());
        Ok(current.clone())
    }

    async fn schema(&self, principal: &Principal) -> Result<Value, OntoError> {
        let mut state = self.enter("schema", principal)?;
        let (version, kinds, links) = Self::schema_of(&mut state, &principal.tenant).clone();
        Ok(json!({
            "schema_version": version,
            "object_types": kinds,
            "link_types": links,
            "action_types": {
                "define_node_type": { "risk": "medium" },
                "define_relation_type": { "risk": "low" },
                "retire_execution_type": { "risk": "high" },
            },
        }))
    }

    async fn plan_schema(
        &self,
        principal: &Principal,
        change: &SchemaChange,
    ) -> Result<Value, OntoError> {
        let mut state = self.enter("plan_schema", principal)?;
        Self::plan(&mut state, &principal.tenant, &change.changes)
    }

    async fn propose_schema(
        &self,
        principal: &Principal,
        proposal: &SchemaProposalDraft,
    ) -> Result<Value, OntoError> {
        let mut state = self.enter("propose_schema", principal)?;
        if proposal.title.trim().is_empty() || proposal.changes.is_empty() {
            return Err(OntoError::Invalid(
                "a proposal needs a title and at least one change".into(),
            ));
        }
        let plan = Self::plan(&mut state, &principal.tenant, &proposal.changes)?;
        let filed = state.proposals.entry(principal.tenant.clone()).or_default();
        let now = Utc::now().timestamp_millis();
        let value = json!({
            "id": format!("prop_{:04}", filed.len() + 1),
            "title": proposal.title,
            "rationale": proposal.rationale,
            "change_set": { "changes": proposal.changes, "migrations": proposal.migrations },
            "proposer": {
                "id": principal.id,
                "roles": principal.roles,
                "agent": principal.has_role(role::AGENT),
                "entity": principal.entity,
            },
            "status": "open",
            "base_version": plan["base_version"],
            "plan": plan,
            "reviews": [],
            "source": proposal.source,
            "workspace": principal.workspace,
            "created_at": now,
            "updated_at": now,
            "revision": 1,
        });
        filed.push(value.clone());
        Ok(value)
    }

    async fn proposals(
        &self,
        principal: &Principal,
        status: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Value>, OntoError> {
        let state = self.enter("proposals", principal)?;
        let limit = if limit == 0 { 100 } else { limit as usize };
        Ok(state
            .proposals
            .get(&principal.tenant)
            .into_iter()
            .flatten()
            .rev()
            .filter(|p| status.is_none_or(|s| p["status"] == s))
            .take(limit)
            .cloned()
            .collect())
    }

    async fn mint_session(
        &self,
        principal: &Principal,
        ttl_secs: u64,
    ) -> Result<OntoSession, OntoError> {
        let _state = self.enter("mint_session", principal)?;
        if principal.has_role("admin") {
            return Err(OntoError::Denied(
                "the backend's token may not grant role admin".into(),
            ));
        }
        let ttl = ttl_secs.clamp(60, 12 * 3600) as i64;
        Ok(OntoSession {
            token: format!("onto1.stub.{}", principal.id.replace(':', "-")),
            expires_at: Utc::now().timestamp() + ttl,
            tenant: principal.tenant.clone(),
        })
    }
}

pub mod server {
    //! The stub behind the real wire: an in-process server with the
    //! `onto_intelli.v1` gRPC services the backend uses and the HTTP session
    //! endpoint, on one port as `onto serve` has them, so the client is
    //! exercised end to end (metadata, streaming, status codes, the session
    //! request) without a real onto-intelli.

    use std::net::SocketAddr;
    use std::sync::Arc;

    use axum::Json;
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::{IntoResponse, Response};
    use futures::StreamExt;
    use serde_json::{Value, json};
    use tokio::sync::oneshot;
    use tokio::task::JoinHandle;
    use tonic::metadata::MetadataMap;
    use tonic::{Request, Status, Streaming};

    use super::StubOntology;
    use crate::domain::ontology::Ontology;
    use crate::ontology::grpc::{convert, header, principal_of, status_of};
    use crate::ontology::pb::admin_service_server::{AdminService, AdminServiceServer};
    use crate::ontology::pb::event_service_server::{EventService, EventServiceServer};
    use crate::ontology::pb::execution_service_server::{ExecutionService, ExecutionServiceServer};
    use crate::ontology::pb::intelli_service_server::{IntelliService, IntelliServiceServer};
    use crate::ontology::pb::ontology_service_server::{OntologyService, OntologyServiceServer};
    use crate::ontology::pb::schema_service_server::{SchemaService, SchemaServiceServer};
    use crate::ontology::{
        AskStreamEvent, OntoError, OntologyBackend, Principal, SchemaChange, SchemaProposalDraft,
        json, pb,
    };

    type Reply<T> = Result<tonic::Response<T>, Status>;

    /// The services, sharing one stub.
    #[derive(Clone)]
    pub struct StubServer {
        stub: Arc<StubOntology>,
        /// The bearer token callers must present, when set.
        token: Option<String>,
    }

    /// A running stub server.
    pub struct Running {
        pub addr: SocketAddr,
        stop: Option<oneshot::Sender<()>>,
        task: JoinHandle<()>,
    }

    impl Running {
        /// `http://127.0.0.1:{port}`, for `NEXC_ONTO_URL`.
        pub fn url(&self) -> String {
            format!("http://{}", self.addr)
        }

        /// Stops the server.
        pub async fn stop(mut self) {
            if let Some(stop) = self.stop.take() {
                let _ = stop.send(());
            }
            let _ = (&mut self.task).await;
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            if let Some(stop) = self.stop.take() {
                let _ = stop.send(());
            }
        }
    }

    /// Serves `stub` on a free local port: gRPC and the session endpoint.
    pub async fn serve(stub: Arc<StubOntology>, token: Option<&str>) -> std::io::Result<Running> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = StubServer {
            stub,
            token: token.map(str::to_owned),
        };
        let sessions = server.clone();
        let http = axum::Router::new().route(
            "/v1/auth/session",
            axum::routing::post(move |headers: HeaderMap, body: Json<Value>| {
                let server = sessions.clone();
                async move { server.session(&headers, body.0).await }
            }),
        );
        let routes = tonic::service::Routes::from(http)
            .add_service(AdminServiceServer::new(server.clone()))
            .add_service(EventServiceServer::new(server.clone()))
            .add_service(IntelliServiceServer::new(server.clone()))
            .add_service(ExecutionServiceServer::new(server.clone()))
            .add_service(SchemaServiceServer::new(server.clone()))
            .add_service(OntologyServiceServer::new(server));
        let (stop_tx, stop_rx) = oneshot::channel();
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        let task = tokio::spawn(async move {
            let result = tonic::transport::Server::builder()
                .accept_http1(true)
                .add_routes(routes)
                .serve_with_incoming_shutdown(incoming, async {
                    let _ = stop_rx.await;
                })
                .await;
            if let Err(err) = result {
                tracing::error!(error = %err, "stub onto-intelli server failed");
            }
        });
        Ok(Running {
            addr,
            stop: Some(stop_tx),
            task,
        })
    }

    impl StubServer {
        fn authenticated(&self, presented: Option<&str>) -> Result<(), Status> {
            if let Some(expected) = &self.token
                && presented.and_then(|v| v.strip_prefix("Bearer ")) != Some(expected.as_str())
            {
                return Err(Status::unauthenticated("invalid token"));
            }
            Ok(())
        }

        /// Who the call is made as: the token, then the impersonation headers.
        fn principal(&self, metadata: &MetadataMap) -> Result<Principal, Status> {
            self.authenticated(metadata.get("authorization").and_then(|v| v.to_str().ok()))?;
            let mut principal = principal_of(metadata);
            if principal.workspace.is_empty() {
                principal.workspace = "default".into();
            }
            if principal.acts() && principal.has_role("admin") {
                return Err(Status::permission_denied(
                    "nexc-backend may not grant role admin (impersonate_roles)",
                ));
            }
            Ok(principal)
        }

        async fn session(&self, headers: &HeaderMap, body: Value) -> Response {
            let presented = headers.get("authorization").and_then(|v| v.to_str().ok());
            if self.authenticated(presented).is_err() {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": {"code": "unauthenticated", "message": "invalid token"}})),
                )
                    .into_response();
            }
            let text = |key: &str| body[key].as_str().unwrap_or_default().to_owned();
            let principal = Principal {
                id: text("act_as"),
                roles: body["roles"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
                entity: body["entity"].as_str().map(str::to_owned),
                tenant: text("tenant"),
                workspace: body["workspaces"][0]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            };
            let ttl = body["ttl_secs"].as_u64().unwrap_or(3600);
            match self.stub.mint_session(&principal, ttl).await {
                Ok(session) => Json(json!({
                    "token": session.token,
                    "expires_at": session.expires_at,
                    "principal": { "id": principal.id, "roles": principal.roles },
                    "tenant": session.tenant,
                }))
                .into_response(),
                Err(err) => {
                    let status = match err {
                        OntoError::Denied(_) => StatusCode::FORBIDDEN,
                        OntoError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
                        OntoError::NotFound(_) => StatusCode::NOT_FOUND,
                        OntoError::Invalid(_) => StatusCode::BAD_REQUEST,
                        OntoError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
                    };
                    (
                        status,
                        Json(json!({"error": {"code": "error", "message": err.to_string()}})),
                    )
                        .into_response()
                }
            }
        }
    }

    fn failed(err: OntoError) -> Status {
        status_of(&err)
    }

    fn reply(value: Value) -> Reply<prost_types::Struct> {
        Ok(tonic::Response::new(json::to_struct(&value)))
    }

    #[tonic::async_trait]
    impl AdminService for StubServer {
        async fn health(&self, req: Request<()>) -> Reply<pb::HealthStatus> {
            self.principal(req.metadata())?;
            let health = self.stub.health().await.map_err(failed)?;
            Ok(tonic::Response::new(pb::HealthStatus {
                status: health.status,
                version: health.version,
                revision: health.revision,
                llm: health.llm,
                llm_available: health.llm_available,
            }))
        }

        async fn stats(&self, _: Request<()>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn schema(&self, _: Request<()>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn actions(&self, _: Request<()>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn call(&self, _: Request<pb::CallRequest>) -> Reply<pb::CallResponse> {
            Err(Status::unimplemented("stub"))
        }
    }

    #[tonic::async_trait]
    impl EventService for StubServer {
        async fn append(&self, _: Request<pb::EventInput>) -> Reply<pb::Event> {
            Err(Status::unimplemented("stub"))
        }

        async fn list(&self, req: Request<pb::EventFilter>) -> Reply<pb::EventList> {
            let principal = self.principal(req.metadata())?;
            let query = convert::event_query_from(req.into_inner());
            let events = self.stub.events(&principal, query).await.map_err(failed)?;
            Ok(tonic::Response::new(pb::EventList {
                events: events.into_iter().map(convert::event_to).collect(),
            }))
        }

        async fn ingest(
            &self,
            req: Request<Streaming<prost_types::Struct>>,
        ) -> Reply<pb::IngestSummary> {
            let principal = self.principal(req.metadata())?;
            let mut stream = req.into_inner();
            let mut records: Vec<Value> = Vec::new();
            while let Some(record) = stream.next().await {
                records.push(json::from_struct(&record?));
            }
            let outcome = self
                .stub
                .ingest(&principal, records)
                .await
                .map_err(failed)?;
            Ok(tonic::Response::new(pb::IngestSummary {
                accepted: outcome.accepted,
                queued: outcome.queued,
                result: None,
            }))
        }
    }

    type AskEventStream =
        std::pin::Pin<Box<dyn futures::Stream<Item = Result<pb::AskEvent, Status>> + Send>>;

    #[tonic::async_trait]
    impl IntelliService for StubServer {
        type AskStreamStream = AskEventStream;

        async fn ask(&self, req: Request<pb::AskRequest>) -> Reply<pb::AskResponse> {
            let principal = self.principal(req.metadata())?;
            let request = convert::ask_request_from(req.into_inner());
            let answer = self.stub.ask(&principal, request).await.map_err(failed)?;
            Ok(tonic::Response::new(convert::ask_answer_to(&answer)))
        }

        async fn ask_stream(&self, req: Request<pb::AskRequest>) -> Reply<Self::AskStreamStream> {
            let principal = self.principal(req.metadata())?;
            let request = convert::ask_request_from(req.into_inner());
            let events = self
                .stub
                .ask_stream(&principal, request)
                .await
                .map_err(failed)?;
            let wire = events.map(|event| {
                Ok(match event {
                    AskStreamEvent::Stage(text) => pb::AskEvent {
                        r#type: "stage".into(),
                        text,
                        data: None,
                    },
                    AskStreamEvent::Entities(entities) => pb::AskEvent {
                        r#type: "entities".into(),
                        text: String::new(),
                        data: Some(json::to_proto(
                            &serde_json::to_value(entities).unwrap_or_default(),
                        )),
                    },
                    AskStreamEvent::Delta(text) => pb::AskEvent {
                        r#type: "delta".into(),
                        text,
                        data: None,
                    },
                    AskStreamEvent::Done(answer) => pb::AskEvent {
                        r#type: "done".into(),
                        text: String::new(),
                        data: Some(convert::ask_answer_to_value(&answer)),
                    },
                    AskStreamEvent::Error(text) => pb::AskEvent {
                        r#type: "error".into(),
                        text,
                        data: None,
                    },
                })
            });
            Ok(tonic::Response::new(Box::pin(wire)))
        }

        async fn build_context(
            &self,
            _: Request<pb::ContextRequest>,
        ) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn infer(&self, _: Request<pb::GraphQuery>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn dependencies(&self, _: Request<pb::GraphQuery>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn impact(&self, _: Request<pb::GraphQuery>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn root_cause(&self, _: Request<pb::GraphQuery>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn critical_path(&self, _: Request<pb::GraphQuery>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn cycles(&self, _: Request<pb::GraphQuery>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }
    }

    #[tonic::async_trait]
    impl OntologyService for StubServer {
        async fn upsert_entity(&self, _: Request<pb::EntityInput>) -> Reply<pb::Entity> {
            Err(Status::unimplemented("stub"))
        }

        async fn patch_entity(&self, _: Request<pb::EntityPatch>) -> Reply<pb::Entity> {
            Err(Status::unimplemented("stub"))
        }

        async fn get_entity(&self, req: Request<pb::EntityRef>) -> Reply<pb::Entity> {
            let principal = self.principal(req.metadata())?;
            let id = req.into_inner().id;
            match self.stub.entity(&principal.workspace, &id) {
                Some(entity) => Ok(tonic::Response::new(convert::entity_to(
                    &entity,
                    &principal.workspace,
                ))),
                None => Err(Status::not_found(format!("entity {id}"))),
            }
        }

        async fn delete_entity(&self, _: Request<pb::EntityRef>) -> Reply<pb::DeleteResult> {
            Err(Status::unimplemented("stub"))
        }

        async fn find_entities(&self, req: Request<pb::FindRequest>) -> Reply<pb::EntityList> {
            let principal = self.principal(req.metadata())?;
            let query = convert::find_query_from(req.into_inner());
            let found = self.stub.find(&principal, query).await.map_err(failed)?;
            Ok(tonic::Response::new(pb::EntityList {
                total: found.len() as u64,
                entities: found
                    .iter()
                    .map(|e| convert::entity_to(e, &principal.workspace))
                    .collect(),
            }))
        }

        async fn search(&self, _: Request<pb::SearchRequest>) -> Reply<pb::SearchResult> {
            Err(Status::unimplemented("stub"))
        }

        async fn create_link(&self, _: Request<pb::LinkInput>) -> Reply<pb::LinkResult> {
            Err(Status::unimplemented("stub"))
        }

        async fn delete_link(&self, _: Request<pb::LinkInput>) -> Reply<pb::LinkResult> {
            Err(Status::unimplemented("stub"))
        }

        async fn apply(&self, _: Request<pb::ApplyRequest>) -> Reply<pb::MutationResult> {
            Err(Status::unimplemented("stub"))
        }

        async fn neighbors(&self, _: Request<pb::NeighborsRequest>) -> Reply<pb::NeighborsResult> {
            Err(Status::unimplemented("stub"))
        }

        async fn traverse(&self, req: Request<pb::TraverseRequest>) -> Reply<pb::Subgraph> {
            let principal = self.principal(req.metadata())?;
            let query = convert::traverse_query_from(req.into_inner());
            let found = self
                .stub
                .traverse(&principal, query)
                .await
                .map_err(failed)?;
            Ok(tonic::Response::new(convert::subgraph_to(
                &found,
                &principal.workspace,
            )))
        }

        async fn path(&self, _: Request<pb::PathRequest>) -> Reply<pb::PathResult> {
            Err(Status::unimplemented("stub"))
        }
    }

    fn vocabulary_of(
        node_types: &[prost_types::Struct],
        relation_types: &[prost_types::Struct],
    ) -> Result<Ontology, Status> {
        convert::vocabulary_from(&json!({
            "node_types": node_types.iter().map(json::from_struct).collect::<Vec<_>>(),
            "relation_types": relation_types.iter().map(json::from_struct).collect::<Vec<_>>(),
        }))
        .map_err(|err| Status::invalid_argument(err.to_string()))
    }

    #[tonic::async_trait]
    impl ExecutionService for StubServer {
        async fn graphs(&self, _: Request<pb::GraphsRequest>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn vocabulary(&self, req: Request<pb::GraphRef>) -> Reply<prost_types::Struct> {
            let principal = self.principal(req.metadata())?;
            let graph = req.into_inner().graph;
            let found = self
                .stub
                .vocabulary(&principal, &graph)
                .await
                .map_err(failed)?;
            reply(convert::vocabulary_value(&graph, &found))
        }

        async fn set_vocabulary(
            &self,
            req: Request<pb::VocabularyRequest>,
        ) -> Reply<prost_types::Struct> {
            let mut principal = self.principal(req.metadata())?;
            let message = req.into_inner();
            if !message.workspace.is_empty() {
                principal.workspace = message.workspace.clone();
            }
            let vocabulary = vocabulary_of(&message.node_types, &message.relation_types)?;
            let stored = self
                .stub
                .set_vocabulary(&principal, &message.graph, &vocabulary)
                .await
                .map_err(failed)?;
            reply(json!({
                "graph": message.graph,
                "vocabulary": convert::vocabulary_value(&message.graph, &stored),
            }))
        }

        async fn init_graph(
            &self,
            req: Request<pb::InitGraphRequest>,
        ) -> Reply<prost_types::Struct> {
            let mut principal = self.principal(req.metadata())?;
            let message = req.into_inner();
            if !message.workspace.is_empty() {
                principal.workspace = message.workspace.clone();
            }
            let vocabulary = vocabulary_of(&message.node_types, &message.relation_types)?;
            let given = !message.empty
                || !vocabulary.node_types.is_empty()
                || !vocabulary.relation_types.is_empty();
            let stored = self
                .stub
                .init_graph(
                    &principal,
                    &message.graph,
                    &message.title,
                    given.then_some(&vocabulary),
                )
                .await
                .map_err(failed)?;
            reply(json!({
                "graph": message.graph,
                "vocabulary": convert::vocabulary_value(&message.graph, &stored),
            }))
        }
    }

    #[tonic::async_trait]
    impl SchemaService for StubServer {
        async fn get(&self, req: Request<pb::SchemaVersionRequest>) -> Reply<prost_types::Struct> {
            let principal = self.principal(req.metadata())?;
            reply(self.stub.schema(&principal).await.map_err(failed)?)
        }

        async fn status(&self, _: Request<()>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn plan(&self, req: Request<pb::SchemaChangeRequest>) -> Reply<prost_types::Struct> {
            let principal = self.principal(req.metadata())?;
            let message = req.into_inner();
            let change = SchemaChange {
                changes: message.changes.iter().map(json::from_struct).collect(),
                migrations: message.migrations.iter().map(json::from_struct).collect(),
                message: message.message,
            };
            reply(
                self.stub
                    .plan_schema(&principal, &change)
                    .await
                    .map_err(failed)?,
            )
        }

        async fn apply(&self, _: Request<pb::SchemaChangeRequest>) -> Reply<prost_types::Struct> {
            Err(Status::permission_denied(
                "the stub applies nothing: changes go through proposals",
            ))
        }

        async fn history(
            &self,
            _: Request<pb::SchemaHistoryRequest>,
        ) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn version(
            &self,
            _: Request<pb::SchemaVersionRequest>,
        ) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn as_of(&self, _: Request<pb::SchemaAsOfRequest>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn diff(&self, _: Request<pb::SchemaDiffRequest>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn revert(&self, _: Request<pb::SchemaRevertRequest>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn undo(&self, _: Request<pb::SchemaRevertRequest>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn propose(&self, req: Request<pb::ProposeRequest>) -> Reply<prost_types::Struct> {
            let principal = self.principal(req.metadata())?;
            let message = req.into_inner();
            let draft = SchemaProposalDraft {
                title: message.title,
                rationale: message.rationale,
                changes: message.changes.iter().map(json::from_struct).collect(),
                migrations: message.migrations.iter().map(json::from_struct).collect(),
                source: message.source,
            };
            reply(
                self.stub
                    .propose_schema(&principal, &draft)
                    .await
                    .map_err(failed)?,
            )
        }

        async fn list_proposals(
            &self,
            req: Request<pb::ListProposalsRequest>,
        ) -> Reply<prost_types::Struct> {
            let principal = self.principal(req.metadata())?;
            let message = req.into_inner();
            let status = Some(message.status.as_str()).filter(|s| !s.is_empty());
            let found = self
                .stub
                .proposals(&principal, status, message.limit)
                .await
                .map_err(failed)?;
            reply(json!({ "proposals": found }))
        }

        async fn get_proposal(&self, _: Request<pb::ProposalRef>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn approve(&self, _: Request<pb::ReviewRequest>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented(
                "stub: people approve in onto-intelli",
            ))
        }

        async fn reject(&self, _: Request<pb::ReviewRequest>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub: people reject in onto-intelli"))
        }

        async fn comment(&self, _: Request<pb::ReviewRequest>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn withdraw(&self, _: Request<pb::ProposalRef>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }

        async fn revise(&self, _: Request<pb::ReviseRequest>) -> Reply<prost_types::Struct> {
            Err(Status::unimplemented("stub"))
        }
    }

    /// The header names the server reads, for tests that look at the wire.
    pub const HEADERS: [&str; 6] = [
        header::TENANT,
        header::WORKSPACE,
        header::ACT_AS,
        header::ACT_ROLES,
        header::ACT_WORKSPACES,
        header::ACT_ENTITY,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal() -> Principal {
        Principal::system("ws")
    }

    #[tokio::test]
    async fn records_become_entities_links_and_events() {
        let stub = StubOntology::new();
        stub.ingest(
            &principal(),
            vec![
                json!({"op": "upsert", "kind": "agent", "key": "a", "title": "A",
                       "props": {"principal": "agent://w/a"},
                       "links": [{"rel": "ran", "to": "run:1"}]}),
                json!({"id": "run:1", "kind": "run", "state": "running"}),
                json!({"op": "link", "from": "run:1", "rel": "executed", "to": "node:1"}),
                json!({"op": "event", "action": "agent.read", "actor": "agent:a",
                       "target": "document:d", "ts": 5, "payload": {"label": "restricted"}}),
            ],
        )
        .await
        .unwrap();
        assert_eq!(stub.entity("ws", "agent:a").unwrap().title, "A");
        assert!(stub.has_link("ws", "agent:a", "ran", "run:1"));
        assert!(stub.has_link("ws", "run:1", "executed", "node:1"));
        assert_eq!(
            stub.events_of("ws")[0].payload_str("label"),
            Some("restricted")
        );

        // A second upsert merges properties and keeps the state.
        stub.ingest(
            &principal(),
            vec![json!({"id": "run:1", "props": {"cost": 1}})],
        )
        .await
        .unwrap();
        let run = stub.entity("ws", "run:1").unwrap();
        assert_eq!(run.state, "running");
        assert_eq!(run.props["cost"], 1);

        let found = stub
            .find(
                &principal(),
                FindQuery {
                    kind: "run".into(),
                    ..FindQuery::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        let walked = stub
            .traverse(
                &principal(),
                TraverseQuery {
                    start: vec!["node:1".into()],
                    rels: vec![],
                    direction: Direction::In,
                    depth: 3,
                    limit: 10,
                    kinds: vec![],
                },
            )
            .await
            .unwrap();
        let ids: Vec<&str> = walked.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["node:1", "run:1", "agent:a"]);
        let events = stub
            .events(
                &principal(),
                EventQuery {
                    action: Some("agent".into()),
                    since_ms: Some(1),
                    ..EventQuery::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert!(
            stub.entity("other", "agent:a").is_none(),
            "workspaces are apart"
        );

        // A delete removes the entity and its links; deleting what is gone fails the batch.
        stub.ingest(&principal(), vec![json!({"op": "delete", "id": "run:1"})])
            .await
            .unwrap();
        assert!(stub.entity("ws", "run:1").is_none());
        assert!(!stub.has_link("ws", "agent:a", "ran", "run:1"));
        let gone = stub
            .ingest(
                &principal(),
                vec![
                    json!({"id": "team:t", "kind": "team"}),
                    json!({"op": "delete", "id": "run:1"}),
                ],
            )
            .await;
        assert!(matches!(gone, Err(OntoError::NotFound(_))));
        assert!(stub.entity("ws", "team:t").is_none(), "the batch is atomic");

        stub.fail_with(Some(OntoError::Unavailable("down".into())));
        assert!(stub.health().await.is_err());
        let bad = stub.ingest(&principal(), vec![json!({"op": "nope"})]).await;
        assert!(matches!(bad, Err(OntoError::Unavailable(_))));
        stub.fail_with(None);
        let bad = stub.ingest(&principal(), vec![json!({"op": "nope"})]).await;
        assert!(matches!(bad, Err(OntoError::Invalid(_))));
    }

    #[tokio::test]
    async fn vocabularies_live_per_tenant_in_the_servers_order() {
        let stub = StubOntology::new();
        let w = uuid::Uuid::from_u128(1);
        let member = Principal::user(
            "nexc-",
            w,
            uuid::Uuid::from_u128(2),
            crate::domain::workspace::WorkspaceRole::Member,
        );
        let guest = Principal::user(
            "nexc-",
            w,
            uuid::Uuid::from_u128(3),
            crate::domain::workspace::WorkspaceRole::Guest,
        );
        assert!(matches!(
            stub.vocabulary(&member, "g").await,
            Err(OntoError::NotFound(_))
        ));
        assert!(matches!(
            stub.set_vocabulary(&member, "g", &Ontology::starter())
                .await,
            Err(OntoError::NotFound(_))
        ));
        let created = stub
            .init_graph(&member, "g", "G", Some(&Ontology::starter()))
            .await
            .unwrap();
        let keys: Vec<&str> = created.node_types.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(
            keys,
            ["topic", "research", "code", "task", "document", "output"]
        );
        let mut less = Ontology::starter();
        less.node_types.retain(|t| t.key != "topic");
        stub.set_vocabulary(&member, "execution_graph:g", &less)
            .await
            .unwrap();
        assert_eq!(
            stub.vocabulary(&member, "g")
                .await
                .unwrap()
                .node_types
                .len(),
            5
        );
        assert!(matches!(
            stub.set_vocabulary(&guest, "g", &less).await,
            Err(OntoError::Denied(_))
        ));
        let mut bad = less.clone();
        bad.node_types[0].color = "red".into();
        assert!(matches!(
            stub.set_vocabulary(&member, "g", &bad).await,
            Err(OntoError::Invalid(_))
        ));
        let other = Principal::user(
            "nexc-",
            uuid::Uuid::from_u128(9),
            uuid::Uuid::from_u128(2),
            crate::domain::workspace::WorkspaceRole::Member,
        );
        assert!(
            stub.vocabulary(&other, "g").await.is_err(),
            "tenants are apart"
        );
        assert_eq!(stub.calls_to("set_vocabulary").len(), 4);
    }

    #[tokio::test]
    async fn agents_propose_and_the_proposals_are_listed() {
        let stub = StubOntology::new();
        let w = uuid::Uuid::from_u128(1);
        let assistant = Principal::assistant("nexc-", w, uuid::Uuid::from_u128(2));
        let schema = stub.schema(&assistant).await.unwrap();
        assert!(schema["object_types"]["issue"].is_object());
        let change = json!({"op": "put_link_type", "name": "reviews",
                            "def": {"from": ["person"], "to": ["issue"]}});
        let plan = stub
            .plan_schema(
                &assistant,
                &SchemaChange {
                    changes: vec![change.clone()],
                    ..SchemaChange::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(plan["valid"], true);
        let filed = stub
            .propose_schema(
                &assistant,
                &SchemaProposalDraft {
                    title: "Track reviews".into(),
                    rationale: "reviews come up in every retro".into(),
                    changes: vec![change],
                    ..SchemaProposalDraft::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(filed["status"], "open");
        assert_eq!(filed["proposer"]["agent"], true);
        let open = stub.proposals(&assistant, Some("open"), 10).await.unwrap();
        assert_eq!(open.len(), 1);
        let bad = stub
            .plan_schema(
                &assistant,
                &SchemaChange {
                    changes: vec![json!({"name": "x"})],
                    ..SchemaChange::default()
                },
            )
            .await;
        assert!(matches!(bad, Err(OntoError::Invalid(_))));
        let session = stub.mint_session(&assistant, 600).await.unwrap();
        assert!(session.token.starts_with("onto1."));
        assert_eq!(session.tenant, assistant.tenant);
    }
}
