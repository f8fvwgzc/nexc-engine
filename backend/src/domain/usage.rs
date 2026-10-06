//! Token usage: what was spent, by whom, on what, and on whose account.

use chrono::NaiveDate;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::settings::{ConfigScope, LlmProviderKind};
use super::string_enum;

string_enum!(
    /// What an LLM call was for.
    UsagePurpose {
        /// Refining a graph into a plan.
        Plan => "plan",
        /// Executing a node.
        Node => "node",
        /// Extracting memories from a node's output.
        Memory => "memory",
        /// A reply of the workspace assistant.
        Assistant => "assistant",
    }
);

/// One LLM call to record.
#[derive(Debug, Clone)]
pub struct UsageEvent {
    pub workspace_id: Option<Uuid>,
    pub user_id: Uuid,
    pub graph_id: Option<Uuid>,
    pub run_id: Option<Uuid>,
    pub purpose: UsagePurpose,
    pub provider: LlmProviderKind,
    pub model: String,
    /// Whose configuration supplied the credential.
    pub credential: ConfigScope,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cost_usd: f64,
    /// Characters of upstream context left out of the prompt (see `domain::context`).
    pub context_chars_saved: i64,
}

/// Sums over a set of calls.
#[derive(Debug, Clone, Default, PartialEq, Serialize, ToSchema, sqlx::FromRow)]
pub struct UsageTotals {
    pub calls: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cost_usd: f64,
    /// Characters of upstream context that were not sent because they were
    /// padding or irrelevant to the task. Roughly four characters make a token.
    pub context_chars_saved: i64,
}

/// Usage of one group (a day, a member, a model, ...).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UsageSlice<K> {
    pub key: K,
    #[serde(flatten)]
    pub totals: UsageTotals,
}

/// A member a slice of usage belongs to.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UsageMember {
    /// `null` for a member whose account was deleted.
    #[schema(required = true)]
    pub user_id: Option<Uuid>,
    pub name: String,
}

/// A provider and model a slice of usage ran on.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UsageModel {
    pub provider: String,
    pub model: String,
}

string_enum!(
    /// Whether a report covers the whole workspace or only the caller.
    UsageScope {
        Workspace => "workspace",
        Own => "own",
    }
);

/// Token usage of a workspace over the last `days` days.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UsageReport {
    /// `workspace` for admins; other members get a report of their own usage.
    pub scope: UsageScope,
    pub days: i64,
    pub totals: UsageTotals,
    /// Oldest first; days without usage are absent.
    pub by_day: Vec<UsageSlice<NaiveDate>>,
    /// Highest cost first, then most tokens.
    pub by_member: Vec<UsageSlice<UsageMember>>,
    pub by_model: Vec<UsageSlice<UsageModel>>,
    pub by_purpose: Vec<UsageSlice<UsagePurpose>>,
    /// Whose account paid: members' own, the workspace's, or the server's.
    pub by_credential: Vec<UsageSlice<ConfigScope>>,
}
