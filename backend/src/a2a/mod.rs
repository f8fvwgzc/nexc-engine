//! The A2A (Agent-to-Agent protocol) adapter, on the firewall: an agent an
//! admin exposes publishes an agent card and takes tasks over JSON-RPC from
//! registered counterparts, each task a one-node run of the agent with the
//! caller, the firewall's decision and the hop it sits at on the run's
//! trace; and agents call counterparts back as the gateway tool
//! `a2a:<name>`, under the policy, with the hops carried in the task's
//! metadata so depth is bounded and loops refused across platforms.
#![forbid(unsafe_code)]

pub mod card;
pub mod client;
pub mod inbound;

use axum::http::HeaderMap;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::a2a::{E_AGENT_NOT_FOUND, E_FORBIDDEN, E_UNAUTHENTICATED, RpcError};
use crate::domain::agent::{Agent, AgentStatus};
use crate::http::extract::bearer_token;
use crate::repo;
use crate::repo::a2a::CounterpartRow;
use crate::security::api_token;

/// Who is calling: the counterpart its token names, as the external agent
/// principal registered for it.
#[derive(Clone)]
pub struct Caller {
    pub agent: Agent,
    pub counterpart: CounterpartRow,
    pub workspace_id: Uuid,
}

/// The caller a request's bearer token names: a live API token of an
/// external agent that is a registered counterpart, in service.
pub async fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<Caller, RpcError> {
    let presented = bearer_token(headers)
        .ok_or_else(|| RpcError::new(E_UNAUTHENTICATED, "an agent API token is required"))?;
    if !api_token::looks_like_token(presented) {
        return Err(RpcError::new(E_UNAUTHENTICATED, "invalid agent API token"));
    }
    let agent = repo::agent_tokens::authenticate(&state.db, &api_token::hash(presented))
        .await
        .map_err(internal)?
        .ok_or_else(|| RpcError::new(E_UNAUTHENTICATED, "invalid agent API token"))?;
    match agent.status {
        AgentStatus::Quarantined => {
            return Err(RpcError::new(E_FORBIDDEN, "the counterpart is quarantined"));
        }
        AgentStatus::Paused => return Err(RpcError::new(E_FORBIDDEN, "the counterpart is paused")),
        AgentStatus::Active | AgentStatus::OverBudget => {}
    }
    let counterpart = repo::a2a::find_counterpart_by_agent(&state.db, agent.id)
        .await
        .map_err(internal)?
        .ok_or_else(|| {
            RpcError::new(
                E_FORBIDDEN,
                "the token's agent is not a registered A2A counterpart",
            )
        })?;
    Ok(Caller {
        workspace_id: counterpart.counterpart.workspace_id,
        agent,
        counterpart,
    })
}

/// The exposed agent at `address`: its principal (`agent://{workspace}/{name}`,
/// percent-encoded or not) or its id. An agent that is not exposed, or is in
/// another workspace than `workspace`, is as good as missing.
pub async fn exposed_agent(
    state: &AppState,
    address: &str,
    workspace: Option<Uuid>,
) -> Result<(Agent, Uuid), RpcError> {
    let not_found = || RpcError::new(E_AGENT_NOT_FOUND, "no exposed agent at that address");
    let address = percent_decode(address);
    let agent = match address.parse::<Uuid>() {
        Ok(id) => {
            let workspace = repo::agents::workspace_of(&state.db, id)
                .await
                .map_err(internal)?
                .flatten()
                .ok_or_else(not_found)?;
            repo::agents::find(&state.db, workspace, id)
                .await
                .map_err(internal)?
        }
        Err(_) => {
            let workspace = address
                .strip_prefix("agent://")
                .and_then(|rest| rest.split_once('/'))
                .and_then(|(workspace, _)| workspace.parse::<Uuid>().ok())
                .ok_or_else(not_found)?;
            repo::agents::find_by_principal(&state.db, workspace, &address)
                .await
                .map_err(internal)?
        }
    };
    let agent = agent.filter(|a| a.a2a_exposed).ok_or_else(not_found)?;
    let agent_workspace = agent
        .principal_id
        .strip_prefix("agent://")
        .and_then(|rest| rest.split_once('/'))
        .and_then(|(workspace, _)| workspace.parse::<Uuid>().ok())
        .ok_or_else(not_found)?;
    if workspace.is_some_and(|w| w != agent_workspace) {
        return Err(not_found());
    }
    Ok((agent, agent_workspace))
}

/// `%3A%2F%2F` and friends decoded; anything that is not a percent-escape is kept.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A principal as it stands in a path: `agent%3A%2F%2F{workspace}%2F{name}`.
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Where the JSON-RPC endpoint of an exposed agent is.
pub fn endpoint_url(state: &AppState, principal: &str) -> String {
    format!(
        "{}/a2a/agents/{}",
        state.settings.public_url,
        percent_encode(principal)
    )
}

pub(crate) fn internal(err: impl std::fmt::Display) -> RpcError {
    tracing::error!(error = %err, "A2A request failed");
    RpcError::new(
        crate::domain::a2a::E_INTERNAL,
        "the request could not be served",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principals_round_trip_through_paths() {
        let principal = "agent://0199f3a0-6b1e-7c2d-9e4f-1a2b3c4d5e6f/Analyst (demo)";
        let encoded = percent_encode(principal);
        assert!(encoded.starts_with("agent%3A%2F%2F"));
        assert!(!encoded.contains(' ') && !encoded.contains('/'));
        assert_eq!(percent_decode(&encoded), principal);
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
