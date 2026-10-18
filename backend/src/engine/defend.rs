//! The Defend function at work: alerts, and the DLP screen on the way out.
//! An alert is a row, an inbox entry for every admin, an `alert` event on the
//! graph's stream and an audit entry; a DLP finding is a `policy` span, and a
//! denial keeps the artifact in (stored, not downloadable) or refuses the
//! external call.

use serde_json::json;
use uuid::Uuid;

use super::executor::ExecContext;
use crate::app::AppState;
use crate::domain::alert::{Alert, AlertKind};
use crate::domain::audit::AuditAction;
use crate::domain::dlp::{self, Destination, Findings};
use crate::domain::policy::{Action, Decision, Effect};
use crate::domain::policy_document::Severity;
use crate::domain::run::Artifact;
use crate::realtime::events::{LogLevel, SseEvent};
use crate::repo;
use crate::repo::alerts::NewAlert;
use crate::repo::audit::Subject;

/// Who an alert is about and where it was raised.
pub struct About<'a> {
    pub workspace_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub principal: &'a str,
    pub run_id: Option<Uuid>,
    pub node_id: Option<Uuid>,
    pub graph_id: Option<Uuid>,
    /// Who the audit entry is written by: whoever started the run.
    pub actor: Uuid,
}

/// What an alert says.
pub struct Finding<'a> {
    /// The policy span that recorded it, if any.
    pub span_id: Option<Uuid>,
    pub kind: AlertKind,
    pub severity: Severity,
    pub rule: &'a str,
    pub title: &'a str,
    pub detail: &'a str,
}

/// Raises an alert: recorded, put in every admin's inbox, published on the
/// graph's stream and audited. Never fails the work that raised it.
pub async fn raise(state: &AppState, about: &About<'_>, finding: &Finding<'_>) -> Option<Alert> {
    let (kind, severity, title) = (finding.kind, finding.severity, finding.title);
    let new = NewAlert {
        workspace_id: about.workspace_id,
        agent_id: about.agent_id,
        principal: about.principal,
        run_id: about.run_id,
        node_id: about.node_id,
        graph_id: about.graph_id,
        span_id: finding.span_id,
        kind,
        severity,
        rule: finding.rule,
        title,
        detail: finding.detail,
    };
    let alert = match repo::alerts::insert(&state.db, &new).await {
        Ok(alert) => alert,
        Err(err) => {
            tracing::error!(error = %err, rule = finding.rule, "alert not recorded");
            return None;
        }
    };
    state.metrics.alerts.add(&[("kind", kind.as_str())], 1);
    match repo::workspaces::admins(&state.db, about.workspace_id).await {
        Ok(admins) => {
            if let Err(err) =
                repo::alerts::notify(&state.db, alert.id, about.workspace_id, &admins).await
            {
                tracing::error!(alert = %alert.id, error = %err, "admins were not told of the alert");
            }
        }
        Err(err) => {
            tracing::error!(alert = %alert.id, error = %err, "admins not found for the alert")
        }
    }
    if let Some(graph_id) = about.graph_id {
        state.hub.publish(
            graph_id,
            SseEvent::Alert {
                alert: alert.clone(),
            },
        );
    }
    if let Err(err) = repo::audit::record(
        &state.db,
        about.workspace_id,
        about.actor,
        AuditAction::AlertRaised,
        Subject::Text(about.principal),
        &format!("{severity} {kind}: {title}"),
    )
    .await
    {
        tracing::error!(alert = %alert.id, error = %err, "alert not audited");
    }
    Some(alert)
}

impl ExecContext {
    /// This node's agent, as the subject of an alert.
    fn about(&self) -> Option<(About<'_>, String)> {
        let workspace_id = self.workspace_id?;
        let principal = self.principal();
        Some((
            About {
                workspace_id,
                agent_id: self.agent.as_ref().map(|a| a.id),
                principal: "",
                run_id: Some(self.run_id),
                node_id: Some(self.node.id),
                graph_id: Some(self.graph_id),
                actor: self.owner,
            },
            principal,
        ))
    }

    /// Raises an alert about this node's agent.
    pub async fn alert(&self, finding: &Finding<'_>) -> Option<Alert> {
        let (mut about, principal) = self.about()?;
        about.principal = &principal;
        raise(&self.state, &about, finding).await
    }

    /// Screens an artifact just stored: classifies its text, decides under the
    /// DLP rules for the strictest label the node read, and on a denial keeps
    /// the file in. Every finding is a `policy` span; a denial or an alert
    /// also raises an alert.
    pub async fn screen_artifact(&self, artifact: &Artifact, bytes: &[u8]) -> Decision {
        let label = self.strictest_label();
        let rules = &self.rules.dlp;
        let findings = dlp::extract_text(bytes, &artifact.mime, &artifact.path)
            .map(|text| dlp::classify(&text, &rules.pii.kinds, &rules.pii.keywords))
            .unwrap_or_default();
        let decision = dlp::decide(rules, label, &findings, Destination::Artifact);
        if decision.effect == Effect::Allow {
            return decision;
        }
        let span_id = self.policy_span(
            &artifact.path,
            &decision,
            json!({
                "action": "artifact",
                "artifact_id": artifact.id,
                "label": label,
                "secrets": findings.secrets,
                "pii": findings.pii_total(),
                "keywords": findings.keywords,
                "findings": findings.describe(),
            }),
        );
        let (severity, title) = match decision.effect {
            Effect::Deny => (
                Severity::High,
                format!("artifact {} kept in", artifact.path),
            ),
            Effect::Alert { severity } => (
                severity,
                format!("artifact {} carries personal data", artifact.path),
            ),
            _ => (Severity::Low, String::new()),
        };
        if !decision.allowed {
            let reason = format!("{}: {}", decision.rule, decision.reason);
            if let Err(err) =
                repo::artifacts::quarantine(&self.state.db, artifact.id, &reason).await
            {
                tracing::error!(artifact = %artifact.id, error = %err, "artifact not quarantined");
            }
            self.state.metrics.artifacts_quarantined.add(1);
            self.log(
                LogLevel::Warn,
                format!(
                    "artifact `{}` kept in until an admin releases it ({reason})",
                    artifact.path
                ),
            );
            if let Some(workspace) = self.workspace_id
                && let Err(err) = repo::audit::record(
                    &self.state.db,
                    workspace,
                    self.owner,
                    AuditAction::ArtifactQuarantined,
                    Subject::Text(&self.principal()),
                    &format!("{} in run {}: {reason}", artifact.path, self.run_id),
                )
                .await
            {
                tracing::error!(artifact = %artifact.id, error = %err, "quarantine not audited");
            }
        } else {
            self.log(
                LogLevel::Warn,
                format!("artifact `{}`: {}", artifact.path, decision.reason),
            );
        }
        self.alert(&Finding {
            span_id: Some(span_id),
            kind: AlertKind::Dlp,
            severity,
            rule: decision.rule,
            title: &title,
            detail: &decision.reason,
        })
        .await;
        decision
    }

    /// Screens an external call the runtime reported (`egress_host` on a
    /// tool call): the host against the egress rules and the labels the node
    /// read against where such content may go. A denial is a `policy` span
    /// and an alert; the call itself already happened, so it is also a
    /// mismatch the re-check reports.
    pub async fn screen_egress(&self, host: &str, span_id: Uuid) -> Decision {
        let hosts = self.decide(&Action::Egress(host));
        let decision = if hosts.allowed {
            dlp::decide(
                &self.rules.dlp,
                self.strictest_label(),
                &Findings::default(),
                Destination::Egress(host),
            )
        } else {
            hosts
        };
        if decision.allowed {
            return decision;
        }
        let recorded = self.policy_span(
            host,
            &decision,
            json!({
                "action": "egress",
                "span_id": span_id,
                "label": self.strictest_label(),
            }),
        );
        self.alert(&Finding {
            span_id: Some(recorded),
            kind: AlertKind::Dlp,
            severity: Severity::High,
            rule: decision.rule,
            title: &format!("data sent to {host}"),
            detail: &decision.reason,
        })
        .await;
        decision
    }
}
