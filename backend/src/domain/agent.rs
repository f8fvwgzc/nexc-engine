//! Agents of the per-user organisation (Paperclip-style org chart).

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::agent_template::AgentEvaluation;
use super::capabilities::Capabilities;
use super::string_enum;
use super::trace::Span;

/// Maximum agent name length.
pub const AGENT_NAME_MAX: usize = 64;
/// Maximum agent title length.
pub const AGENT_TITLE_MAX: usize = 120;
/// Maximum system prompt length.
pub const SYSTEM_PROMPT_MAX: usize = 16 * 1024;
/// Maximum purpose length.
pub const PURPOSE_MAX: usize = 2_000;
/// Spans on an agent's card.
pub const CARD_SPANS: i64 = 20;

string_enum!(
    /// Whether an agent accepts work.
    AgentStatus {
        Active => "active",
        Paused => "paused",
        OverBudget => "over_budget",
        /// Taken out of service by an admin: running nodes were cancelled, new ones are refused.
        Quarantined => "quarantined",
    }
);

string_enum!(
    /// Where an agent runs: the Python agent runtime, the built-in LLM
    /// executor, or somewhere else entirely (it uses nexc as its policy and
    /// trace service through the Agent Firewall API).
    AgentRuntime {
        Python => "python",
        Builtin => "builtin",
        External => "external",
    }
);

/// Longest name of an API token.
pub const TOKEN_NAME_MAX: usize = 100;

/// An API token of an external agent, as listed (the token itself is shown once).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AgentApiToken {
    pub id: Uuid,
    pub agent_id: Uuid,
    /// The token's first characters, to tell tokens apart.
    pub prefix: String,
    pub name: String,
    #[schema(required = true)]
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    #[schema(required = true)]
    pub last_used_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub revoked_at: Option<DateTime<Utc>>,
}

/// A token just issued: the only time its value is given out.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IssuedAgentApiToken {
    pub token: AgentApiToken,
    /// `nexc_agt_…`; keep it, it is not shown again.
    pub secret: String,
}

/// An agent in the user's organisation.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Agent {
    pub id: Uuid,
    /// The agent's identity across runs and spans: `agent://{workspace}/{name}`.
    pub principal_id: String,
    pub name: String,
    pub role: String,
    pub title: String,
    pub model: String,
    pub system_prompt: String,
    #[schema(required = true)]
    pub reports_to: Option<Uuid>,
    pub budget_tokens: i64,
    pub spent_tokens: i64,
    pub status: AgentStatus,
    pub runtime: AgentRuntime,
    #[schema(required = true)]
    pub heartbeat_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// The person answerable for the agent; `null` once their account is gone.
    #[schema(required = true)]
    pub owner_user_id: Option<Uuid>,
    #[schema(required = true)]
    pub team_id: Option<Uuid>,
    /// What the agent is for, in a paragraph.
    pub purpose: String,
    pub capabilities: Capabilities,
    /// Raised with every change to the capabilities, quarantine and release;
    /// the token of every run records the version that governed it.
    pub policy_version: i32,
    /// When the agent was quarantined; `null` unless it is.
    #[schema(required = true)]
    pub quarantined_at: Option<DateTime<Utc>>,
    /// How far its latest run strayed from what it usually does (0 to 10; 0 until
    /// three runs were learned from).
    pub risk_score: f64,
    /// Why, in words.
    pub risk_reasons: Vec<String>,
    #[schema(required = true)]
    pub risk_updated_at: Option<DateTime<Utc>>,
    /// Whether the agent is published as an A2A agent: its card is served
    /// and registered counterparts may send it tasks (admins set it; audited).
    pub a2a_exposed: bool,
}

impl Agent {
    /// True when the agent may not take more work because of its budget.
    pub fn is_over_budget(&self) -> bool {
        self.budget_tokens > 0 && self.spent_tokens >= self.budget_tokens
    }

    /// Tokens left before the budget is reached; `None` without a budget.
    pub fn budget_left(&self) -> Option<i64> {
        (self.budget_tokens > 0).then(|| (self.budget_tokens - self.spent_tokens).max(0))
    }
}

/// `agent://{workspace}/{name}`; an agent no workspace adopted is named after its creator.
pub fn principal_id(workspace: Option<Uuid>, owner: Uuid, name: &str) -> String {
    match workspace {
        Some(workspace) => format!("agent://{workspace}/{name}"),
        None => format!("agent://user-{owner}/{name}"),
    }
}

/// Who is answerable for an agent, as shown on its card.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AgentOwner {
    pub id: Uuid,
    pub name: String,
}

/// The team an agent belongs to, as shown on its card.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AgentTeam {
    pub id: Uuid,
    pub name: String,
    pub key: String,
}

/// The agent's ID card: the agent, who answers for it, its team, what it
/// has left to spend and what it did last.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AgentCard {
    pub agent: Agent,
    #[schema(required = true)]
    pub owner: Option<AgentOwner>,
    #[schema(required = true)]
    pub team: Option<AgentTeam>,
    /// Tokens left before the budget is reached; `null` without a budget.
    #[schema(required = true)]
    pub budget_left: Option<i64>,
    /// The agent's last spans across the runs the caller may open, newest first.
    pub recent_spans: Vec<Span>,
    /// The competency score of an agent made from a role template; `null` otherwise.
    #[schema(required = true)]
    pub evaluation: Option<AgentEvaluation>,
}

/// Template for one agent of the default organisation.
#[derive(Debug, Clone, Copy)]
pub struct AgentSeed {
    pub name: &'static str,
    pub role: &'static str,
    pub title: &'static str,
    pub runtime: AgentRuntime,
    pub budget_tokens: i64,
    /// Role of the manager, `None` for the head of the organisation.
    pub reports_to: Option<&'static str>,
    pub system_prompt: &'static str,
    /// What the agent is for, on its card.
    pub purpose: &'static str,
}

/// The organisation every new user starts with.
pub const DEFAULT_ORG: [AgentSeed; 5] = [
    AgentSeed {
        name: "Planner",
        role: "planner",
        title: "Chief Planner",
        runtime: AgentRuntime::Builtin,
        budget_tokens: 2_000_000,
        reports_to: None,
        system_prompt: "You are the chief planner of a small expert team. Break goals into clear, \
            verifiable steps, name assumptions explicitly and keep every step actionable.",
        purpose: "Plans the work: turns a goal into a graph of verifiable steps.",
    },
    AgentSeed {
        name: "Researcher",
        role: "researcher",
        title: "Research Lead",
        runtime: AgentRuntime::Python,
        budget_tokens: 2_000_000,
        reports_to: Some("planner"),
        system_prompt: "You are a meticulous researcher. Gather facts, compare sources, separate \
            evidence from opinion and summarise findings with their confidence.",
        purpose: "Gathers and weighs evidence for the other agents to build on.",
    },
    AgentSeed {
        name: "Writer",
        role: "writer",
        title: "Lead Writer",
        runtime: AgentRuntime::Builtin,
        budget_tokens: 2_000_000,
        reports_to: Some("planner"),
        system_prompt: "You are a precise technical writer. Produce well-structured Markdown with \
            headings, short paragraphs and concrete examples. Build on the upstream results.",
        purpose: "Writes the deliverables from the upstream results.",
    },
    AgentSeed {
        name: "Engineer",
        role: "engineer",
        title: "Staff Engineer",
        runtime: AgentRuntime::Python,
        budget_tokens: 2_000_000,
        reports_to: Some("planner"),
        system_prompt: "You are a pragmatic senior engineer. Prefer simple, tested, secure \
            solutions and explain trade-offs briefly.",
        purpose: "Builds and checks the technical parts of a plan.",
    },
    AgentSeed {
        name: "Reviewer",
        role: "reviewer",
        title: "Quality Reviewer",
        runtime: AgentRuntime::Builtin,
        budget_tokens: 1_000_000,
        reports_to: Some("planner"),
        system_prompt: "You are a demanding reviewer. Check the work against the goal, list \
            concrete problems by severity and propose fixes.",
        purpose: "Reviews the work against the goal before it ships.",
    },
];

/// Default system prompt for nodes without an assigned agent.
pub const FALLBACK_SYSTEM_PROMPT: &str = "You are a capable assistant completing one step of a \
    larger plan. Use the upstream results, stay on task and answer in Markdown.";

/// Valid agent roles: lowercase ascii letters, digits, `_` and `-`.
pub fn is_valid_role(role: &str) -> bool {
    !role.is_empty()
        && role.len() <= super::graph::ROLE_MAX
        && role
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_org_is_a_tree_under_the_planner() {
        let roles: Vec<_> = DEFAULT_ORG.iter().map(|a| a.role).collect();
        assert_eq!(
            DEFAULT_ORG
                .iter()
                .filter(|a| a.reports_to.is_none())
                .count(),
            1
        );
        for a in DEFAULT_ORG {
            assert!(is_valid_role(a.role));
            if let Some(m) = a.reports_to {
                assert!(roles.contains(&m));
            }
        }
    }

    #[test]
    fn principals_name_the_workspace_and_the_agent() {
        let (ws, owner) = (Uuid::now_v7(), Uuid::now_v7());
        assert_eq!(
            principal_id(Some(ws), owner, "Researcher"),
            format!("agent://{ws}/Researcher")
        );
        assert_eq!(
            principal_id(None, owner, "Researcher"),
            format!("agent://user-{owner}/Researcher")
        );
    }

    #[test]
    fn role_validation() {
        assert!(is_valid_role("qa-lead_2"));
        assert!(!is_valid_role("QA"));
        assert!(!is_valid_role(""));
        assert!(!is_valid_role("a b"));
    }
}
