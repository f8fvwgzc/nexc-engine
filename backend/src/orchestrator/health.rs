//! Health of external backends, cached for 10 seconds.

use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::app::AppState;
use crate::domain::settings::LlmProviderKind;
use crate::domain::status::BackendHealth;
use crate::engine::credentials::Resolved;
use crate::llm::openai_compat;

const TTL: Duration = Duration::from_secs(10);
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Default)]
struct Slot(Mutex<Option<(Instant, BackendHealth)>>);

impl Slot {
    async fn get_or(&self, probe: impl Future<Output = BackendHealth>) -> BackendHealth {
        let mut slot = self.0.lock().await;
        if let Some((at, health)) = slot.as_ref()
            && at.elapsed() < TTL
        {
            return health.clone();
        }
        let health = probe.await;
        *slot = Some((Instant::now(), health.clone()));
        health
    }
}

/// Cached probes of the agent runtime (both wires), texc-symphony and onto-intelli.
#[derive(Debug, Default)]
pub struct HealthCache {
    runtime: Slot,
    runtime_grpc: Slot,
    symphony: Slot,
    ontology: Slot,
}

impl HealthCache {
    /// Agent runtime health (`GET /healthz` with the runtime token).
    pub async fn agent_runtime(&self, state: &AppState) -> BackendHealth {
        let url = state.settings.runtime_url.trim_end_matches('/').to_owned();
        self.runtime
            .get_or(async {
                let req = state
                    .http
                    .get(format!("{url}/healthz"))
                    .bearer_auth(state.settings.runtime_token.expose())
                    .timeout(PROBE_TIMEOUT);
                probe(req, true, url.clone()).await
            })
            .await
    }

    /// The runtime's gRPC endpoint (`AgentRuntime/Health`), or disabled when
    /// the backend speaks NDJSON.
    pub async fn runtime_grpc(&self, state: &AppState) -> BackendHealth {
        let Some(channel) = state.runtime_grpc.clone() else {
            return BackendHealth {
                enabled: false,
                ok: false,
                url: None,
                detail: Some("disabled: NEXC_RUNTIME_PROTOCOL=ndjson".into()),
            };
        };
        let url = state.settings.runtime_grpc_url.clone();
        self.runtime_grpc
            .get_or(async {
                let (ok, detail) = tokio::time::timeout(PROBE_TIMEOUT, async {
                    let mut client =
                        crate::rpc::runtime::agent_runtime_client::AgentRuntimeClient::new(channel);
                    let mut request = tonic::Request::new(crate::rpc::runtime::HealthRequest {});
                    if let Ok(bearer) =
                        format!("Bearer {}", state.settings.runtime_token.expose()).parse()
                    {
                        request.metadata_mut().insert("authorization", bearer);
                    }
                    client.health(request).await
                })
                .await
                .map_or_else(
                    |_| (false, "no answer within 3 seconds".to_owned()),
                    |reply| match reply {
                        Ok(reply) => {
                            let reply = reply.into_inner();
                            (
                                reply.status == "ok",
                                format!(
                                    "runtime {} ({} agent(s) live, {} run(s) active)",
                                    reply.version, reply.agents_loaded, reply.runs_active
                                ),
                            )
                        }
                        Err(status) => (false, format!("unreachable: {}", status.message())),
                    },
                );
                BackendHealth {
                    enabled: true,
                    ok,
                    url: Some(url.clone()),
                    detail: Some(detail),
                }
            })
            .await
    }

    /// texc-symphony health (`GET /api/v1/health`), or disabled.
    pub async fn symphony(&self, state: &AppState) -> BackendHealth {
        let bridge = &state.symphony;
        if !bridge.enabled() {
            return BackendHealth {
                enabled: false,
                ok: false,
                url: None,
                detail: Some("disabled".into()),
            };
        }
        let url = bridge.url().to_owned();
        self.symphony
            .get_or(probe(
                state
                    .http
                    .get(format!("{url}/api/v1/health"))
                    .timeout(PROBE_TIMEOUT),
                true,
                url.clone(),
            ))
            .await
    }
}

impl HealthCache {
    /// onto-intelli health (`AdminService/Health`), or disabled.
    pub async fn ontology(&self, state: &AppState) -> BackendHealth {
        let Some(backend) = &state.ontology else {
            return BackendHealth {
                enabled: false,
                ok: false,
                url: None,
                detail: Some("disabled".into()),
            };
        };
        let url = backend.location();
        self.ontology
            .get_or(async {
                let probe = tokio::time::timeout(PROBE_TIMEOUT, backend.health());
                let (ok, detail) = match probe.await {
                    Ok(Ok(health)) => (
                        health.status == "ok",
                        format!(
                            "onto-intelli {}, explains with {} ({})",
                            health.version,
                            health.llm,
                            if health.llm_available {
                                "available"
                            } else {
                                "unavailable"
                            }
                        ),
                    ),
                    Ok(Err(err)) => (false, err.to_string()),
                    Err(_) => (false, "no answer within 3 seconds".into()),
                };
                BackendHealth {
                    enabled: true,
                    ok,
                    url: Some(url.clone()),
                    detail: Some(detail),
                }
            })
            .await
    }
}

/// Runs a GET probe and reports the outcome.
pub async fn probe(req: reqwest::RequestBuilder, enabled: bool, url: String) -> BackendHealth {
    let (ok, detail) = match req.send().await {
        Ok(resp) if resp.status().is_success() => (true, None),
        Ok(resp) => (false, Some(format!("HTTP {}", resp.status()))),
        Err(err) => (false, Some(format!("unreachable: {}", err.without_url()))),
    };
    BackendHealth {
        enabled,
        ok,
        url: Some(url),
        detail,
    }
}

/// LLM "health": whether requests can be sent with the effective settings.
pub fn llm(resolved: &Resolved) -> BackendHealth {
    let t = &resolved.target;
    let url = match t.provider {
        LlmProviderKind::Anthropic => Some(
            t.base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com".into()),
        ),
        LlmProviderKind::OpenaiCompatible => Some(
            t.base_url
                .clone()
                .unwrap_or_else(|| openai_compat::DEFAULT_BASE_URL.into()),
        ),
        LlmProviderKind::Demo | LlmProviderKind::ClaudeCode => None,
    };
    let detail = if resolved.is_demo() {
        "demo mode: deterministic offline provider".to_owned()
    } else if resolved.is_usable() {
        format!("{} / {} (key: {})", t.provider, t.model, resolved.source)
    } else {
        "no API key configured".to_owned()
    };
    BackendHealth {
        enabled: true,
        ok: resolved.is_usable(),
        url,
        detail: Some(detail),
    }
}
