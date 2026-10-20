//! Spans, runs and agents as ontology facts: the records the export streams
//! to onto-intelli. Only ids, names, hosts, counts, decisions and status
//! leave the database: a span's `attributes` are read key by key for the
//! few that name things, never copied, and no prompt, argument, output or
//! document content is ever a fact.
//!
//! Entity ids are `kind:key`. Facts of one nexc workspace live in one
//! onto-intelli workspace, so keys are nexc ids: `run:{uuid}`,
//! `document:{uuid}`, `agent:{principal without the scheme}`,
//! `tool:{name}`, `host:{host}`, `policy:{agent key}` (an agent's
//! capabilities), `action:{kind}/{subject}` (something a policy decided on).

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::domain::capabilities::Capabilities;
use crate::domain::trace::{Span, SpanKind, SpanStatus};
use crate::repo::onto_export::{ExportAgent, ExportRun, ExportSpan};

/// Longest name kept in a fact.
const NAME_MAX: usize = 200;
/// The scheme of a principal URN.
const AGENT_SCHEME: &str = "agent://";
/// The group of a workspace's owners and admins in onto-intelli (the
/// projection states who is in it): what is restricted is visible to them.
pub const ADMINS: &str = "team:admins";
/// The tool that runs code; the policy question looks for it.
pub const CODE_EXECUTION_TOOL: &str = "run_python";

/// Relations between facts.
pub mod rel {
    pub const RAN: &str = "ran";
    pub const EXECUTED: &str = "executed";
    pub const USED_TOOL: &str = "used_tool";
    pub const USED_MODEL: &str = "used_model";
    pub const READ: &str = "read";
    pub const CALLED: &str = "called";
    pub const DELEGATED_TO: &str = "delegated_to";
    pub const DENIED: &str = "denied";
    pub const DENIED_IN: &str = "denied_in";
    pub const ATTEMPTED: &str = "attempted";
    pub const GOVERNED_BY: &str = "governed_by";
    pub const ALLOWS: &str = "allows";
    pub const REPORTS_TO: &str = "reports_to";
}

/// Actions of the events the export records (time-windowed questions use them).
pub mod action {
    pub const LLM: &str = "agent.llm";
    pub const TOOL: &str = "agent.tool";
    pub const READ: &str = "agent.read";
    pub const EGRESS: &str = "agent.egress";
    pub const DELEGATED: &str = "agent.delegated";
    pub const APPROVAL: &str = "agent.approval";
    pub const DENIED: &str = "policy.denied";
    pub const RUN_FINISHED: &str = "run.finished";
    pub const POLICY_CHANGED: &str = "policy.changed";
}

/// Entity kinds.
pub mod kind {
    pub const AGENT: &str = "agent";
    pub const RUN: &str = "run";
    pub const NODE: &str = "node";
    pub const TOOL: &str = "tool";
    pub const MODEL: &str = "model";
    pub const DOCUMENT: &str = "document";
    pub const MEMORY: &str = "memory";
    pub const HOST: &str = "host";
    pub const POLICY: &str = "policy";
    pub const ACTION: &str = "action";
}

/// A name as a fact: no control characters, bounded.
pub fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(NAME_MAX)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The first eight characters of an id, for titles.
pub fn short(id: Uuid) -> String {
    id.to_string().chars().take(8).collect()
}

/// The key of an agent entity: its principal without the scheme (`system` stays).
pub fn agent_key(principal: &str) -> String {
    clean(principal.strip_prefix(AGENT_SCHEME).unwrap_or(principal))
}

pub fn agent_id(principal: &str) -> String {
    format!("{}:{}", kind::AGENT, agent_key(principal))
}

/// The principal an agent entity id stands for.
pub fn principal_of(agent_entity_id: &str) -> Option<String> {
    let key = agent_entity_id.strip_prefix("agent:")?;
    Some(if key == "system" {
        key.to_owned()
    } else {
        format!("{AGENT_SCHEME}{key}")
    })
}

/// What an agent is called: the principal after its workspace
/// (`writer`, `writer/researcher-1`), `system` for the backend.
pub fn agent_title(principal: &str) -> String {
    let key = agent_key(principal);
    match key.split_once('/') {
        Some((_, rest)) => rest.to_owned(),
        None => key,
    }
}

/// The agent's own name: the first segment of its title.
pub fn agent_name(principal: &str) -> String {
    let title = agent_title(principal);
    title.split('/').next().unwrap_or(&title).to_owned()
}

/// Whether a principal names a sub-agent (`{agent}/{role}-{n}`).
pub fn is_sub_agent(principal: &str) -> bool {
    agent_title(principal).contains('/')
}

pub fn run_id(id: Uuid) -> String {
    format!("{}:{id}", kind::RUN)
}

pub fn node_id(id: Uuid) -> String {
    format!("{}:{id}", kind::NODE)
}

pub fn document_id(id: Uuid) -> String {
    format!("{}:{id}", kind::DOCUMENT)
}

pub fn memory_id(id: Uuid) -> String {
    format!("{}:{id}", kind::MEMORY)
}

pub fn tool_id(name: &str) -> String {
    format!("{}:{}", kind::TOOL, clean(name))
}

pub fn model_id(name: &str) -> String {
    format!("{}:{}", kind::MODEL, clean(name))
}

pub fn host_id(host: &str) -> String {
    format!("{}:{}", kind::HOST, clean(host).to_lowercase())
}

/// An agent's policy: its capabilities document.
pub fn policy_id(principal: &str) -> String {
    format!("{}:{}", kind::POLICY, agent_key(principal))
}

/// Something a policy decided on: `action:tool/run_python`, `action:egress/evil.example`.
pub fn action_entity_id(action_kind: &str, subject: &str) -> String {
    format!("{}:{}/{}", kind::ACTION, clean(action_kind), clean(subject))
}

/// The nexc id inside an entity id, when it is one (`run:{uuid}`, `document:{uuid}`).
pub fn uuid_of(entity_id: &str) -> Option<Uuid> {
    entity_id.split_once(':')?.1.parse().ok()
}

/// Labels the export resolved for a `data_read` span: the documents behind
/// its passage ids and the memories it lists, with their classification.
#[derive(Debug, Clone, Default)]
pub struct ReadLabels {
    pub documents: Vec<(Uuid, String)>,
    pub memories: Vec<(Uuid, String)>,
}

/// The records of one export, with every entity and link stated once.
#[derive(Debug)]
pub struct Batch {
    workspace: String,
    seen: HashSet<String>,
    pub records: Vec<Value>,
    pub spans: usize,
}

fn millis(at: DateTime<Utc>) -> i64 {
    at.timestamp_millis()
}

fn attr_str<'a>(span: &'a Span, key: &str) -> Option<&'a str> {
    span.attributes.get(key).and_then(Value::as_str)
}

fn attr_int(span: &Span, key: &str) -> Option<i64> {
    span.attributes.get(key).and_then(Value::as_i64)
}

impl Batch {
    pub fn new(workspace: &str) -> Self {
        Batch {
            workspace: workspace.to_owned(),
            seen: HashSet::new(),
            records: Vec::new(),
            spans: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// States an entity once per batch; `props` are merged into what onto-intelli has.
    fn entity(&mut self, id: &str, kind: &str, title: String, state: Option<&str>, props: Value) {
        self.entity_aliased(id, kind, title, state, props, Vec::new());
    }

    fn entity_aliased(
        &mut self,
        id: &str,
        kind: &str,
        title: String,
        state: Option<&str>,
        props: Value,
        aliases: Vec<String>,
    ) {
        if !self.seen.insert(format!("e {id}")) {
            return;
        }
        let mut record = Map::new();
        record.insert("op".into(), json!("upsert"));
        record.insert("id".into(), json!(id));
        record.insert("kind".into(), json!(kind));
        record.insert("workspace".into(), json!(self.workspace));
        record.insert("title".into(), json!(title));
        if let Some(state) = state {
            record.insert("state".into(), json!(state));
        }
        record.insert("props".into(), props);
        if !aliases.is_empty() {
            record.insert("aliases".into(), json!(aliases));
        }
        self.records.push(Value::Object(record));
    }

    fn link(&mut self, from: &str, rel: &str, to: &str) {
        if !self.seen.insert(format!("l {from} {rel} {to}")) {
            return;
        }
        self.records.push(json!({
            "op": "link", "from": from, "rel": rel, "to": to, "workspace": self.workspace,
        }));
    }

    /// An event of the audit trail. onto-intelli stamps it with the moment it
    /// arrives (`ts`); the moment it happened travels as `payload.at`, Unix
    /// milliseconds, so a backfill of old spans does not look like today.
    fn event(
        &mut self,
        action: &str,
        actor: &str,
        target: &str,
        at: DateTime<Utc>,
        outcome: &str,
        mut payload: Value,
    ) {
        if let Value::Object(map) = &mut payload {
            map.insert("at".into(), json!(millis(at)));
        }
        self.records.push(json!({
            "op": "event", "action": action, "actor": actor, "target": target,
            "workspace": self.workspace, "outcome": outcome, "source": "nexc",
            "payload": payload,
        }));
    }

    fn agent_entity(&mut self, principal: &str, nexc_agent: Option<Uuid>) -> String {
        let id = agent_id(principal);
        let mut props = json!({
            "principal": principal,
            "name": agent_name(principal),
            "sub_agent": is_sub_agent(principal),
        });
        if let Some(agent) = nexc_agent {
            props["nexc_agent_id"] = json!(agent);
        }
        self.entity_aliased(
            &id,
            kind::AGENT,
            agent_title(principal),
            None,
            props,
            vec![agent_name(principal)],
        );
        id
    }

    /// The facts of one span.
    pub fn span(&mut self, exported: &ExportSpan, run: Option<&ExportRun>, labels: &ReadLabels) {
        let span = &exported.span;
        self.spans += 1;
        let agent = self.agent_entity(&span.principal, span.agent_id);
        let run_entity = match run {
            Some(run) => self.run(run, false),
            None => {
                let id = run_id(span.run_id);
                self.entity(
                    &id,
                    kind::RUN,
                    format!("run {}", short(span.run_id)),
                    None,
                    json!({"graph_id": exported.graph_id, "nexc_id": span.run_id}),
                );
                id
            }
        };
        let node = node_id(span.node_id);
        self.entity(
            &node,
            kind::NODE,
            format!("node {}", short(span.node_id)),
            None,
            json!({"graph_id": exported.graph_id, "nexc_id": span.node_id}),
        );
        self.link(&agent, rel::RAN, &run_entity);
        self.link(&run_entity, rel::EXECUTED, &node);
        let outcome = match span.status {
            SpanStatus::Ok => "success",
            SpanStatus::Error => "failure",
        };
        let at = span.started_at;
        let where_ = json!({"run": span.run_id, "node": span.node_id, "span": span.id});
        let with = |extra: Value| {
            let mut payload = where_.clone();
            if let (Value::Object(into), Value::Object(from)) = (&mut payload, extra) {
                into.extend(from);
            }
            payload
        };
        match span.kind {
            SpanKind::ToolCall => {
                let name = attr_str(span, "tool").unwrap_or(&span.name);
                let tool = tool_id(name);
                self.entity(&tool, kind::TOOL, clean(name), None, json!({}));
                self.link(&agent, rel::USED_TOOL, &tool);
                self.event(action::TOOL, &agent, &tool, at, outcome, with(json!({})));
            }
            SpanKind::LlmCall => {
                let name = attr_str(span, "model").unwrap_or(&span.name);
                let model = model_id(name);
                self.entity(&model, kind::MODEL, clean(name), None, json!({}));
                self.link(&agent, rel::USED_MODEL, &model);
                let tokens = json!({
                    "tokens_in": attr_int(span, "tokens_in").unwrap_or(0),
                    "tokens_out": attr_int(span, "tokens_out").unwrap_or(0),
                });
                self.event(action::LLM, &agent, &model, at, outcome, with(tokens));
            }
            SpanKind::DataRead => {
                for (document, label) in &labels.documents {
                    let id = document_id(*document);
                    let mut props = json!({"label": label, "nexc_id": document});
                    if label == "restricted" {
                        props["visibility"] = json!("restricted");
                        props["acl"] = json!([ADMINS]);
                    }
                    // The workspace projection names the document; the audit fills
                    // only what is not there yet.
                    let before = self.records.len();
                    self.entity(
                        &id,
                        kind::DOCUMENT,
                        format!("document {}", short(*document)),
                        None,
                        props,
                    );
                    if let Some(Value::Object(record)) = self.records.get_mut(before) {
                        record.insert("fill_only".into(), json!(true));
                    }
                    self.link(&agent, rel::READ, &id);
                    self.event(
                        action::READ,
                        &agent,
                        &id,
                        at,
                        outcome,
                        with(json!({"label": label, "kind": kind::DOCUMENT})),
                    );
                }
                for (memory, label) in &labels.memories {
                    let id = memory_id(*memory);
                    let mut props = json!({"label": label, "nexc_id": memory});
                    if label == "restricted" {
                        props["visibility"] = json!("restricted");
                        props["acl"] = json!([ADMINS]);
                    }
                    self.entity(
                        &id,
                        kind::MEMORY,
                        format!("memory {}", short(*memory)),
                        None,
                        props,
                    );
                    self.link(&agent, rel::READ, &id);
                    self.event(
                        action::READ,
                        &agent,
                        &id,
                        at,
                        outcome,
                        with(json!({"label": label, "kind": kind::MEMORY})),
                    );
                }
            }
            SpanKind::Egress => {
                let host = attr_str(span, "host").unwrap_or(&span.name);
                let id = host_id(host);
                self.entity(&id, kind::HOST, clean(host).to_lowercase(), None, json!({}));
                self.link(&agent, rel::CALLED, &id);
                self.event(action::EGRESS, &agent, &id, at, outcome, with(json!({})));
            }
            SpanKind::Spawn => {
                let child_principal = attr_str(span, "child").unwrap_or(&span.name);
                let child = self.agent_entity(child_principal, span.agent_id);
                self.link(&agent, rel::DELEGATED_TO, &child);
                let role = attr_str(span, "role").map(clean);
                self.event(
                    action::DELEGATED,
                    &agent,
                    &child,
                    at,
                    outcome,
                    with(json!({"role": role})),
                );
            }
            SpanKind::Policy => {
                let decision = attr_str(span, "decision").unwrap_or("deny");
                if decision == "allow" {
                    return;
                }
                let rule = attr_str(span, "rule").map(clean).unwrap_or_default();
                let action_kind = attr_str(span, "action")
                    .map(clean)
                    .unwrap_or_else(|| "action".into());
                let subject = clean(&span.name);
                let version = attr_int(span, "policy_version");
                let policy = policy_id(&span.principal);
                let mut props = json!({"principal": span.principal});
                if let Some(version) = version {
                    props["version"] = json!(version);
                }
                self.entity(
                    &policy,
                    kind::POLICY,
                    format!("policy of {}", agent_title(&span.principal)),
                    None,
                    props,
                );
                let action_entity = action_entity_id(&action_kind, &subject);
                self.entity(
                    &action_entity,
                    kind::ACTION,
                    format!("{action_kind} {subject}"),
                    None,
                    json!({"action": action_kind, "subject": subject}),
                );
                self.link(&agent, rel::GOVERNED_BY, &policy);
                self.link(&policy, rel::DENIED, &action_entity);
                self.link(&action_entity, rel::DENIED_IN, &run_entity);
                self.link(&agent, rel::ATTEMPTED, &action_entity);
                self.event(
                    action::DENIED,
                    &agent,
                    &run_entity,
                    at,
                    "denied",
                    with(json!({
                        "rule": rule, "action": action_kind, "subject": subject,
                        "decision": decision, "policy_version": version, "policy": policy,
                    })),
                );
            }
            SpanKind::Approval => {
                let decision = attr_str(span, "decision").map(clean);
                let approver = attr_str(span, "approver").map(clean);
                let outcome = match (span.status, decision.as_deref()) {
                    (SpanStatus::Error, _) | (_, Some("denied") | Some("rejected")) => "denied",
                    (_, Some("pending")) => "pending",
                    _ => "success",
                };
                self.event(action::APPROVAL, &agent, &run_entity, at, outcome,
                    with(json!({"name": clean(&span.name), "decision": decision, "approver": approver})));
            }
        }
    }

    /// The run entity, with its status and cost; `finished` adds the event
    /// the "who ran what" questions look for.
    pub fn run(&mut self, run: &ExportRun, finished: bool) -> String {
        let id = run_id(run.id);
        if !self.seen.contains(&format!("e {id}")) || finished {
            self.seen.remove(&format!("e {id}"));
            self.entity(
                &id,
                kind::RUN,
                format!("run {}", short(run.id)),
                Some(run.status.as_str()),
                json!({
                    "graph_id": run.graph_id,
                    "nexc_id": run.id,
                    "cost_usd": run.cost_usd,
                    "tokens_in": run.tokens_in,
                    "tokens_out": run.tokens_out,
                    "policy_mismatches": run.policy_mismatches,
                    "started_at": run.started_at,
                    "finished_at": run.finished_at,
                }),
            );
        }
        if finished && let Some(at) = run.finished_at {
            let outcome = match run.status.as_str() {
                "succeeded" => "success",
                "cancelled" => "denied",
                _ => "failure",
            };
            self.event(
                action::RUN_FINISHED,
                "system",
                &id,
                at,
                outcome,
                json!({"status": run.status, "cost_usd": run.cost_usd,
                       "tokens_in": run.tokens_in, "tokens_out": run.tokens_out, "run": run.id}),
            );
        }
        id
    }

    /// An agent and its policy (its capabilities), as of now; `changed` adds
    /// the `policy.changed` event (not for a backfill, which states what is).
    pub fn agent(&mut self, agent: &ExportAgent, all: &[ExportAgent], changed: bool) {
        let id = agent_id(&agent.principal_id);
        self.seen.remove(&format!("e {id}"));
        self.entity_aliased(
            &id,
            kind::AGENT,
            agent_title(&agent.principal_id),
            Some(agent.status.as_str()),
            json!({
                "principal": agent.principal_id,
                "name": agent.name,
                "role": agent.role,
                "model": agent.model,
                "nexc_agent_id": agent.id,
                "sub_agent": false,
                "policy_version": agent.policy_version,
            }),
            vec![agent.name.clone()],
        );
        let capabilities: Capabilities =
            serde_json::from_value(agent.capabilities.clone()).unwrap_or_default();
        let policy = policy_id(&agent.principal_id);
        let tools: Value = match &capabilities.tools {
            Some(tools) => json!(tools),
            None => Value::Null,
        };
        self.seen.remove(&format!("e {policy}"));
        self.entity(
            &policy,
            kind::POLICY,
            format!("policy of {} v{}", agent.name, agent.policy_version),
            None,
            json!({
                "principal": agent.principal_id,
                "nexc_agent_id": agent.id,
                "version": agent.policy_version,
                "all_tools": capabilities.tools.is_none(),
                "tools": tools,
                "labels": capabilities.data.labels,
                "egress_hosts": capabilities.egress_hosts,
                "models": capabilities.models,
                "max_depth": capabilities.delegation.max_depth,
                "max_fan_out": capabilities.delegation.max_fan_out,
            }),
        );
        self.link(&id, rel::GOVERNED_BY, &policy);
        if let Some(tools) = &capabilities.tools {
            for tool in tools {
                let tool_entity = tool_id(tool);
                self.entity(&tool_entity, kind::TOOL, clean(tool), None, json!({}));
                self.link(&policy, rel::ALLOWS, &tool_entity);
            }
        }
        if let Some(manager) = agent.reports_to
            && let Some(manager) = all.iter().find(|a| a.id == manager)
        {
            self.link(&id, rel::REPORTS_TO, &agent_id(&manager.principal_id));
        }
        if changed {
            self.event(
                action::POLICY_CHANGED,
                "system",
                &policy,
                Utc::now(),
                "success",
                json!({"version": agent.policy_version, "agent": agent.id}),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;
    use crate::domain::agent::AgentStatus;
    use crate::domain::run::RunStatus;
    use crate::domain::trace::{GENESIS, Span};

    const PRINCIPAL: &str = "agent://0190a1b2-c3d4-7e5f-8a9b-9c1d2e3f4a5b/writer";

    fn span(kind: SpanKind, name: &str, attributes: Value) -> ExportSpan {
        let now = Utc::now();
        ExportSpan {
            span: Span {
                id: Uuid::now_v7(),
                run_id: Uuid::now_v7(),
                node_id: Uuid::now_v7(),
                agent_id: Some(Uuid::now_v7()),
                principal: PRINCIPAL.into(),
                parent_id: None,
                kind,
                name: name.into(),
                started_at: now,
                ended_at: Some(now),
                status: SpanStatus::Ok,
                attributes,
                seq: 0,
                prev_hash: GENESIS,
                hash: GENESIS,
            },
            seq: 1,
            graph_id: Uuid::now_v7(),
        }
    }

    fn ops<'a>(batch: &'a Batch, op: &str) -> Vec<&'a Value> {
        batch.records.iter().filter(|r| r["op"] == op).collect()
    }

    fn links(batch: &Batch) -> Vec<(String, String, String)> {
        ops(batch, "link")
            .iter()
            .map(|l| {
                (
                    l["from"].as_str().unwrap().to_owned(),
                    l["rel"].as_str().unwrap().to_owned(),
                    l["to"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn names_are_derived_from_the_principal() {
        assert_eq!(agent_id(PRINCIPAL), format!("agent:{}", &PRINCIPAL[8..]));
        assert_eq!(agent_title(PRINCIPAL), "writer");
        assert_eq!(agent_name(&format!("{PRINCIPAL}/researcher-1")), "writer");
        assert_eq!(
            agent_title(&format!("{PRINCIPAL}/researcher-1")),
            "writer/researcher-1"
        );
        assert!(is_sub_agent(&format!("{PRINCIPAL}/researcher-1")));
        assert_eq!(agent_id("system"), "agent:system");
        assert_eq!(
            principal_of(&agent_id(PRINCIPAL)).as_deref(),
            Some(PRINCIPAL)
        );
        assert_eq!(principal_of("agent:system").as_deref(), Some("system"));
        assert_eq!(host_id(" API.Example.com\n"), "host:api.example.com");
        assert_eq!(
            action_entity_id("tool", "run_python"),
            "action:tool/run_python"
        );
        assert_eq!(clean(&"x".repeat(500)).len(), NAME_MAX);
        let id = Uuid::now_v7();
        assert_eq!(uuid_of(&run_id(id)), Some(id));
        assert_eq!(uuid_of("tool:write_file"), None);
    }

    #[test]
    fn every_span_kind_yields_its_facts() {
        let mut batch = Batch::new("ws");
        let tool = span(
            SpanKind::ToolCall,
            "write_file",
            json!({"args_sha256": "ab", "result_bytes": 3}),
        );
        batch.span(&tool, None, &ReadLabels::default());
        let agent = agent_id(PRINCIPAL);
        let l = links(&batch);
        assert!(l.contains(&(agent.clone(), "ran".into(), run_id(tool.span.run_id))));
        assert!(l.contains(&(
            run_id(tool.span.run_id),
            "executed".into(),
            node_id(tool.span.node_id)
        )));
        assert!(l.contains(&(agent.clone(), "used_tool".into(), "tool:write_file".into())));
        assert_eq!(ops(&batch, "event")[0]["action"], "agent.tool");

        let doc = Uuid::now_v7();
        let memory = Uuid::now_v7();
        let read = span(
            SpanKind::DataRead,
            "context",
            json!({"document_chunk_ids": ["x"], "memory_ids": [memory]}),
        );
        let labels = ReadLabels {
            documents: vec![(doc, "restricted".into())],
            memories: vec![(memory, "internal".into())],
        };
        let mut batch = Batch::new("ws");
        batch.span(&read, None, &labels);
        let l = links(&batch);
        assert!(l.contains(&(agent.clone(), "read".into(), document_id(doc))));
        assert!(l.contains(&(agent.clone(), "read".into(), memory_id(memory))));
        let document = ops(&batch, "upsert")
            .into_iter()
            .find(|e| e["id"] == document_id(doc))
            .unwrap();
        assert_eq!(document["props"]["label"], "restricted");
        assert_eq!(document["props"]["visibility"], "restricted");
        let events = ops(&batch, "event");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["payload"]["label"], "restricted");

        let mut batch = Batch::new("ws");
        batch.span(
            &span(SpanKind::Egress, "api.example.com", json!({})),
            None,
            &ReadLabels::default(),
        );
        assert!(links(&batch).contains(&(
            agent.clone(),
            "called".into(),
            "host:api.example.com".into()
        )));

        let child = format!("{PRINCIPAL}/researcher-1");
        let mut batch = Batch::new("ws");
        batch.span(
            &span(
                SpanKind::Spawn,
                &child,
                json!({"child": child, "role": "researcher"}),
            ),
            None,
            &ReadLabels::default(),
        );
        assert!(links(&batch).contains(&(agent.clone(), "delegated_to".into(), agent_id(&child))));
        let sub = ops(&batch, "upsert")
            .into_iter()
            .find(|e| e["id"] == agent_id(&child))
            .unwrap();
        assert_eq!(sub["props"]["sub_agent"], true);
        assert_eq!(sub["title"], "writer/researcher-1");

        let mut batch = Batch::new("ws");
        let denied = span(
            SpanKind::Policy,
            "run_python",
            json!({"decision": "deny", "rule": "tools.allow", "action": "tool", "policy_version": 3, "reason": "no"}),
        );
        batch.span(&denied, None, &ReadLabels::default());
        let l = links(&batch);
        assert!(l.contains(&(
            policy_id(PRINCIPAL),
            "denied".into(),
            "action:tool/run_python".into()
        )));
        assert!(l.contains(&(
            "action:tool/run_python".into(),
            "denied_in".into(),
            run_id(denied.span.run_id)
        )));
        let event = ops(&batch, "event")
            .into_iter()
            .find(|e| e["action"] == "policy.denied")
            .unwrap();
        assert_eq!(event["outcome"], "denied");
        assert_eq!(event["payload"]["rule"], "tools.allow");
        assert_eq!(event["target"], run_id(denied.span.run_id));

        let mut batch = Batch::new("ws");
        batch.span(
            &span(
                SpanKind::Policy,
                "write_file",
                json!({"decision": "allow", "rule": "tools.allow"}),
            ),
            None,
            &ReadLabels::default(),
        );
        assert!(
            ops(&batch, "event").is_empty(),
            "an allowed action is not an event"
        );

        let mut batch = Batch::new("ws");
        batch.span(
            &span(
                SpanKind::Approval,
                "deploy",
                json!({"decision": "approved", "approver": "user:1"}),
            ),
            None,
            &ReadLabels::default(),
        );
        let event = ops(&batch, "event")
            .into_iter()
            .find(|e| e["action"] == "agent.approval")
            .unwrap();
        assert_eq!(event["payload"]["approver"], "user:1");

        let mut batch = Batch::new("ws");
        batch.span(
            &span(
                SpanKind::LlmCall,
                "claude-x",
                json!({"tokens_in": 10, "tokens_out": 2}),
            ),
            None,
            &ReadLabels::default(),
        );
        let event = ops(&batch, "event")[0];
        assert_eq!(event["payload"]["tokens_in"], 10);
        assert!(
            event["payload"]["at"].is_i64() && event.get("ts").is_none(),
            "{event}"
        );
    }

    #[test]
    fn content_never_becomes_a_fact() {
        let poison = json!({
            "prompt": "SECRET-PROMPT", "args": {"path": "SECRET-ARGS"}, "content": "SECRET-CONTENT",
            "api_key": "SECRET-KEY", "output": "SECRET-OUTPUT", "reason": "SECRET-REASON",
            "args_sha256": "SECRET-HASH", "decision": "deny", "rule": "tools.allow", "action": "tool",
            "host": "SECRET-HOST?", "tool": "SECRET-TOOL",
        });
        let mut batch = Batch::new("ws");
        for kind in [
            SpanKind::LlmCall,
            SpanKind::ToolCall,
            SpanKind::Spawn,
            SpanKind::DataRead,
            SpanKind::Egress,
            SpanKind::Policy,
            SpanKind::Approval,
        ] {
            batch.span(
                &span(kind, "name", poison.clone()),
                None,
                &ReadLabels::default(),
            );
        }
        let text = serde_json::to_string(&batch.records).unwrap();
        for leaked in [
            "SECRET-PROMPT",
            "SECRET-ARGS",
            "SECRET-CONTENT",
            "SECRET-KEY",
            "SECRET-OUTPUT",
            "SECRET-REASON",
            "SECRET-HASH",
        ] {
            assert!(!text.contains(leaked), "{leaked} leaked: {text}");
        }
        // Names of things (a tool, a host) are facts; they are the only attributes read.
        assert!(text.contains("tool:SECRET-TOOL") && text.contains("host:secret-host?"));
    }

    #[test]
    fn runs_and_agents_carry_status_cost_and_policy() {
        let run = ExportRun {
            id: Uuid::now_v7(),
            graph_id: Uuid::now_v7(),
            status: RunStatus::Succeeded,
            cost_usd: 0.25,
            tokens_in: 100,
            tokens_out: 20,
            started_at: Some(Utc::now()),
            finished_at: Some(Utc::now()),
            policy_mismatches: 0,
            queued: false,
        };
        let mut batch = Batch::new("ws");
        batch.run(&run, true);
        let entity = ops(&batch, "upsert")[0];
        assert_eq!(entity["state"], "succeeded");
        assert_eq!(entity["props"]["cost_usd"], 0.25);
        assert_eq!(ops(&batch, "event")[0]["action"], "run.finished");

        let manager = ExportAgent {
            id: Uuid::now_v7(),
            principal_id: "agent://w/lead".into(),
            name: "lead".into(),
            role: "lead".into(),
            model: String::new(),
            status: AgentStatus::Active,
            policy_version: 0,
            capabilities: json!({}),
            reports_to: None,
        };
        let agent = ExportAgent {
            id: Uuid::now_v7(),
            principal_id: "agent://w/coder".into(),
            name: "coder".into(),
            role: "coder".into(),
            model: "claude-x".into(),
            status: AgentStatus::Active,
            policy_version: 2,
            capabilities: json!({"tools": ["run_python", "write_file"]}),
            reports_to: Some(manager.id),
        };
        let mut batch = Batch::new("ws");
        batch.agent(&agent, std::slice::from_ref(&manager), true);
        assert_eq!(ops(&batch, "event")[0]["action"], "policy.changed");
        let l = links(&batch);
        assert!(l.contains(&(
            "agent:w/coder".into(),
            "governed_by".into(),
            "policy:w/coder".into()
        )));
        assert!(l.contains(&(
            "policy:w/coder".into(),
            "allows".into(),
            "tool:run_python".into()
        )));
        assert!(l.contains(&(
            "agent:w/coder".into(),
            "reports_to".into(),
            "agent:w/lead".into()
        )));
        let policy = ops(&batch, "upsert")
            .into_iter()
            .find(|e| e["id"] == "policy:w/coder")
            .unwrap();
        assert_eq!(policy["props"]["all_tools"], false);
        assert_eq!(policy["props"]["version"], 2);

        let mut batch = Batch::new("ws");
        batch.agent(&manager, &[], false);
        assert!(
            ops(&batch, "event").is_empty(),
            "a backfill states, it does not change"
        );
        let policy = ops(&batch, "upsert")
            .into_iter()
            .find(|e| e["id"] == "policy:w/lead")
            .unwrap();
        assert_eq!(
            policy["props"]["all_tools"], true,
            "no tool list means every tool"
        );
    }
}
