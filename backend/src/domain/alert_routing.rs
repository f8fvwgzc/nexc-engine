//! Alert routing: where a workspace's alerts and approvals go (a signed JSON
//! webhook, a Slack incoming webhook), what each destination wants, and what
//! became of every delivery. The worker in `alerting` sends; this module
//! holds the shapes and the rules.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::alert::AlertKind;
use super::policy_document::Severity;
use super::string_enum;
use super::validation::{FieldErrors, check_text};

/// Longest destination name.
pub const NAME_MAX: usize = 100;
/// Longest URL.
pub const URL_MAX: usize = 2_000;
/// Most destinations one workspace registers.
pub const DESTINATIONS_MAX: i64 = 32;
/// Deliveries that fail in a row before a destination is switched off.
pub const FAILURES_MAX: i32 = 50;
/// Prefix of a destination's signing secret.
pub const SECRET_PREFIX: &str = "nexc_whs_";
/// Most delivery log entries listed at once.
pub const LOG_MAX: i64 = 200;
/// How long a delivery is retried before it is given up.
pub const RETRY_WINDOW: Duration = Duration::from_secs(24 * 3600);
/// The first wait after a failed attempt; doubled every time, up to [`BACKOFF_MAX`].
pub const BACKOFF_FIRST: Duration = Duration::from_secs(30);
/// The longest wait between two attempts.
pub const BACKOFF_MAX: Duration = Duration::from_secs(3600);

string_enum!(
    /// What a destination speaks.
    DestinationKind {
        /// A JSON POST signed with the destination's secret.
        Webhook => "webhook",
        /// A Slack incoming webhook: a Block Kit message.
        Slack => "slack",
    }
);

string_enum!(
    /// Where a delivery stands.
    DeliveryStatus {
        Pending => "pending",
        Delivered => "delivered",
        /// Given up: refused for good, or retried for a day.
        Failed => "failed",
    }
);

/// What a delivery carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum DeliveryEvent {
    #[serde(rename = "alert.raised")]
    AlertRaised,
    #[serde(rename = "approval.requested")]
    ApprovalRequested,
    /// Approved, denied, or expired with nobody deciding.
    #[serde(rename = "approval.decided")]
    ApprovalDecided,
    /// `POST /workspaces/{wid}/alert-destinations/{id}/test`.
    #[serde(rename = "test")]
    Test,
}

impl DeliveryEvent {
    /// The wire / database spelling of this value.
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryEvent::AlertRaised => "alert.raised",
            DeliveryEvent::ApprovalRequested => "approval.requested",
            DeliveryEvent::ApprovalDecided => "approval.decided",
            DeliveryEvent::Test => "test",
        }
    }
}

impl std::str::FromStr for DeliveryEvent {
    type Err = super::UnknownVariant;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "alert.raised" => Ok(DeliveryEvent::AlertRaised),
            "approval.requested" => Ok(DeliveryEvent::ApprovalRequested),
            "approval.decided" => Ok(DeliveryEvent::ApprovalDecided),
            "test" => Ok(DeliveryEvent::Test),
            other => Err(super::UnknownVariant(other.to_owned())),
        }
    }
}

impl std::fmt::Display for DeliveryEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A destination, as shown: the URL's host only, the secret never.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AlertDestination {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub kind: DestinationKind,
    /// The host of the URL; the URL itself is kept encrypted (a Slack URL carries its token).
    pub host: String,
    /// Alert kinds sent; empty means every kind.
    pub kinds: Vec<AlertKind>,
    /// The least severity sent.
    pub min_severity: Severity,
    /// Whether approvals requested and decided are sent too.
    pub include_approvals: bool,
    pub enabled: bool,
    /// Why the platform switched it off, when it did.
    pub disabled_reason: String,
    /// Deliveries that failed in a row; at 50 the destination is switched off.
    pub failures: i32,
    #[schema(required = true)]
    pub last_delivered_at: Option<DateTime<Utc>>,
    /// What the last failed delivery said.
    pub last_error: String,
    #[schema(required = true)]
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AlertDestination {
    /// Whether an alert of `kind` and `severity` goes to this destination.
    pub fn wants_alert(&self, kind: AlertKind, severity: Severity) -> bool {
        self.enabled
            && (self.kinds.is_empty() || self.kinds.contains(&kind))
            && severity >= self.min_severity
    }

    /// Whether approvals go to this destination.
    pub fn wants_approvals(&self) -> bool {
        self.enabled && self.include_approvals
    }
}

/// A destination with its secret: the answer to its creation, and nowhere else.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IssuedAlertDestination {
    pub destination: AlertDestination,
    /// `nexc_whs_…`: the key of `X-Nexc-Signature` on every delivery. Shown once.
    pub secret: String,
}

/// The fields a destination is registered with.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AlertDestinationInput {
    pub name: String,
    pub kind: DestinationKind,
    /// An http(s) URL; private and loopback hosts are refused unless the
    /// server allows them (`NEXC_ALERT_ALLOW_PRIVATE_HOSTS`).
    pub url: String,
    /// Alert kinds sent; empty (the default) sends every kind.
    #[serde(default)]
    pub kinds: Vec<AlertKind>,
    /// The least severity sent; `low` by default.
    #[serde(default)]
    pub min_severity: Option<Severity>,
    #[serde(default)]
    pub include_approvals: bool,
    /// On by default.
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// What `PATCH` changes; a field left out stays as it is.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AlertDestinationPatch {
    #[serde(default)]
    pub name: Option<String>,
    /// A new URL (the secret stays).
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub kinds: Option<Vec<AlertKind>>,
    #[serde(default)]
    pub min_severity: Option<Severity>,
    #[serde(default)]
    pub include_approvals: Option<bool>,
    /// Enabling clears a reason the platform switched it off with and its failure count.
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// Whether `url` is an http(s) URL without credentials; the host, lower-cased, when it is.
pub fn check_url(url: &str) -> Result<String, String> {
    if url.chars().count() > URL_MAX {
        return Err(format!("must be at most {URL_MAX} characters"));
    }
    let parsed =
        reqwest::Url::parse(url.trim()).map_err(|_| "must be an http(s) URL".to_owned())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("must be an http(s) URL".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("must not carry credentials".into());
    }
    parsed
        .host_str()
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| "must name a host".to_owned())
}

fn check_name(errors: &mut FieldErrors, name: &str) {
    check_text(errors, "name", name, NAME_MAX);
    if name.chars().any(char::is_control) {
        errors.add("name", "must be one line of text");
    }
}

impl AlertDestinationInput {
    /// Adds every problem with the input to `errors` (whether the host may be
    /// reached is the server's call, made with its settings).
    pub fn check(&self, errors: &mut FieldErrors) {
        check_name(errors, &self.name);
        if let Err(problem) = check_url(&self.url) {
            errors.add("url", problem);
        }
    }
}

impl AlertDestinationPatch {
    /// Adds every problem with the patch to `errors`.
    pub fn check(&self, errors: &mut FieldErrors) {
        if let Some(name) = &self.name {
            check_name(errors, name);
        }
        if let Some(url) = &self.url
            && let Err(problem) = check_url(url)
        {
            errors.add("url", problem);
        }
    }

    /// Whether the patch changes anything at all.
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.url.is_none()
            && self.kinds.is_none()
            && self.min_severity.is_none()
            && self.include_approvals.is_none()
            && self.enabled.is_none()
    }
}

/// One delivery to a destination: an entry of its log.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AlertDelivery {
    pub id: Uuid,
    pub destination_id: Uuid,
    pub event: DeliveryEvent,
    #[schema(required = true)]
    pub alert_id: Option<Uuid>,
    #[schema(required = true)]
    pub approval_id: Option<Uuid>,
    pub status: DeliveryStatus,
    pub attempts: i32,
    /// When the next attempt is due, while pending.
    #[schema(required = true)]
    pub next_attempt_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub last_attempt_at: Option<DateTime<Utc>>,
    /// The HTTP status of the last attempt, when one came back.
    #[schema(required = true)]
    pub last_status: Option<i32>,
    pub last_error: String,
    pub created_at: DateTime<Utc>,
    #[schema(required = true)]
    pub delivered_at: Option<DateTime<Utc>>,
}

/// What `POST …/test` found.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DestinationTest {
    pub ok: bool,
    /// The HTTP status the destination answered, when it answered.
    #[schema(required = true)]
    pub status: Option<u16>,
    pub detail: String,
    pub latency_ms: u64,
}

/// How long to wait before the next attempt after `attempts` failed:
/// 30 s, 1 min, 2 min, … capped at an hour.
pub fn backoff(attempts: i32) -> Duration {
    let doublings = u32::try_from(attempts.saturating_sub(1))
        .unwrap_or(0)
        .min(16);
    BACKOFF_FIRST
        .checked_mul(1u32 << doublings)
        .unwrap_or(BACKOFF_MAX)
        .min(BACKOFF_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn destination() -> AlertDestination {
        AlertDestination {
            id: Uuid::now_v7(),
            workspace_id: Uuid::now_v7(),
            name: "SIEM".into(),
            kind: DestinationKind::Webhook,
            host: "siem.example.com".into(),
            kinds: vec![AlertKind::Dlp],
            min_severity: Severity::Medium,
            include_approvals: false,
            enabled: true,
            disabled_reason: String::new(),
            failures: 0,
            last_delivered_at: None,
            last_error: String::new(),
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn a_destination_filters_by_kind_severity_and_state() {
        let mut d = destination();
        assert!(d.wants_alert(AlertKind::Dlp, Severity::High));
        assert!(d.wants_alert(AlertKind::Dlp, Severity::Medium));
        assert!(
            !d.wants_alert(AlertKind::Dlp, Severity::Low),
            "below the minimum"
        );
        assert!(
            !d.wants_alert(AlertKind::Risk, Severity::Critical),
            "not its kind"
        );
        assert!(!d.wants_approvals());
        d.kinds.clear();
        d.include_approvals = true;
        assert!(
            d.wants_alert(AlertKind::Risk, Severity::Critical),
            "every kind"
        );
        assert!(d.wants_approvals());
        d.enabled = false;
        assert!(!d.wants_alert(AlertKind::Dlp, Severity::Critical) && !d.wants_approvals());
    }

    #[test]
    fn urls_and_names_are_checked() {
        assert_eq!(
            check_url("https://Hooks.Slack.com/services/T/B/x").unwrap(),
            "hooks.slack.com"
        );
        assert!(check_url("ftp://x.example").is_err());
        assert!(check_url("https://user:pw@x.example/").is_err());
        assert!(check_url("not a url").is_err());
        assert!(check_url(&format!("https://x.example/{}", "a".repeat(URL_MAX))).is_err());
        let bad = AlertDestinationInput {
            name: "a\nb".into(),
            kind: DestinationKind::Slack,
            url: "nope".into(),
            kinds: vec![],
            min_severity: None,
            include_approvals: false,
            enabled: None,
        };
        let mut errors = FieldErrors::default();
        bad.check(&mut errors);
        let fields: Vec<&String> = errors.as_map().keys().collect();
        assert_eq!(fields, ["name", "url"]);
        let patch = AlertDestinationPatch::default();
        assert!(patch.is_empty());
        let mut errors = FieldErrors::default();
        AlertDestinationPatch {
            url: Some("x".into()),
            ..AlertDestinationPatch::default()
        }
        .check(&mut errors);
        assert!(!errors.is_empty());
    }

    #[test]
    fn backoff_doubles_up_to_an_hour() {
        assert_eq!(backoff(0), Duration::from_secs(30));
        assert_eq!(backoff(1), Duration::from_secs(30));
        assert_eq!(backoff(2), Duration::from_secs(60));
        assert_eq!(backoff(5), Duration::from_secs(480));
        assert_eq!(backoff(8), BACKOFF_MAX);
        assert_eq!(backoff(100), BACKOFF_MAX);
        assert_eq!(
            serde_json::to_value(DeliveryEvent::ApprovalDecided).unwrap(),
            "approval.decided"
        );
        assert_eq!(
            "alert.raised".parse::<DeliveryEvent>().unwrap(),
            DeliveryEvent::AlertRaised
        );
        assert!("alert_raised".parse::<DeliveryEvent>().is_err());
    }
}
