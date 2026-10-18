//! "Ask the audit": a question about what agents did, answered from the
//! facts in onto-intelli with the facts it rests on.
//!
//! Six questions every workspace asks are canned: the backend recognises
//! them (and their slots: a run id, an agent's name, a time window) and
//! answers them as graph and event queries, which costs no tokens and takes
//! milliseconds. Anything else goes to onto-intelli's `Ask`, which answers
//! from the graph when it can and otherwise asks a model to explain, with
//! the facts it cites. Either way the answer names its facts as nexc things
//! (an agent, a run, a document, a policy) that link back into the console;
//! an answer that makes claims without a fact is marked `unverified`.
//!
//! The asker's workspace role is their clearance: admins and owners see
//! every fact, members do not see confidential or restricted data ids (the
//! fact stays, as "withheld", so counts remain honest), guests are refused.
//! onto-intelli's own bearer authentication binds the backend's token to one
//! principal, so the clearance is applied here, before an answer leaves.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Instant;

use async_stream::stream;
use chrono::{Duration, Utc};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::facts::{self, action, kind, rel};
use super::{
    AskMode, AskRequest, AskStreamEvent, Direction, EventQuery, FindQuery, OntoError, OntoEvent,
    OntologyBackend, Principal, PrincipalRole, TraverseQuery, workspace_name,
};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::workspace::WorkspaceRole;
use crate::repo::onto_export as repo;

/// Longest question accepted.
pub const QUESTION_MAX: usize = 1000;
/// Most events one canned question reads.
const EVENTS_MAX: u32 = 2000;
/// Most facts cited by one answer.
const FACTS_MAX: usize = 60;

/// One of the questions offered as a chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct CannedQuestion {
    pub id: &'static str,
    /// The question as offered; `{run}` and `{agent}` are the slots the UI fills.
    pub label: &'static str,
}

/// The six canned questions, in the order the console offers them.
pub const CANNED: [CannedQuestion; 6] = [
    CannedQuestion {
        id: "restricted_reads",
        label: "Which agents read restricted data this week?",
    },
    CannedQuestion {
        id: "egress_denied",
        label: "Which agents called hosts outside the allow-list?",
    },
    CannedQuestion {
        id: "run_denied",
        label: "Why was run {run} denied?",
    },
    CannedQuestion {
        id: "agent_day",
        label: "What did agent {agent} do today?",
    },
    CannedQuestion {
        id: "code_policies",
        label: "Which policies allow code execution?",
    },
    CannedQuestion {
        id: "approvals",
        label: "Who approved what this month?",
    },
];

/// How an answer was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnswerMode {
    /// From the graph alone: no model, 0 tokens.
    Deterministic,
    /// A model explained, with facts it was given.
    Llm,
}

/// A fact an answer rests on, as a thing in nexc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AuditFact {
    /// The entity in onto-intelli (`agent:…`, `run:…`, `document:…`, `policy:…`) or
    /// `withheld:…` for data the asker is not cleared for.
    pub id: String,
    /// `agent`, `run`, `node`, `document`, `memory`, `policy`, `action`, `tool`, `host`, `model`, `withheld`.
    pub kind: String,
    pub label: String,
    /// Where to open it in the console; `null` for things without a page.
    #[schema(required = true)]
    pub link: Option<String>,
}

/// An answer from the audit.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct AuditAnswer {
    pub answer: String,
    pub mode: AnswerMode,
    pub facts: Vec<AuditFact>,
    /// Model tokens the answer cost (0 when deterministic).
    pub tokens: u64,
    /// From the question arriving to the answer leaving, in milliseconds.
    pub latency_ms: f64,
    /// True when the answer makes claims but cites no fact: read it as a
    /// model's words, not the record.
    pub unverified: bool,
    /// The canned question this was recognised as, when it was one.
    #[schema(required = true)]
    pub question: Option<String>,
}

/// One event of a streamed answer.
#[derive(Debug, Clone, PartialEq)]
pub enum AuditStreamEvent {
    Stage(String),
    Delta(String),
    Done(AuditAnswer),
    Error(String),
}

/// Who is asking.
#[derive(Debug, Clone, Copy)]
pub struct Asker {
    pub user_id: Uuid,
    pub workspace_id: Uuid,
    pub role: WorkspaceRole,
}

/// What the asker may see of the data agents read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clearance {
    /// Admins and owners see confidential and restricted ids; members do not.
    pub all: bool,
}

impl Clearance {
    pub fn of(role: WorkspaceRole) -> Self {
        Clearance {
            all: role.is_admin(),
        }
    }

    pub fn may_see(self, label: &str) -> bool {
        self.all || matches!(label, "public" | "internal")
    }
}

impl Asker {
    fn principal(&self, state: &AppState) -> Principal {
        Principal {
            id: format!("user:{}", self.user_id),
            role: if self.role.is_admin() {
                PrincipalRole::Admin
            } else {
                PrincipalRole::Member
            },
            workspace: workspace_name(&state.settings.onto_workspace_prefix, self.workspace_id),
        }
    }

    fn clearance(&self) -> Clearance {
        Clearance::of(self.role)
    }
}

/// The configured onto-intelli, or the 503 that says there is none.
pub fn backend(state: &AppState) -> Result<&dyn OntologyBackend, AppError> {
    state.ontology.as_deref().ok_or_else(|| {
        AppError::Unavailable(
            "Ask the audit is off: no onto-intelli is configured (set NEXC_ONTO_URL)".into(),
        )
    })
}

fn failed(err: OntoError) -> AppError {
    match err {
        OntoError::Unavailable(detail) => {
            AppError::Unavailable(format!("the audit service is not reachable: {detail}"))
        }
        OntoError::Denied(detail) => {
            AppError::Forbidden(format!("the audit service refused the question: {detail}"))
        }
        OntoError::Invalid(detail) => AppError::Unprocessable(detail),
        OntoError::Other(detail) => AppError::Internal(anyhow::anyhow!("onto-intelli: {detail}")),
    }
}

/// Refuses a question that is empty, too long, or asked by a guest.
pub fn check(asker: &Asker, question: &str) -> Result<(), AppError> {
    if !asker.role.is_member() {
        return Err(AppError::Forbidden(
            "guests cannot ask the audit; ask a member of the workspace".into(),
        ));
    }
    if question.trim().is_empty() {
        return Err(AppError::field("question", "must not be empty"));
    }
    if question.chars().count() > QUESTION_MAX {
        return Err(AppError::field(
            "question",
            format!("must be at most {QUESTION_MAX} characters"),
        ));
    }
    Ok(())
}

// ───────────────────────── recognising the canned questions ─────────────────────────

/// A canned question with its slots filled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matched {
    pub id: &'static str,
    pub run: Option<Uuid>,
    pub agent: Option<String>,
    /// The time window, in days.
    pub days: i64,
    /// Which classifications count as "restricted data".
    pub labels: Vec<&'static str>,
}

fn words(question: &str) -> Vec<String> {
    question
        .to_lowercase()
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_' && c != '.')
                .trim_end_matches('.')
                .to_owned()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// "today" → 1, "this week" → 7, "last 3 days" → 3, …; `None` when no window is named.
fn window(words: &[String]) -> Option<i64> {
    let text = words.join(" ");
    for (i, w) in words.iter().enumerate() {
        if matches!(w.as_str(), "last" | "past")
            && let Some(n) = words.get(i + 1).and_then(|n| n.parse::<i64>().ok())
        {
            let unit = words.get(i + 2).map(String::as_str).unwrap_or("days");
            return Some(match unit.trim_end_matches('s') {
                "week" => n * 7,
                "month" => n * 30,
                "hour" => 1,
                _ => n,
            });
        }
    }
    if words.iter().any(|w| w == "today") {
        Some(1)
    } else if words.iter().any(|w| w == "yesterday") {
        Some(2)
    } else if text.contains("this week") || text.contains("last week") || text.contains("past week")
    {
        Some(7)
    } else if text.contains("this month")
        || text.contains("last month")
        || text.contains("past month")
    {
        Some(30)
    } else if text.contains("this quarter") || text.contains("last quarter") {
        Some(90)
    } else if text.contains("this year") || text.contains("last year") {
        Some(365)
    } else {
        None
    }
}

fn has_any(words: &[String], needles: &[&str]) -> bool {
    words.iter().any(|w| needles.contains(&w.as_str()))
}

fn has_prefix(words: &[String], prefixes: &[&str]) -> bool {
    words
        .iter()
        .any(|w| prefixes.iter().any(|p| w.starts_with(p)))
}

/// The agent named between "did" and "do" ("what did agent writer do today").
fn agent_slot(words: &[String]) -> Option<String> {
    let did = words.iter().position(|w| w == "did")?;
    let end = words
        .iter()
        .enumerate()
        .skip(did + 1)
        .find(|(_, w)| matches!(w.as_str(), "do" | "does" | "done"))
        .map(|(i, _)| i)?;
    let name: Vec<&str> = words[did + 1..end]
        .iter()
        .map(String::as_str)
        .filter(|w| !matches!(*w, "agent" | "the" | "our" | "my"))
        .collect();
    (!name.is_empty()).then(|| name.join(" "))
}

/// Recognises a canned question, with its slots.
pub fn classify(question: &str) -> Option<Matched> {
    let words = words(question);
    let text = words.join(" ");
    let uuid = words.iter().find_map(|w| w.parse::<Uuid>().ok());
    let days = window(&words);
    let matched = |id: &'static str, default_days: i64| Matched {
        id,
        run: uuid,
        agent: None,
        days: days.unwrap_or(default_days),
        labels: vec!["restricted"],
    };
    let denied = has_prefix(&words, &["denied", "deny", "refus", "block", "reject"]);
    if has_any(&words, &["why"]) && denied && (has_any(&words, &["run"]) || uuid.is_some()) {
        return Some(matched("run_denied", 0));
    }
    let data = has_prefix(
        &words,
        &["restricted", "confidential", "classified", "sensitive"],
    );
    if data
        && has_prefix(
            &words,
            &["read", "access", "saw", "see", "use", "touch", "open"],
        )
    {
        let mut m = matched("restricted_reads", 7);
        if has_prefix(&words, &["confidential", "classified", "sensitive"]) {
            m.labels = vec!["confidential", "restricted"];
        }
        return Some(m);
    }
    let hosts = has_prefix(
        &words,
        &[
            "host", "egress", "call", "reach", "connect", "url", "domain",
        ],
    );
    let outside = denied
        || has_prefix(
            &words,
            &[
                "outside",
                "allow-list",
                "allowlist",
                "allow",
                "whitelist",
                "forbidden",
                "not",
            ],
        );
    if hosts && outside && !has_prefix(&words, &["polic"]) {
        return Some(matched("egress_denied", 30));
    }
    if has_prefix(&words, &["polic"])
        && (text.contains("code execution")
            || text.contains("execute code")
            || text.contains("run code")
            || text.contains("running code")
            || has_any(&words, &[facts::CODE_EXECUTION_TOOL, "code"]))
    {
        return Some(matched("code_policies", 0));
    }
    if has_prefix(&words, &["approv"]) {
        return Some(matched("approvals", 30));
    }
    if has_any(&words, &["did"])
        && has_any(&words, &["do", "does", "done"])
        && !has_any(&words, &["why"])
    {
        let mut m = matched("agent_day", 1);
        m.agent = agent_slot(&words);
        return Some(m);
    }
    None
}

// ───────────────────────────────── answering ─────────────────────────────────

/// Answers `question` for `asker`.
pub async fn answer(
    state: &AppState,
    asker: &Asker,
    question: &str,
) -> Result<AuditAnswer, AppError> {
    check(asker, question)?;
    let backend = backend(state)?;
    let started = Instant::now();
    let principal = asker.principal(state);
    let mut answer = match classify(question) {
        Some(matched) => canned(state, backend, asker, &principal, &matched).await?,
        None => {
            let reply = backend
                .ask(
                    &principal,
                    AskRequest {
                        question: question.to_owned(),
                        mode: AskMode::Auto,
                        token_budget: None,
                    },
                )
                .await
                .map_err(failed)?;
            from_onto(state, asker, reply).await?
        }
    };
    answer.latency_ms = started.elapsed().as_secs_f64() * 1000.0;
    state
        .metrics
        .onto_asks
        .add(&[("mode", mode_label(answer.mode))], 1);
    Ok(answer)
}

fn mode_label(mode: AnswerMode) -> &'static str {
    match mode {
        AnswerMode::Deterministic => "deterministic",
        AnswerMode::Llm => "llm",
    }
}

/// Answers `question` as a stream: stages and text as onto-intelli produces
/// them, then the whole answer. A canned question, or a member whose
/// clearance has to be applied to the text, gets the answer in one event.
pub async fn answer_stream(
    state: AppState,
    asker: Asker,
    question: String,
) -> Result<impl Stream<Item = AuditStreamEvent> + Send + 'static, AppError> {
    check(&asker, &question)?;
    backend(&state)?;
    let direct = classify(&question).is_some() || !asker.clearance().all;
    let started = Instant::now();
    let events = stream! {
        if direct {
            match answer(&state, &asker, &question).await {
                Ok(done) => yield AuditStreamEvent::Done(done),
                Err(err) => yield AuditStreamEvent::Error(err.to_string()),
            }
            return;
        }
        let backend = match backend(&state) {
            Ok(backend) => backend,
            Err(err) => {
                yield AuditStreamEvent::Error(err.to_string());
                return;
            }
        };
        let principal = asker.principal(&state);
        let request = AskRequest { question: question.clone(), mode: AskMode::Auto, token_budget: None };
        let mut inner = match backend.ask_stream(&principal, request).await {
            Ok(inner) => inner,
            Err(err) => {
                yield AuditStreamEvent::Error(failed(err).to_string());
                return;
            }
        };
        while let Some(event) = inner.next().await {
            match event {
                AskStreamEvent::Stage(stage) => yield AuditStreamEvent::Stage(stage),
                AskStreamEvent::Entities(_) => {}
                AskStreamEvent::Delta(text) => yield AuditStreamEvent::Delta(text),
                AskStreamEvent::Done(reply) => {
                    match from_onto(&state, &asker, reply).await {
                        Ok(mut done) => {
                            done.latency_ms = started.elapsed().as_secs_f64() * 1000.0;
                            state.metrics.onto_asks.add(&[("mode", mode_label(done.mode))], 1);
                            yield AuditStreamEvent::Done(done);
                        }
                        Err(err) => yield AuditStreamEvent::Error(err.to_string()),
                    }
                    return;
                }
                AskStreamEvent::Error(text) => {
                    yield AuditStreamEvent::Error(text);
                    return;
                }
            }
        }
        yield AuditStreamEvent::Error("the audit service ended the answer early".into());
    };
    Ok(events)
}

/// onto-intelli's answer as the audit's: its facts resolved to nexc things,
/// the asker's clearance applied, and `unverified` when it cites none.
async fn from_onto(
    state: &AppState,
    asker: &Asker,
    reply: super::AskAnswer,
) -> Result<AuditAnswer, AppError> {
    let mut ids: Vec<String> = reply
        .facts
        .iter()
        .flat_map(|fact| entity_ids_in(fact))
        .collect();
    ids.extend(reply.entities.iter().map(|e| e.id.clone()));
    let mut text = reply.answer.clone();
    let facts = resolve(state, asker, ids, &mut text).await?;
    let mode = match reply.mode.as_str() {
        "deterministic" | "answer_cache" => AnswerMode::Deterministic,
        _ => AnswerMode::Llm,
    };
    let unverified = facts.is_empty() && !text.trim().is_empty();
    Ok(AuditAnswer {
        answer: text,
        mode,
        facts,
        tokens: reply.tokens(),
        latency_ms: reply.total_ms,
        unverified,
        question: None,
    })
}

/// The entity ids in a fact as onto-intelli states it (`agent:x read document:y`).
fn entity_ids_in(fact: &str) -> Vec<String> {
    fact.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')' | '[' | ']'))
        .filter(|token| {
            token.split_once(':').is_some_and(|(kind, key)| {
                !kind.is_empty()
                    && !key.is_empty()
                    && kind.chars().all(|c| c.is_ascii_lowercase() || c == '_')
            })
        })
        .map(|token| token.trim_end_matches(['.', ':']).to_owned())
        .collect()
}

// ─────────────────────────────── canned answers ───────────────────────────────

struct Draft {
    text: String,
    facts: Vec<String>,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn window_text(days: i64) -> String {
    match days {
        1 => "today".into(),
        7 => "this week".into(),
        30 => "this month".into(),
        n => format!("in the last {n} days"),
    }
}

fn since_ms(days: i64) -> i64 {
    (Utc::now() - Duration::days(days.max(1))).timestamp_millis()
}

async fn events_since(
    backend: &dyn OntologyBackend,
    principal: &Principal,
    action: &str,
    days: i64,
    actor: Option<String>,
    target: Option<String>,
) -> Result<Vec<OntoEvent>, AppError> {
    backend
        .events(
            principal,
            EventQuery {
                actor,
                target,
                action: Some(action.to_owned()),
                since_ms: (days > 0).then(|| since_ms(days)),
                until_ms: None,
                limit: EVENTS_MAX,
            },
        )
        .await
        .map_err(failed)
}

async fn canned(
    state: &AppState,
    backend: &dyn OntologyBackend,
    asker: &Asker,
    principal: &Principal,
    matched: &Matched,
) -> Result<AuditAnswer, AppError> {
    let draft = match matched.id {
        "restricted_reads" => restricted_reads(backend, principal, matched).await?,
        "egress_denied" => egress_denied(backend, principal, matched).await?,
        "run_denied" => run_denied(state, backend, asker, principal, matched).await?,
        "agent_day" => agent_day(state, backend, asker, principal, matched).await?,
        "code_policies" => code_policies(backend, principal).await?,
        _ => approvals(backend, principal, matched).await?,
    };
    let mut text = draft.text;
    let facts = resolve(state, asker, draft.facts, &mut text).await?;
    Ok(AuditAnswer {
        answer: text,
        mode: AnswerMode::Deterministic,
        facts,
        tokens: 0,
        latency_ms: 0.0,
        unverified: false,
        question: Some(matched.id.to_owned()),
    })
}

async fn restricted_reads(
    backend: &dyn OntologyBackend,
    principal: &Principal,
    matched: &Matched,
) -> Result<Draft, AppError> {
    let events = events_since(backend, principal, action::READ, matched.days, None, None).await?;
    let mut by_agent: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for event in &events {
        let label = event.payload_str("label").unwrap_or("internal");
        if matched.labels.contains(&label) {
            by_agent
                .entry(event.actor.clone())
                .or_default()
                .insert(event.target.clone());
        }
    }
    let what = if matched.labels.len() > 1 {
        "confidential or restricted data"
    } else {
        "restricted data"
    };
    let when = window_text(matched.days);
    if by_agent.is_empty() {
        return Ok(Draft {
            text: format!("No agent read {what} {when}."),
            facts: Vec::new(),
        });
    }
    let mut lines = Vec::new();
    let mut facts = Vec::new();
    for (agent, items) in &by_agent {
        lines.push(format!(
            "{} read {}",
            facts::agent_title(&facts::principal_of(agent).unwrap_or_default()),
            plural(items.len(), "item", "items")
        ));
        facts.push(agent.clone());
        facts.extend(items.iter().cloned());
    }
    Ok(Draft {
        text: format!(
            "{} read {what} {when}: {}.",
            plural(by_agent.len(), "agent", "agents"),
            lines.join("; ")
        ),
        facts,
    })
}

async fn egress_denied(
    backend: &dyn OntologyBackend,
    principal: &Principal,
    matched: &Matched,
) -> Result<Draft, AppError> {
    let denials =
        events_since(backend, principal, action::DENIED, matched.days, None, None).await?;
    let mut by_agent: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut facts = Vec::new();
    for event in &denials {
        if event.payload_str("rule") != Some("egress.hosts") {
            continue;
        }
        let host = event.payload_str("subject").unwrap_or("?").to_owned();
        by_agent
            .entry(event.actor.clone())
            .or_default()
            .insert(host.clone());
        facts.push(facts::action_entity_id("egress", &host));
        if let Some(policy) = event.payload_str("policy") {
            facts.push(policy.to_owned());
        }
        facts.push(event.target.clone());
    }
    let when = window_text(matched.days);
    if by_agent.is_empty() {
        return Ok(Draft {
            text: format!("No agent tried a host outside its allow-list {when}."),
            facts: Vec::new(),
        });
    }
    let lines: Vec<String> = by_agent
        .iter()
        .map(|(agent, hosts)| {
            format!(
                "{} tried {}",
                facts::agent_title(&facts::principal_of(agent).unwrap_or_default()),
                hosts.iter().cloned().collect::<Vec<_>>().join(", ")
            )
        })
        .collect();
    let mut all: Vec<String> = by_agent.keys().cloned().collect();
    all.extend(facts);
    Ok(Draft {
        text: format!(
            "{} called hosts outside the allow-list {when} (every attempt was refused by rule egress.hosts): {}.",
            plural(by_agent.len(), "agent", "agents"),
            lines.join("; ")
        ),
        facts: all,
    })
}

async fn run_denied(
    state: &AppState,
    backend: &dyn OntologyBackend,
    asker: &Asker,
    principal: &Principal,
    matched: &Matched,
) -> Result<Draft, AppError> {
    let Some(run) = matched.run else {
        return Ok(Draft {
            text: "Name the run: ask \"Why was run <run id> denied?\" with the id from the run's page.".into(),
            facts: Vec::new(),
        });
    };
    if !repo::run_in_workspace(&state.db, asker.workspace_id, run)
        .await
        .map_err(AppError::from)?
    {
        return Err(AppError::NotFound("run"));
    }
    let run_entity = facts::run_id(run);
    let denials = events_since(
        backend,
        principal,
        action::DENIED,
        0,
        None,
        Some(run_entity.clone()),
    )
    .await?;
    if denials.is_empty() {
        return Ok(Draft {
            text: format!(
                "Run {} has no denied action on record: every policy decision in it was an allow.",
                facts::short(run)
            ),
            facts: vec![run_entity],
        });
    }
    let mut lines = Vec::new();
    let mut facts = vec![run_entity];
    for event in denials.iter().rev() {
        let rule = event.payload_str("rule").unwrap_or("?");
        let kind_ = event.payload_str("action").unwrap_or("action");
        let subject = event.payload_str("subject").unwrap_or("?");
        let agent = facts::agent_title(&facts::principal_of(&event.actor).unwrap_or_default());
        let version = event
            .payload
            .get("policy_version")
            .and_then(serde_json::Value::as_i64)
            .map(|v| format!(" v{v}"))
            .unwrap_or_default();
        let decision = event.payload_str("decision").unwrap_or("deny");
        lines.push(format!(
            "{agent} was refused {kind_} {subject} by rule {rule} of its policy{version}{}",
            if decision == "mismatch" {
                " (the runtime had decided otherwise: a policy mismatch)"
            } else {
                ""
            }
        ));
        facts.push(event.actor.clone());
        facts.push(facts::action_entity_id(kind_, subject));
        if let Some(policy) = event.payload_str("policy") {
            facts.push(policy.to_owned());
        }
    }
    Ok(Draft {
        text: format!(
            "Run {} had {}: {}.",
            facts::short(run),
            plural(lines.len(), "denied action", "denied actions"),
            lines.join("; ")
        ),
        facts,
    })
}

async fn agent_day(
    state: &AppState,
    backend: &dyn OntologyBackend,
    asker: &Asker,
    principal: &Principal,
    matched: &Matched,
) -> Result<Draft, AppError> {
    let Some(name) = &matched.agent else {
        return Ok(Draft {
            text: "Name the agent: ask \"What did agent <name> do today?\".".into(),
            facts: Vec::new(),
        });
    };
    let agents = repo::agents_named(&state.db, asker.workspace_id, name)
        .await
        .map_err(AppError::from)?;
    if agents.is_empty() {
        return Ok(Draft {
            text: format!("There is no agent named {name} in this workspace."),
            facts: Vec::new(),
        });
    }
    // Several agents may share a name (a seeded one and a made one): the
    // answer covers all of them and says so.
    let agent_name = agents[0].2.clone();
    let actors: Vec<String> = agents
        .iter()
        .map(|(_, principal, _)| facts::agent_id(principal))
        .collect();
    let when = window_text(matched.days);
    let mut events = Vec::new();
    let mut denials = Vec::new();
    for actor in &actors {
        events.extend(
            events_since(
                backend,
                principal,
                "agent",
                matched.days,
                Some(actor.clone()),
                None,
            )
            .await?,
        );
        denials.extend(
            events_since(
                backend,
                principal,
                action::DENIED,
                matched.days,
                Some(actor.clone()),
                None,
            )
            .await?,
        );
    }
    let who = if actors.len() > 1 {
        format!("{agent_name} ({} agents of that name)", actors.len())
    } else {
        agent_name.clone()
    };
    if events.is_empty() && denials.is_empty() {
        return Ok(Draft {
            text: format!("{who} did nothing on record {when}."),
            facts: actors,
        });
    }
    let mut runs: BTreeSet<String> = BTreeSet::new();
    let mut tools: BTreeMap<String, usize> = BTreeMap::new();
    let mut turns = 0usize;
    let mut reads: BTreeMap<String, usize> = BTreeMap::new();
    let mut read_items: Vec<String> = Vec::new();
    let mut hosts: BTreeSet<String> = BTreeSet::new();
    let mut children: BTreeSet<String> = BTreeSet::new();
    for event in &events {
        if let Some(run) = event
            .payload_str("run")
            .and_then(|r| r.parse::<Uuid>().ok())
        {
            runs.insert(facts::run_id(run));
        }
        match event.action.as_str() {
            action::LLM => turns += 1,
            action::TOOL => *tools.entry(event.target.clone()).or_default() += 1,
            action::READ => {
                *reads
                    .entry(event.payload_str("label").unwrap_or("internal").to_owned())
                    .or_default() += 1;
                read_items.push(event.target.clone());
            }
            action::EGRESS => {
                hosts.insert(event.target.clone());
            }
            action::DELEGATED => {
                children.insert(event.target.clone());
            }
            _ => {}
        }
    }
    let mut parts = vec![format!("worked in {}", plural(runs.len(), "run", "runs"))];
    if turns > 0 {
        parts.push(format!(
            "took {}",
            plural(turns, "model turn", "model turns")
        ));
    }
    if !tools.is_empty() {
        let calls: usize = tools.values().sum();
        let names: Vec<String> = tools
            .iter()
            .map(|(tool, n)| format!("{} ×{n}", tool.trim_start_matches("tool:")))
            .collect();
        parts.push(format!(
            "made {} ({})",
            plural(calls, "tool call", "tool calls"),
            names.join(", ")
        ));
    }
    if !reads.is_empty() {
        let by_label: Vec<String> = reads
            .iter()
            .map(|(label, n)| format!("{n} {label}"))
            .collect();
        parts.push(format!(
            "read {} ({})",
            plural(read_items.len(), "item", "items"),
            by_label.join(", ")
        ));
    }
    if !hosts.is_empty() {
        parts.push(format!(
            "called {}",
            hosts
                .iter()
                .map(|h| h.trim_start_matches("host:"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !children.is_empty() {
        parts.push(format!(
            "delegated to {}",
            plural(children.len(), "sub-agent", "sub-agents")
        ));
    }
    if !denials.is_empty() {
        parts.push(format!(
            "was refused {}",
            plural(denials.len(), "action", "actions")
        ));
    }
    let mut facts = actors;
    facts.extend(runs);
    facts.extend(tools.into_keys());
    facts.extend(read_items);
    facts.extend(hosts);
    facts.extend(children);
    facts.extend(denials.drain(..).filter_map(|d| {
        Some(facts::action_entity_id(
            d.payload_str("action")?,
            d.payload_str("subject")?,
        ))
    }));
    Ok(Draft {
        text: format!("{who} {when}: {}.", parts.join(", ")),
        facts,
    })
}

async fn code_policies(
    backend: &dyn OntologyBackend,
    principal: &Principal,
) -> Result<Draft, AppError> {
    let unrestricted = backend
        .find(
            principal,
            FindQuery {
                kind: kind::POLICY.into(),
                props: serde_json::json!({"all_tools": true}),
                state: Vec::new(),
                limit: 200,
            },
        )
        .await
        .map_err(failed)?;
    let explicit = backend
        .traverse(
            principal,
            TraverseQuery {
                start: vec![facts::tool_id(facts::CODE_EXECUTION_TOOL)],
                rels: vec![rel::ALLOWS.into()],
                direction: Direction::In,
                depth: 1,
                limit: 200,
                kinds: vec![kind::POLICY.into()],
            },
        )
        .await
        .map_err(failed)?;
    let mut policies: BTreeMap<String, String> = BTreeMap::new();
    for policy in unrestricted {
        policies.insert(policy.id.clone(), format!("{} (every tool)", policy.title));
    }
    for policy in explicit.nodes {
        policies
            .entry(policy.id.clone())
            .or_insert_with(|| format!("{} ({} listed)", policy.title, facts::CODE_EXECUTION_TOOL));
    }
    if policies.is_empty() {
        return Ok(Draft {
            text: format!(
                "No policy allows code execution: no agent's capabilities admit the {} tool.",
                facts::CODE_EXECUTION_TOOL
            ),
            facts: vec![facts::tool_id(facts::CODE_EXECUTION_TOOL)],
        });
    }
    let mut facts: Vec<String> = policies.keys().cloned().collect();
    facts.push(facts::tool_id(facts::CODE_EXECUTION_TOOL));
    Ok(Draft {
        text: format!(
            "{} code execution ({}): {}.",
            if policies.len() == 1 {
                "1 policy allows".to_owned()
            } else {
                format!("{} policies allow", policies.len())
            },
            facts::CODE_EXECUTION_TOOL,
            policies.values().cloned().collect::<Vec<_>>().join("; ")
        ),
        facts,
    })
}

async fn approvals(
    backend: &dyn OntologyBackend,
    principal: &Principal,
    matched: &Matched,
) -> Result<Draft, AppError> {
    let events = events_since(
        backend,
        principal,
        action::APPROVAL,
        matched.days,
        None,
        None,
    )
    .await?;
    let when = window_text(matched.days);
    if events.is_empty() {
        return Ok(Draft {
            text: format!("Nothing was approved {when}: no approval is on record."),
            facts: Vec::new(),
        });
    }
    let mut lines = Vec::new();
    let mut facts = Vec::new();
    for event in events.iter().rev() {
        let approver = event.payload_str("approver").unwrap_or("someone");
        let name = event.payload_str("name").unwrap_or("an action");
        let decision = event.payload_str("decision").unwrap_or(&event.outcome);
        lines.push(format!(
            "{approver} {decision} {name} for {} in {}",
            facts::agent_title(&facts::principal_of(&event.actor).unwrap_or_default()),
            event
                .target
                .trim_start_matches("run:")
                .chars()
                .take(8)
                .collect::<String>()
        ));
        facts.push(event.target.clone());
        facts.push(event.actor.clone());
    }
    Ok(Draft {
        text: format!(
            "{} {when}: {}.",
            plural(events.len(), "approval", "approvals"),
            lines.join("; ")
        ),
        facts,
    })
}

// ─────────────────────────── facts as nexc things ───────────────────────────

/// Resolves entity ids to things in nexc (with links), applies the asker's
/// clearance to them and to `text`, and bounds the list.
async fn resolve(
    state: &AppState,
    asker: &Asker,
    ids: Vec<String>,
    text: &mut String,
) -> Result<Vec<AuditFact>, AppError> {
    let mut unique: Vec<String> = Vec::new();
    for id in ids {
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    unique.truncate(FACTS_MAX);
    let clearance = asker.clearance();

    // Agents: by principal, the parent's for sub-agents.
    let mut principals: Vec<String> = Vec::new();
    for id in &unique {
        let principal = match id.split_once(':') {
            Some(("agent", _)) => facts::principal_of(id),
            Some(("policy", key)) => facts::principal_of(&format!("agent:{key}")),
            _ => None,
        };
        if let Some(principal) = principal {
            principals.push(parent_principal(&principal));
        }
    }
    principals.sort_unstable();
    principals.dedup();
    let agents: HashMap<String, (Uuid, String)> = if principals.is_empty() {
        HashMap::new()
    } else {
        repo::agents_by_principal(&state.db, asker.workspace_id, &principals)
            .await
            .map_err(AppError::from)?
            .into_iter()
            .map(|(id, principal, name)| (principal, (id, name)))
            .collect()
    };
    let document_ids: Vec<Uuid> = unique
        .iter()
        .filter(|id| id.starts_with("document:"))
        .filter_map(|id| facts::uuid_of(id))
        .collect();
    let memory_ids: Vec<Uuid> = unique
        .iter()
        .filter(|id| id.starts_with("memory:"))
        .filter_map(|id| facts::uuid_of(id))
        .collect();
    let labels: HashMap<Uuid, String> = {
        let mut labels = HashMap::new();
        if !document_ids.is_empty() {
            labels.extend(
                repo::document_labels(&state.db, &document_ids)
                    .await
                    .map_err(AppError::from)?,
            );
        }
        if !memory_ids.is_empty() {
            labels.extend(
                repo::memory_labels(&state.db, &memory_ids)
                    .await
                    .map_err(AppError::from)?,
            );
        }
        labels
    };

    let mut out = Vec::with_capacity(unique.len());
    let mut withheld = 0usize;
    for id in unique {
        let (kind_, key) = id.split_once(':').unwrap_or(("fact", id.as_str()));
        let fact = match kind_ {
            "agent" => {
                let principal = facts::principal_of(&id).unwrap_or_default();
                let card = agents.get(&parent_principal(&principal));
                AuditFact {
                    id: id.clone(),
                    kind: kind_.into(),
                    label: card
                        .filter(|_| !facts::is_sub_agent(&principal))
                        .map_or_else(|| facts::agent_title(&principal), |(_, name)| name.clone()),
                    link: card.map(|(agent, _)| format!("/app/agents?agent={agent}")),
                }
            }
            "policy" => {
                let principal = facts::principal_of(&format!("agent:{key}")).unwrap_or_default();
                let card = agents.get(&parent_principal(&principal));
                AuditFact {
                    id: id.clone(),
                    kind: kind_.into(),
                    label: format!("policy of {}", facts::agent_title(&principal)),
                    link: card.map(|(agent, _)| format!("/app/agents?agent={agent}")),
                }
            }
            "run" => AuditFact {
                id: id.clone(),
                kind: kind_.into(),
                label: format!("run {}", key.chars().take(8).collect::<String>()),
                link: facts::uuid_of(&id).map(|run| format!("/app/runs/{run}")),
            },
            "node" => AuditFact {
                id: id.clone(),
                kind: kind_.into(),
                label: format!("node {}", key.chars().take(8).collect::<String>()),
                link: None,
            },
            "document" | "memory" => {
                let uuid = facts::uuid_of(&id);
                let label = uuid
                    .and_then(|u| labels.get(&u))
                    .map_or("internal", String::as_str);
                if !clearance.may_see(label) {
                    withheld += 1;
                    if let Some(uuid) = uuid {
                        let placeholder = format!("a {label} {kind_}");
                        *text = text.replace(&uuid.to_string(), &placeholder);
                        *text = text.replace(&id, &placeholder);
                    }
                    AuditFact {
                        id: format!("withheld:{kind_}:{withheld}"),
                        kind: "withheld".into(),
                        label: format!("a {label} {kind_} (withheld: not in your clearance)"),
                        link: None,
                    }
                } else {
                    AuditFact {
                        id: id.clone(),
                        kind: kind_.into(),
                        label: format!(
                            "{kind_} {} ({label})",
                            key.chars().take(8).collect::<String>()
                        ),
                        link: Some(if kind_ == "document" {
                            "/app/settings/knowledge".into()
                        } else {
                            "/app/settings/memory".into()
                        }),
                    }
                }
            }
            "tool" | "host" | "model" => AuditFact {
                id: id.clone(),
                kind: kind_.into(),
                label: format!("{kind_} {key}"),
                link: None,
            },
            "action" => AuditFact {
                id: id.clone(),
                kind: kind_.into(),
                label: key.replacen('/', " ", 1),
                link: None,
            },
            _ => AuditFact {
                id: id.clone(),
                kind: kind_.into(),
                label: id.clone(),
                link: None,
            },
        };
        out.push(fact);
    }
    Ok(out)
}

/// The workspace agent behind a principal: itself, or its parent for a sub-agent.
fn parent_principal(principal: &str) -> String {
    let key = facts::agent_key(principal);
    match key.split_once('/') {
        Some((workspace, rest)) => {
            let name = rest.split('/').next().unwrap_or(rest);
            format!("agent://{workspace}/{name}")
        }
        None => principal.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_canned_questions_are_recognised_with_their_slots() {
        let run = Uuid::now_v7();
        let cases: Vec<(String, &str)> = vec![
            (CANNED[0].label.into(), "restricted_reads"),
            (CANNED[1].label.into(), "egress_denied"),
            (
                CANNED[2].label.replace("{run}", &run.to_string()),
                "run_denied",
            ),
            (CANNED[3].label.replace("{agent}", "writer"), "agent_day"),
            (CANNED[4].label.into(), "code_policies"),
            (CANNED[5].label.into(), "approvals"),
            (
                "which agents accessed confidential documents in the last 3 days".into(),
                "restricted_reads",
            ),
            (format!("why was {run} refused?"), "run_denied"),
            ("What did the coder do yesterday?".into(), "agent_day"),
            (
                "Which agents reached a host that is not allowed this month?".into(),
                "egress_denied",
            ),
            ("Who approved anything last 2 weeks?".into(), "approvals"),
            ("which policies permit run_python".into(), "code_policies"),
        ];
        for (question, id) in cases {
            let matched =
                classify(&question).unwrap_or_else(|| panic!("{question} not recognised"));
            assert_eq!(matched.id, id, "{question}");
        }
        let m = classify(&CANNED[2].label.replace("{run}", &run.to_string())).unwrap();
        assert_eq!(m.run, Some(run));
        let m = classify("What did agent Writer do today?").unwrap();
        assert_eq!(m.agent.as_deref(), Some("writer"));
        assert_eq!(m.days, 1);
        let m =
            classify("which agents accessed confidential documents in the last 3 days").unwrap();
        assert_eq!(m.days, 3);
        assert_eq!(m.labels, vec!["confidential", "restricted"]);
        let m = classify("Who approved anything last 2 weeks?").unwrap();
        assert_eq!(m.days, 14);
        assert_eq!(classify(CANNED[0].label).unwrap().days, 7);
        assert_eq!(classify(CANNED[5].label).unwrap().days, 30);
        for free in [
            "Why is project alpha blocked?",
            "How many runs failed?",
            "Summarise what happened",
            "",
        ] {
            assert!(classify(free).is_none(), "{free}");
        }
    }

    #[test]
    fn entity_ids_are_found_in_fact_text() {
        assert_eq!(
            entity_ids_in("agent:w/writer read document:0190 (restricted), see run:abc."),
            vec!["agent:w/writer", "document:0190", "run:abc"]
        );
        assert!(entity_ids_in("Nothing here: just words").is_empty());
    }

    #[test]
    fn clearance_follows_the_role() {
        assert!(Clearance::of(WorkspaceRole::Owner).may_see("restricted"));
        assert!(Clearance::of(WorkspaceRole::Admin).may_see("confidential"));
        assert!(!Clearance::of(WorkspaceRole::Member).may_see("confidential"));
        assert!(Clearance::of(WorkspaceRole::Member).may_see("internal"));
        assert_eq!(
            parent_principal("agent://w/writer/researcher-1"),
            "agent://w/writer"
        );
        assert_eq!(parent_principal("agent://w/writer"), "agent://w/writer");
        assert_eq!(parent_principal("system"), "system");
    }
}
