//! What a delivery carries: the JSON envelope a webhook receives (and is
//! signed over), the Slack message rendered from it, and the signature.

use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Value, json};
use sha2::Sha256;
use uuid::Uuid;

use crate::domain::alert::{Alert, AlertKind};
use crate::domain::alert_routing::DeliveryEvent;
use crate::domain::approval::{Approval, ApprovalStatus};
use crate::domain::policy_document::Severity;

/// Longest free text (an alert's detail, an approval's reason) carried in a delivery.
pub const TEXT_MAX: usize = 4_000;
/// Slack's limit on a section's text.
const SLACK_SECTION_MAX: usize = 2_900;
/// Slack's limit on a header.
const SLACK_HEADER_MAX: usize = 140;

/// The workspace a delivery is about.
#[derive(Debug, Clone)]
pub struct WorkspaceLabel {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
}

/// What a delivery is about.
#[derive(Debug, Clone, Copy)]
pub enum Subject<'a> {
    Alert(&'a Alert),
    Approval(&'a Approval),
    /// `POST …/test`.
    Test,
}

/// `sha256=<hex HMAC-SHA256 of body under secret>`: the value of `X-Nexc-Signature`.
pub fn sign(secret: &[u8], body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC takes any key length");
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// The display name of a principal: `agent://{workspace}/{name}` → `{name}`.
pub fn agent_name(principal: &str) -> &str {
    principal.rsplit('/').next().unwrap_or(principal)
}

fn agent_of(id: Option<Uuid>, principal: &str) -> Value {
    json!({
        "id": id,
        "principal": principal,
        "name": agent_name(principal),
    })
}

fn links(base: &str, run_id: Option<Uuid>, span_id: Option<Uuid>, agent_id: Option<Uuid>) -> Value {
    let run = run_id.map(|run| match span_id {
        Some(span) => format!("{base}/app/runs/{run}?span={span}"),
        None => format!("{base}/app/runs/{run}"),
    });
    json!({
        "console": format!("{base}/app/secops"),
        "run": run,
        "agent": agent_id.map(|id| format!("{base}/app/agents?agent={id}")),
    })
}

/// The envelope of one delivery: `id` is its idempotency id, `timestamp`
/// when the thing happened, and the alert or approval as the API returns it
/// (free text cut to [`TEXT_MAX`]).
pub fn envelope(
    id: Uuid,
    event: DeliveryEvent,
    at: DateTime<Utc>,
    workspace: &WorkspaceLabel,
    subject: Subject<'_>,
    public_url: &str,
) -> Value {
    let mut body = json!({
        "id": id,
        "event": event,
        "timestamp": at,
        "workspace": {"id": workspace.id, "name": workspace.name, "slug": workspace.slug},
    });
    let map = body.as_object_mut().expect("an object");
    match subject {
        Subject::Alert(alert) => {
            let mut value = serde_json::to_value(alert).expect("an alert serialises");
            value["detail"] = Value::String(cut(&alert.detail, TEXT_MAX));
            map.insert("agent".into(), agent_of(alert.agent_id, &alert.principal));
            map.insert("alert".into(), value);
            map.insert(
                "links".into(),
                links(public_url, alert.run_id, alert.span_id, alert.agent_id),
            );
        }
        Subject::Approval(approval) => {
            let mut value = serde_json::to_value(approval).expect("an approval serialises");
            value["reason"] = Value::String(cut(&approval.reason, TEXT_MAX));
            value["decision_reason"] = Value::String(cut(&approval.decision_reason, TEXT_MAX));
            map.insert(
                "agent".into(),
                agent_of(approval.agent_id, &approval.principal),
            );
            map.insert("approval".into(), value);
            map.insert(
                "links".into(),
                links(
                    public_url,
                    Some(approval.run_id),
                    Some(approval.span_id),
                    approval.agent_id,
                ),
            );
        }
        Subject::Test => {
            map.insert("agent".into(), Value::Null);
            map.insert(
                "message".into(),
                Value::String(format!(
                    "A test from nexc-engine: alerts of {} reach this destination.",
                    workspace.name
                )),
            );
            map.insert("links".into(), links(public_url, None, None, None));
        }
    }
    body
}

/// The colour bar of a Slack message for a severity.
pub fn severity_colour(severity: Severity) -> &'static str {
    match severity {
        Severity::Low => "#64748b",
        Severity::Medium => "#f59e0b",
        Severity::High => "#ea580c",
        Severity::Critical => "#dc2626",
    }
}

fn kind_label(kind: AlertKind) -> &'static str {
    match kind {
        AlertKind::Dlp => "Data leaving",
        AlertKind::Risk => "Behaviour",
        AlertKind::Injection => "Injection",
        AlertKind::Policy => "Policy",
    }
}

fn header(text: &str) -> Value {
    json!({"type": "header", "text": {"type": "plain_text", "text": cut(text, SLACK_HEADER_MAX), "emoji": false}})
}

fn fields(pairs: &[(&str, String)]) -> Value {
    json!({
        "type": "section",
        "fields": pairs.iter().map(|(label, value)| json!({
            "type": "mrkdwn",
            "text": format!("*{label}*\n{}", cut(value, 500)),
        })).collect::<Vec<_>>(),
    })
}

fn section(text: &str) -> Value {
    json!({"type": "section", "text": {"type": "mrkdwn", "text": cut(text, SLACK_SECTION_MAX)}})
}

fn buttons(links: &Value) -> Option<Value> {
    let mut elements = Vec::new();
    for (label, key) in [
        ("Open in console", "console"),
        ("Open run", "run"),
        ("Open agent", "agent"),
    ] {
        if let Some(url) = links[key].as_str() {
            elements.push(json!({
                "type": "button",
                "text": {"type": "plain_text", "text": label, "emoji": false},
                "url": url,
            }));
        }
    }
    (!elements.is_empty()).then(|| json!({"type": "actions", "elements": elements}))
}

/// A Slack message (Block Kit, in one coloured attachment) for the envelope
/// of a delivery: title, agent, rule, and buttons that link into the console.
pub fn slack_message(envelope: &Value) -> Value {
    let workspace = envelope["workspace"]["name"].as_str().unwrap_or("");
    let agent = envelope["agent"]["name"].as_str().unwrap_or("nobody");
    let links = &envelope["links"];
    let (text, colour, mut blocks) = if let Some(alert) = envelope.get("alert") {
        let severity: Severity =
            serde_json::from_value(alert["severity"].clone()).unwrap_or(Severity::Low);
        let kind: AlertKind =
            serde_json::from_value(alert["kind"].clone()).unwrap_or(AlertKind::Policy);
        let title = alert["title"].as_str().unwrap_or("alert");
        let rule = alert["rule"].as_str().unwrap_or("");
        let detail = alert["detail"].as_str().unwrap_or("");
        let mut blocks = vec![
            header(&format!("{}: {title}", kind_label(kind))),
            fields(&[
                ("Severity", severity.to_string()),
                ("Agent", agent.to_owned()),
                ("Rule", format!("`{rule}`")),
                ("Workspace", workspace.to_owned()),
            ]),
        ];
        if !detail.is_empty() {
            blocks.push(section(detail));
        }
        (
            format!("[{severity}] {}: {title} — {agent}", kind_label(kind)),
            severity_colour(severity),
            blocks,
        )
    } else if let Some(approval) = envelope.get("approval") {
        let status: ApprovalStatus =
            serde_json::from_value(approval["status"].clone()).unwrap_or(ApprovalStatus::Pending);
        let tool = approval["tool"].as_str().unwrap_or("a tool");
        let rule = approval["rule"].as_str().unwrap_or("");
        let decider = approval["decided_by"]["name"].as_str().unwrap_or("");
        let (title, colour) = match status {
            ApprovalStatus::Pending => (format!("Approval requested: {tool}"), "#f59e0b"),
            ApprovalStatus::Approved => (format!("Approved: {tool}"), "#16a34a"),
            ApprovalStatus::Denied => (format!("Denied: {tool}"), "#dc2626"),
            ApprovalStatus::Expired => (format!("Expired: {tool}"), "#64748b"),
        };
        let mut pairs = vec![
            ("Agent", agent.to_owned()),
            ("Rule", format!("`{rule}`")),
            ("Workspace", workspace.to_owned()),
        ];
        if status == ApprovalStatus::Pending {
            pairs.push((
                "Decide by",
                approval["expires_at"].as_str().unwrap_or("").to_owned(),
            ));
        } else if !decider.is_empty() {
            pairs.push(("Decided by", decider.to_owned()));
        }
        let mut blocks = vec![header(&title), fields(&pairs)];
        let reason = if status == ApprovalStatus::Pending {
            approval["reason"].as_str().unwrap_or("")
        } else {
            approval["decision_reason"].as_str().unwrap_or("")
        };
        if !reason.is_empty() {
            blocks.push(section(reason));
        }
        (format!("{title} — {agent}"), colour, blocks)
    } else {
        let message = envelope["message"]
            .as_str()
            .unwrap_or("A test from nexc-engine.");
        (
            message.to_owned(),
            "#2563eb",
            vec![header("nexc-engine: test delivery"), section(message)],
        )
    };
    if let Some(actions) = buttons(links) {
        blocks.push(actions);
    }
    json!({
        "text": cut(&text, 300),
        "attachments": [{"color": colour, "blocks": blocks}],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policy::PolicyVersions;

    fn workspace() -> WorkspaceLabel {
        WorkspaceLabel {
            id: Uuid::now_v7(),
            name: "Acme".into(),
            slug: "acme".into(),
        }
    }

    fn alert() -> Alert {
        Alert {
            id: Uuid::now_v7(),
            workspace_id: Uuid::now_v7(),
            agent_id: Some(Uuid::now_v7()),
            principal: "agent://acme/Writer".into(),
            run_id: Some(Uuid::now_v7()),
            node_id: None,
            graph_id: None,
            span_id: Some(Uuid::now_v7()),
            kind: AlertKind::Dlp,
            severity: Severity::High,
            rule: "dlp.secrets".into(),
            title: "artifact out.md kept in".into(),
            detail: "x".repeat(10_000),
            created_at: Utc::now(),
            acknowledged_at: None,
            acknowledged_by: None,
            deliveries: 0,
        }
    }

    #[test]
    fn signatures_are_hmac_sha256_of_the_body() {
        let sig = sign(b"key", b"The quick brown fox jumps over the lazy dog");
        assert_eq!(
            sig,
            "sha256=f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
        assert_ne!(sign(b"other", b"x"), sign(b"key", b"x"));
    }

    #[test]
    fn the_envelope_carries_the_alert_links_and_identity() {
        let a = alert();
        let id = Uuid::now_v7();
        let body = envelope(
            id,
            DeliveryEvent::AlertRaised,
            a.created_at,
            &workspace(),
            Subject::Alert(&a),
            "https://nexc.example.com",
        );
        assert_eq!(body["id"], id.to_string());
        assert_eq!(body["event"], "alert.raised");
        assert_eq!(body["workspace"]["slug"], "acme");
        assert_eq!(body["agent"]["name"], "Writer");
        assert_eq!(body["alert"]["rule"], "dlp.secrets");
        assert_eq!(
            body["alert"]["detail"].as_str().unwrap().chars().count(),
            TEXT_MAX,
            "free text is cut"
        );
        assert_eq!(
            body["links"]["console"],
            "https://nexc.example.com/app/secops"
        );
        assert_eq!(
            body["links"]["run"],
            format!(
                "https://nexc.example.com/app/runs/{}?span={}",
                a.run_id.unwrap(),
                a.span_id.unwrap()
            )
        );
        let slack = slack_message(&body);
        assert_eq!(
            slack["attachments"][0]["color"],
            severity_colour(Severity::High)
        );
        let blocks = slack["attachments"][0]["blocks"].as_array().unwrap();
        assert_eq!(blocks[0]["type"], "header");
        assert!(
            blocks[0]["text"]["text"]
                .as_str()
                .unwrap()
                .contains("artifact out.md kept in")
        );
        let actions = blocks.last().unwrap();
        assert_eq!(actions["type"], "actions");
        assert_eq!(actions["elements"].as_array().unwrap().len(), 3);
        assert!(actions["elements"][0]["url"].is_string());
        assert!(slack["text"].as_str().unwrap().contains("[high]"));
    }

    #[test]
    fn approvals_and_tests_render_too() {
        let approval = Approval {
            id: Uuid::now_v7(),
            workspace_id: Uuid::now_v7(),
            run_id: Uuid::now_v7(),
            node_id: Uuid::now_v7(),
            graph_id: Uuid::now_v7(),
            agent_id: None,
            principal: "agent://acme/Scribe".into(),
            span_id: Uuid::now_v7(),
            tool: "make_docx".into(),
            args_sha256: None,
            rule: "approvals.tools".into(),
            reason: "exports".into(),
            approver_ids: vec![],
            status: ApprovalStatus::Denied,
            requested_at: Utc::now(),
            expires_at: Utc::now(),
            decided_by: Some(crate::domain::approval::Decider {
                id: None,
                name: "Ada".into(),
            }),
            decided_at: Some(Utc::now()),
            decision_reason: "not now".into(),
            decision_span_id: None,
            policy_versions: PolicyVersions::default(),
        };
        let body = envelope(
            Uuid::now_v7(),
            DeliveryEvent::ApprovalDecided,
            Utc::now(),
            &workspace(),
            Subject::Approval(&approval),
            "http://localhost:8080",
        );
        assert_eq!(body["approval"]["tool"], "make_docx");
        assert!(
            body["links"]["agent"].is_null(),
            "no agent id, no agent link"
        );
        let slack = slack_message(&body);
        assert_eq!(slack["attachments"][0]["color"], "#dc2626");
        let text = slack.to_string();
        assert!(
            text.contains("Denied: make_docx") && text.contains("Ada") && text.contains("not now")
        );
        let test = envelope(
            Uuid::now_v7(),
            DeliveryEvent::Test,
            Utc::now(),
            &workspace(),
            Subject::Test,
            "http://localhost:8080",
        );
        assert_eq!(test["event"], "test");
        assert!(test["message"].as_str().unwrap().contains("Acme"));
        let slack = slack_message(&test);
        assert_eq!(slack["attachments"][0]["blocks"][0]["type"], "header");
        assert_eq!(agent_name("system"), "system");
    }
}
