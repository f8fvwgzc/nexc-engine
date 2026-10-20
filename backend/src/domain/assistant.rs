//! The workspace assistant: answers from the workspace's memory, can file
//! issues on the member's behalf, and can propose changes to the workspace's
//! ontology in onto-intelli (people approve them there).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::issue::Issue;

/// Name of the structured-output schema the assistant answers with.
pub const ASSISTANT_SCHEMA_NAME: &str = "assistant_reply";
/// Precedes the member's own words in the assistant's last message.
pub const USER_MESSAGE_MARKER: &str = "MEMBER_MESSAGE:";
/// Starts the line that lists the teams the member can file issues with.
pub const TEAMS_MARKER: &str = "Teams you can file issues with: ";
/// Longest message a member can send.
pub const MESSAGE_MAX: usize = 4_000;
/// Earlier turns kept as context.
pub const HISTORY_MAX: usize = 20;
/// Most issues one reply may file.
pub const ISSUES_PER_REPLY_MAX: usize = 5;
/// Heads the workspace's ontology schema in the assistant's context.
pub const SCHEMA_MARKER: &str = "## Ontology schema (onto-intelli)";
/// Most schema change proposals one reply may file.
pub const PROPOSALS_PER_REPLY_MAX: usize = 3;
/// Longest page path or title the client may report.
pub const PAGE_MAX: usize = 300;
/// A conversation is titled by its first message, cut to this many characters.
pub const CONVERSATION_TITLE_MAX: usize = 80;
/// Most conversations one listing returns, latest first.
pub const CONVERSATIONS_MAX: i64 = 200;

/// Where in the app the member is while they write; the assistant is told, so
/// "this graph" or "this issue" means something.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageContext {
    /// The address inside the app, e.g. `/app/graphs/<id>`.
    #[serde(default)]
    pub path: String,
    /// What the member sees it called, e.g. `Graphs › Research report`.
    #[serde(default)]
    pub title: String,
}

impl PageContext {
    pub fn is_empty(&self) -> bool {
        self.path.trim().is_empty() && self.title.trim().is_empty()
    }
}

/// An issue the assistant filed, as remembered with the conversation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct FiledIssue {
    pub id: Uuid,
    pub identifier: String,
    pub title: String,
}

/// A schema change proposal the assistant filed in onto-intelli, as
/// remembered with the conversation; people review it there.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct FiledProposal {
    /// onto-intelli's id (`prop_…`).
    pub id: String,
    pub title: String,
    /// Whether applying it would break data or vocabulary in use.
    pub breaking: bool,
    /// Where to review it in onto-intelli's UI; `null` when its address is unknown.
    #[schema(required = true)]
    pub review_url: Option<String>,
}

/// What an assistant turn did besides answering, kept with its message.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct TurnOutcome {
    #[serde(default)]
    pub created: Vec<FiledIssue>,
    #[serde(default)]
    pub skipped: Vec<String>,
    #[serde(default)]
    pub memories_used: usize,
    /// Schema change proposals it filed.
    #[serde(default)]
    pub proposals: Vec<FiledProposal>,
}

/// A saved conversation with the assistant: one member's, in one workspace.
#[derive(Debug, Clone, Serialize, ToSchema, sqlx::FromRow)]
pub struct Conversation {
    pub id: Uuid,
    pub workspace_id: Uuid,
    /// The first message, shortened.
    pub title: String,
    /// Where the member was when it started.
    pub page_path: String,
    pub page_title: String,
    pub message_count: i64,
    pub created_at: DateTime<Utc>,
    /// When the last message was added.
    pub updated_at: DateTime<Utc>,
}

/// One message of a saved conversation.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConversationMessage {
    pub id: Uuid,
    pub role: AssistantRole,
    pub content: String,
    /// For an assistant turn: what it did besides answering.
    pub outcome: Option<TurnOutcome>,
    /// For a user turn: where they were when they wrote it.
    pub page_path: String,
    pub page_title: String,
    pub created_at: DateTime<Utc>,
}

/// A conversation with all of its messages, oldest first.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ConversationDetail {
    pub conversation: Conversation,
    pub messages: Vec<ConversationMessage>,
}

/// An earlier turn of the conversation.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AssistantTurn {
    /// `user` or `assistant`.
    pub role: AssistantRole,
    pub content: String,
}

/// Who said a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssistantRole {
    User,
    Assistant,
}

impl AssistantRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            _ => None,
        }
    }
}

/// An issue the assistant wants filed.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct IssueDraft {
    pub team_key: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub priority: i16,
}

/// A schema change the assistant wants proposed: onto-intelli's change
/// objects, written as a JSON array (structured outputs take no free-form
/// objects).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ProposalDraft {
    pub title: String,
    #[serde(default)]
    pub rationale: String,
    /// `[{"op": "put_link_type", "name": …, "def": {…}}, …]`.
    #[serde(default)]
    pub changes_json: String,
}

/// What the model returns.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct AssistantOutput {
    pub reply: String,
    #[serde(default)]
    pub issues: Vec<IssueDraft>,
    #[serde(default)]
    pub schema_proposals: Vec<ProposalDraft>,
}

/// What the member gets back.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AssistantReply {
    /// The conversation this exchange was saved to; send it back to continue it.
    pub conversation_id: Uuid,
    pub reply: String,
    /// Issues the assistant filed while answering.
    pub created: Vec<Issue>,
    /// Drafts it could not file (unknown team, or no right to file there;
    /// a schema change onto-intelli refused), as text.
    pub skipped: Vec<String>,
    /// How many memories of the workspace it was given.
    pub memories_used: usize,
    /// Schema change proposals it filed in onto-intelli, for people to review there.
    pub proposals: Vec<FiledProposal>,
}
