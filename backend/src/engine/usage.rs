//! Writes the usage ledger. Recording never fails the work that spent the
//! tokens: a ledger error is logged and the call goes on.

use crate::app::AppState;
use crate::domain::usage::UsageEvent;
use crate::repo;

/// Appends `event` to the ledger unless it spent nothing.
pub async fn record(state: &AppState, event: UsageEvent) {
    if event.tokens_in <= 0 && event.tokens_out <= 0 {
        return;
    }
    if let Err(err) = repo::usage::insert(&state.db, &event).await {
        tracing::error!(error = %err, purpose = %event.purpose, "cannot record LLM usage");
    }
}
