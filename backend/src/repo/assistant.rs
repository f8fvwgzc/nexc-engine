//! Saved conversations with the workspace assistant. A conversation belongs to
//! one member in one workspace; nobody else lists or reads it.

use chrono::{DateTime, Utc};
use sqlx::PgExecutor;
use sqlx::types::Json;
use uuid::Uuid;

use crate::domain::assistant::{
    AssistantRole, AssistantTurn, Conversation, ConversationMessage, PageContext, TurnOutcome,
};

macro_rules! columns {
    () => {
        "c.id, c.workspace_id, c.title, c.page_path, c.page_title,
         (SELECT count(*) FROM assistant_messages m WHERE m.conversation_id = c.id) AS message_count,
         c.created_at, c.updated_at"
    };
}

/// Starts a conversation for `user_id` in `workspace_id`.
pub async fn create(
    db: impl PgExecutor<'_>,
    id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    title: &str,
    page: &PageContext,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO assistant_conversations (id, workspace_id, user_id, title, page_path, page_title)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(workspace_id)
    .bind(user_id)
    .bind(title)
    .bind(&page.path)
    .bind(&page.title)
    .execute(db)
    .await?;
    Ok(())
}

/// One of `user_id`'s conversations in `workspace_id`, if `id` is one.
pub async fn find(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<Conversation>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM assistant_conversations c
          WHERE c.id = $1 AND c.user_id = $2 AND c.workspace_id = $3"
    ))
    .bind(id)
    .bind(user_id)
    .bind(workspace_id)
    .fetch_optional(db)
    .await
}

/// `user_id`'s conversations in `workspace_id`, most recently continued first.
pub async fn list(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
    limit: i64,
) -> Result<Vec<Conversation>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT ",
        columns!(),
        " FROM assistant_conversations c
          WHERE c.user_id = $1 AND c.workspace_id = $2
          ORDER BY c.updated_at DESC, c.id DESC LIMIT $3"
    ))
    .bind(user_id)
    .bind(workspace_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

type MessageRow = (
    Uuid,
    String,
    String,
    Option<Json<TurnOutcome>>,
    String,
    String,
    DateTime<Utc>,
);

fn message(row: MessageRow) -> ConversationMessage {
    let (id, role, content, outcome, page_path, page_title, created_at) = row;
    ConversationMessage {
        id,
        // The column is constrained to the two names; anything else would be a schema change.
        role: AssistantRole::parse(&role).unwrap_or(AssistantRole::Assistant),
        content,
        outcome: outcome.map(|Json(o)| o),
        page_path,
        page_title,
        created_at,
    }
}

/// Every message of a conversation, oldest first.
pub async fn messages(
    db: impl PgExecutor<'_>,
    conversation_id: Uuid,
) -> Result<Vec<ConversationMessage>, sqlx::Error> {
    let rows: Vec<MessageRow> = sqlx::query_as(
        "SELECT id, role, content, outcome, page_path, page_title, created_at
         FROM assistant_messages WHERE conversation_id = $1 ORDER BY created_at, id",
    )
    .bind(conversation_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(message).collect())
}

/// The last `limit` turns of a conversation, oldest first, as the model sees them.
pub async fn history(
    db: impl PgExecutor<'_>,
    conversation_id: Uuid,
    limit: i64,
) -> Result<Vec<AssistantTurn>, sqlx::Error> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT role, content FROM (
             SELECT role, content, created_at, id FROM assistant_messages
             WHERE conversation_id = $1 ORDER BY created_at DESC, id DESC LIMIT $2
         ) last ORDER BY created_at, id",
    )
    .bind(conversation_id)
    .bind(limit)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(role, content)| {
            AssistantRole::parse(&role).map(|role| AssistantTurn { role, content })
        })
        .collect())
}

/// Adds a message and marks the conversation as continued now.
pub async fn append(
    db: &mut sqlx::PgConnection,
    conversation_id: Uuid,
    role: AssistantRole,
    content: &str,
    outcome: Option<&TurnOutcome>,
    page: &PageContext,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO assistant_messages (id, conversation_id, role, content, outcome, page_path, page_title)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(conversation_id)
    .bind(role.as_str())
    .bind(content)
    .bind(outcome.map(Json))
    .bind(&page.path)
    .bind(&page.title)
    .execute(&mut *db)
    .await?;
    sqlx::query("UPDATE assistant_conversations SET updated_at = now() WHERE id = $1")
        .bind(conversation_id)
        .execute(&mut *db)
        .await?;
    Ok(id)
}

/// Removes one of `user_id`'s conversations with its messages; `false` when it is not theirs.
pub async fn delete(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "DELETE FROM assistant_conversations WHERE id = $1 AND user_id = $2 AND workspace_id = $3",
    )
    .bind(id)
    .bind(user_id)
    .bind(workspace_id)
    .execute(db)
    .await?;
    Ok(done.rows_affected() > 0)
}
