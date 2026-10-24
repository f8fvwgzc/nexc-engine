//! The A2A agent card of an exposed agent: its name, its purpose, the
//! skills its role template and capabilities give it, where tasks are sent
//! and how callers authenticate.

use serde_json::{Value, json};
use uuid::Uuid;

use super::endpoint_url;
use crate::app::AppState;
use crate::domain::agent::Agent;
use crate::domain::{agent_template, skill};

/// What a built-in tool does, for the card's skills.
fn tool_description(tool: &str) -> String {
    match tool {
        "read_file" => "Reads a file of the task's workspace.".into(),
        "list_files" => "Lists the files of the task's workspace.".into(),
        "write_file" => "Writes a file the task delivers.".into(),
        "make_docx" => "Delivers a Word document (.docx).".into(),
        "run_python" => "Runs Python in a sandbox.".into(),
        "spawn_subagent" => "Delegates part of the task to a sub-agent.".into(),
        "fetch_url" => "Reads a web page through the platform's gateway.".into(),
        "finish" => "Ends the task with its answer.".into(),
        other => match other.strip_prefix("mcp:") {
            Some(rest) => format!("Calls the MCP tool {rest} through the platform's gateway."),
            None => match other.strip_prefix("a2a:") {
                Some(name) => format!("Sends a task to the A2A counterpart {name}."),
                None => format!("Uses the tool {other}."),
            },
        },
    }
}

/// The agent card (A2A `AgentCard`).
pub async fn build(state: &AppState, agent: &Agent, workspace_id: Uuid) -> anyhow::Result<Value> {
    let workspace_name: Option<String> =
        sqlx::query_scalar("SELECT name FROM workspaces WHERE id = $1")
            .bind(workspace_id)
            .fetch_optional(&state.db)
            .await?;
    let resolved = state
        .caches
        .resolved(&state.db, workspace_id, agent)
        .await?;
    let resolved = &resolved.resolved;
    let template = agent
        .template_id
        .as_deref()
        .and_then(agent_template::find)
        .or_else(|| {
            agent_template::builtin()
                .iter()
                .find(|t| t.role == agent.role)
        });
    let mut skills = vec![json!({
        "id": format!("role:{}", agent.role),
        "name": if agent.title.is_empty() { agent.role.clone() } else { agent.title.clone() },
        "description": template.map_or_else(
            || if agent.purpose.is_empty() { format!("Works as a {}.", agent.role) } else { agent.purpose.clone() },
            |t| t.description.clone(),
        ),
        "tags": template.map_or_else(|| vec![agent.role.clone()], |t| vec![agent.role.clone(), t.id.clone()]),
        "examples": template.map(|t| t.questions.iter().map(|q| q.question.clone()).collect::<Vec<_>>()).unwrap_or_default(),
        "inputModes": ["text", "file"],
        "outputModes": ["text", "file"],
    })];
    // Its skills: the working knowledge it brings to a task.
    for skill in agent.skills.iter().filter_map(|id| skill::find(id)) {
        let mut tags = vec![skill.category.clone()];
        tags.extend(skill.keywords.iter().take(6).cloned());
        skills.push(json!({
            "id": format!("skill:{}", skill.id),
            "name": skill.name,
            "description": skill.description,
            "tags": tags,
            "inputModes": ["text", "file"],
            "outputModes": ["text", "file"],
        }));
    }
    if let Some(tools) = &resolved.capabilities.tools {
        for tool in tools {
            skills.push(json!({
                "id": format!("tool:{tool}"),
                "name": tool,
                "description": tool_description(tool),
                "tags": ["tool"],
            }));
        }
    }
    Ok(json!({
        "name": agent.name,
        "description": if agent.purpose.is_empty() { agent.title.clone() } else { agent.purpose.clone() },
        "url": endpoint_url(state, &agent.principal_id),
        "provider": {
            "organization": workspace_name.unwrap_or_else(|| "nexc-engine".into()),
            "url": state.settings.public_url,
        },
        "version": format!("{}.{}", env!("CARGO_PKG_VERSION"), agent.policy_version),
        "documentationUrl": format!("{}/api/docs", state.settings.public_url),
        "capabilities": {"streaming": true, "pushNotifications": false, "stateTransitionHistory": false},
        "authentication": {
            "schemes": ["Bearer"],
            "credentials": "an agent API token (nexc_agt_…) issued to the caller's registration as an A2A counterpart of this workspace",
        },
        "defaultInputModes": ["text", "file"],
        "defaultOutputModes": ["text", "file"],
        "skills": skills,
        "principal": agent.principal_id,
    }))
}
