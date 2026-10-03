//! Single-use realtime tickets, stored in the database so that a ticket
//! issued by one backend replica can be redeemed on another. Only the
//! SHA-256 digest of a ticket is stored.

use std::time::Duration;

use sqlx::PgExecutor;
use uuid::Uuid;

use crate::security::random::{random_token, token_digest};

/// Ticket lifetime.
pub const TICKET_TTL: Duration = Duration::from_secs(30);

/// What a ticket grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::FromRow)]
pub struct TicketGrant {
    pub user_id: Uuid,
    pub graph_id: Uuid,
}

/// Issues a ticket for `grant`; returns the ticket string.
pub async fn issue(db: impl PgExecutor<'_>, grant: TicketGrant) -> Result<String, sqlx::Error> {
    let ticket = random_token();
    sqlx::query(
        "INSERT INTO realtime_tickets (ticket_hash, user_id, graph_id, expires_at)
         VALUES ($1, $2, $3, now() + make_interval(secs => $4))",
    )
    .bind(token_digest(&ticket))
    .bind(grant.user_id)
    .bind(grant.graph_id)
    .bind(TICKET_TTL.as_secs_f64())
    .execute(db)
    .await?;
    Ok(ticket)
}

/// Consumes `ticket` atomically; returns its grant if it existed and was unexpired.
pub async fn redeem(
    db: impl PgExecutor<'_>,
    ticket: &str,
) -> Result<Option<TicketGrant>, sqlx::Error> {
    sqlx::query_as(
        "DELETE FROM realtime_tickets WHERE ticket_hash = $1 AND expires_at > now() RETURNING user_id, graph_id",
    )
    .bind(token_digest(ticket))
    .fetch_optional(db)
    .await
}

/// Deletes expired tickets.
pub async fn purge_expired(db: impl PgExecutor<'_>) -> Result<u64, sqlx::Error> {
    let done = sqlx::query("DELETE FROM realtime_tickets WHERE expires_at <= now()")
        .execute(db)
        .await?;
    Ok(done.rows_affected())
}
