//! The workspace assistant: answers from the workspace's memory and can file
//! issues on the member's behalf.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

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

/// An earlier turn of the conversation.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AssistantTurn {
    /// `user` or `assistant`.
    pub role: AssistantRole,
    pub content: String,
}

/// Who said a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssistantRole {
    User,
    Assistant,
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

/// What the model returns.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct AssistantOutput {
    pub reply: String,
    #[serde(default)]
    pub issues: Vec<IssueDraft>,
}

/// What the member gets back.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AssistantReply {
    pub reply: String,
    /// Issues the assistant filed while answering.
    pub created: Vec<Issue>,
    /// Drafts it could not file (unknown team, or no right to file there), as text.
    pub skipped: Vec<String>,
    /// How many memories of the workspace it was given.
    pub memories_used: usize,
}
