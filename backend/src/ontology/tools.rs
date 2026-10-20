//! The assistant's ontology tools. The assistant works for a member as an
//! agent principal of its own (`assistant:{user id}`, role `agent`): it
//! reads the schema (`get_schema`), dry-runs a change (`plan_schema_change`)
//! and files a proposal with its rationale (`propose_schema_change`). It
//! never applies or approves a change: onto-intelli refuses agents that, and
//! people review proposals in onto-intelli's inbox.

use serde_json::Value;
use uuid::Uuid;

use super::{OntoError, OntologyBackend, Principal, SchemaChange, SchemaProposalDraft};

/// What proposals filed by the assistant say they come from.
pub const SOURCE: &str = "nexc-assistant";
/// Longest schema summary put before the model.
const SUMMARY_MAX: usize = 6_000;

/// The assistant's principal for `user` in a workspace.
pub fn principal(prefix: &str, workspace_id: Uuid, user: Uuid) -> Principal {
    Principal::assistant(prefix, workspace_id, user)
}

/// `get_schema`: the tenant's active schema document.
pub async fn get_schema(
    backend: &dyn OntologyBackend,
    assistant: &Principal,
) -> Result<Value, OntoError> {
    backend.schema(assistant).await
}

/// `plan_schema_change`: validation, diff and impact of a change set, nothing applied.
pub async fn plan_schema_change(
    backend: &dyn OntologyBackend,
    assistant: &Principal,
    changes: Vec<Value>,
) -> Result<Value, OntoError> {
    backend
        .plan_schema(
            assistant,
            &SchemaChange {
                changes,
                ..SchemaChange::default()
            },
        )
        .await
}

/// `propose_schema_change`: files the change set with its rationale, for
/// people to review in onto-intelli.
pub async fn propose_schema_change(
    backend: &dyn OntologyBackend,
    assistant: &Principal,
    title: &str,
    rationale: &str,
    changes: Vec<Value>,
) -> Result<Value, OntoError> {
    backend
        .propose_schema(
            assistant,
            &SchemaProposalDraft {
                title: title.to_owned(),
                rationale: rationale.to_owned(),
                changes,
                migrations: Vec::new(),
                source: SOURCE.to_owned(),
            },
        )
        .await
}

/// The schema as the model is shown it: version, kinds (with their states),
/// link types (from → to) and action types, one line each, bounded.
pub fn summary(schema: &Value) -> String {
    let names = |key: &str| -> Vec<(String, Value)> {
        schema[key]
            .as_object()
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    };
    let list = |v: &Value| -> String {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "any".to_owned())
    };
    let mut out = format!(
        "Schema version {}\n",
        schema["schema_version"].as_u64().unwrap_or(0)
    );
    out.push_str("Kinds: ");
    let kinds: Vec<String> = names("object_types")
        .into_iter()
        .map(|(name, def)| match def["states"].as_array() {
            Some(states) if !states.is_empty() => format!("{name}[{}]", list(&def["states"])),
            _ => name,
        })
        .collect();
    out.push_str(&kinds.join(", "));
    out.push_str("\nLink types: ");
    let links: Vec<String> = names("link_types")
        .into_iter()
        .map(|(name, def)| format!("{name} ({} -> {})", list(&def["from"]), list(&def["to"])))
        .collect();
    out.push_str(&links.join(", "));
    out.push_str("\nAction types: ");
    let actions: Vec<String> = names("action_types").into_iter().map(|(n, _)| n).collect();
    out.push_str(&actions.join(", "));
    out.push('\n');
    if out.chars().count() > SUMMARY_MAX {
        out = out.chars().take(SUMMARY_MAX).collect();
        out.push_str("…\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_summary_names_kinds_links_and_actions() {
        let schema = json!({
            "schema_version": 3,
            "object_types": {"issue": {"states": ["open", "closed"]}, "team": {}},
            "link_types": {"member_of": {"from": ["person"], "to": ["team"]}, "relates_to": {}},
            "action_types": {"assign": {}},
        });
        let text = summary(&schema);
        assert!(text.contains("Schema version 3"));
        assert!(text.contains("issue[open|closed]"));
        assert!(text.contains("member_of (person -> team)"));
        assert!(text.contains("relates_to (any -> any)"));
        assert!(text.contains("assign"));
    }
}
