//! Loads a workspace's guardrails and applies them before tokens are spent.

use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::guardrails::Guardrails;
use crate::domain::settings::LlmProviderKind;
use crate::repo;

/// The policy of a workspace; the permissive default when it set none, or
/// for work that belongs to no workspace.
pub async fn load(state: &AppState, workspace_id: Option<Uuid>) -> Result<Guardrails, AppError> {
    match workspace_id {
        Some(id) => Ok(repo::workspaces::guardrails(&state.db, id)
            .await?
            .unwrap_or_default()),
        None => Ok(Guardrails::default()),
    }
}

/// Decides whether `user` may start work in `workspace_id` on `provider`
/// now: the provider must be allowed and this month's budgets not used up.
/// Returns the policy so the caller can apply the rest of it.
pub async fn admit(
    state: &AppState,
    workspace_id: Option<Uuid>,
    user: Uuid,
    provider: LlmProviderKind,
) -> Result<Guardrails, AppError> {
    let policy = load(state, workspace_id).await?;
    if let Some(reason) = policy.provider_refusal(provider) {
        return Err(AppError::Forbidden(reason));
    }
    if let Some(workspace) = workspace_id
        && (policy.monthly_token_budget.is_some() || policy.member_monthly_token_budget.is_some())
    {
        let (workspace_used, member_used) =
            repo::usage::tokens_this_month(&state.db, workspace, user).await?;
        if let Some(reason) = policy.budget_refusal(workspace_used, member_used) {
            return Err(AppError::Forbidden(reason));
        }
    }
    Ok(policy)
}
