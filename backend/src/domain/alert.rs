//! Alerts: something about an agent that needs a look. Raised by the DLP
//! classifier, the behaviour baseline and the prompt-injection screen; they
//! land in the admins' inbox and on the graph's event stream.

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::policy_document::Severity;
use super::string_enum;

string_enum!(
    /// What raised the alert.
    AlertKind {
        /// A finding on the way out: a credential, personal data, a label leaving.
        Dlp => "dlp",
        /// The agent's behaviour left its baseline.
        Risk => "risk",
        /// Instructions were found inside data headed for the model.
        Injection => "injection",
        /// A policy rule asked for an alert.
        Policy => "policy",
    }
);

/// One alert.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Alert {
    pub id: Uuid,
    pub workspace_id: Uuid,
    #[schema(required = true)]
    pub agent_id: Option<Uuid>,
    pub principal: String,
    #[schema(required = true)]
    pub run_id: Option<Uuid>,
    #[schema(required = true)]
    pub node_id: Option<Uuid>,
    #[schema(required = true)]
    pub graph_id: Option<Uuid>,
    /// The policy span that recorded the finding, if any.
    #[schema(required = true)]
    pub span_id: Option<Uuid>,
    pub kind: AlertKind,
    pub severity: Severity,
    /// The rule that raised it.
    pub rule: String,
    pub title: String,
    pub detail: String,
    pub created_at: DateTime<Utc>,
    #[schema(required = true)]
    pub acknowledged_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub acknowledged_by: Option<Uuid>,
    /// Destinations the alert was delivered to (alert routing); 0 until the worker sent it.
    pub deliveries: i64,
}
