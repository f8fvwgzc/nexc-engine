//! Agents of the per-user organisation (Paperclip-style org chart).

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::string_enum;

/// Maximum agent name length.
pub const AGENT_NAME_MAX: usize = 64;
/// Maximum agent title length.
pub const AGENT_TITLE_MAX: usize = 120;
/// Maximum system prompt length.
pub const SYSTEM_PROMPT_MAX: usize = 16 * 1024;

string_enum!(
    /// Whether an agent accepts work.
    AgentStatus {
        Active => "active",
        Paused => "paused",
        OverBudget => "over_budget",
    }
);

string_enum!(
    /// Where an agent runs: the Python agent runtime or the built-in LLM executor.
    AgentRuntime {
        Python => "python",
        Builtin => "builtin",
    }
);

/// An agent in the user's organisation.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Agent {
    pub id: Uuid,
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
}

impl Agent {
    /// True when the agent may not take more work because of its budget.
    pub fn is_over_budget(&self) -> bool {
        self.budget_tokens > 0 && self.spent_tokens >= self.budget_tokens
    }
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
    fn role_validation() {
        assert!(is_valid_role("qa-lead_2"));
        assert!(!is_valid_role("QA"));
        assert!(!is_valid_role(""));
        assert!(!is_valid_role("a b"));
    }
}
