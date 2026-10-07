//! Anthropic Messages API (`POST /v1/messages`) with streaming, adaptive
//! thinking, structured outputs and server-side refusal fallbacks.

use std::time::Duration;

use async_stream::try_stream;
use futures::StreamExt;
use serde_json::{Value, json};

use super::retry::{full_jitter, parse_retry_after};
use super::sse::{SseDecoder, SseEvent};
use super::{LlmError, LlmEvent, LlmProvider, LlmRequest, LlmStream, StopReason, Usage};

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2023-06-01";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const MAX_HTTP_ATTEMPTS: u32 = 4;

/// Client for the Claude Messages API.
#[derive(Clone)]
pub struct AnthropicProvider {
    http: reqwest::Client,
    fallbacks: bool,
}

impl AnthropicProvider {
    /// `fallbacks` opts into server-side refusal fallbacks on the first-party API.
    pub fn new(http: reqwest::Client, fallbacks: bool) -> Self {
        AnthropicProvider { http, fallbacks }
    }

    fn base_url(req: &LlmRequest) -> &str {
        req.target
            .base_url
            .as_deref()
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/')
    }

    fn uses_fallbacks(&self, req: &LlmRequest) -> bool {
        self.fallbacks && Self::base_url(req) == DEFAULT_BASE_URL
    }

    /// The JSON request body.
    pub fn body(&self, req: &LlmRequest) -> Value {
        let mut body = json!({
            "model": req.target.model,
            "max_tokens": req.max_tokens,
            "messages": req.messages,
            "stream": true,
            "thinking": { "type": "adaptive" },
        });
        if !req.system.is_empty() {
            body["system"] = json!(req.system);
        }
        let mut output_config = serde_json::Map::new();
        if let Some(schema) = &req.json_schema {
            output_config.insert(
                "format".into(),
                json!({ "type": "json_schema", "schema": schema.schema }),
            );
        }
        if let Some(effort) = req.effort {
            output_config.insert("effort".into(), json!(effort));
        }
        if !output_config.is_empty() {
            body["output_config"] = Value::Object(output_config);
        }
        if self.uses_fallbacks(req) {
            body["fallbacks"] = json!("default");
        }
        body
    }

    async fn send(&self, req: &LlmRequest) -> Result<reqwest::Response, LlmError> {
        let key = req.target.api_key.as_ref().ok_or(LlmError::MissingKey)?;
        let url = format!("{}/v1/messages", Self::base_url(req));
        let body = self.body(req);
        let mut attempt = 1;
        loop {
            let mut builder = self
                .http
                .post(&url)
                .header("x-api-key", key.expose())
                .header("anthropic-version", API_VERSION)
                .header("content-type", "application/json")
                .json(&body);
            if self.uses_fallbacks(req) {
                builder = builder.header("anthropic-beta", FALLBACK_BETA);
            }
            let (err, retry_after) = match builder.send().await {
                Ok(resp) if resp.status().is_success() => return Ok(resp),
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let retry_after = parse_retry_after(
                        resp.headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok()),
                    );
                    let message = error_message(&resp.text().await.unwrap_or_default());
                    (LlmError::from_status(status, message), retry_after)
                }
                Err(e) => (LlmError::from(e), None),
            };
            if !err.is_retryable() || attempt >= MAX_HTTP_ATTEMPTS {
                return Err(err);
            }
            let delay = retry_after.unwrap_or_else(|| {
                full_jitter(attempt, Duration::from_secs(1), Duration::from_secs(30))
            });
            tracing::warn!(attempt, ?delay, error = %err, "retrying Anthropic request");
            tokio::time::sleep(delay).await;
            attempt += 1;
        }
    }
}

impl LlmProvider for AnthropicProvider {
    fn stream(&self, req: LlmRequest) -> LlmStream {
        let this = self.clone();
        Box::pin(try_stream! {
            let resp = this.send(&req).await?;
            let mut bytes = resp.bytes_stream();
            let mut decoder = SseDecoder::default();
            let mut state = StreamState::default();
            while let Some(chunk) = bytes.next().await {
                let chunk = chunk.map_err(LlmError::from)?;
                for event in decoder.push(&chunk).map_err(LlmError::Protocol)? {
                    for out in state.handle(&event)? {
                        let done = matches!(out, LlmEvent::Done(_));
                        yield out;
                        if done {
                            return;
                        }
                    }
                }
            }
            Err(LlmError::Protocol("stream ended before message_stop".into()))?;
        })
    }
}

/// Extracts `error.message` from an error body (bounded length).
fn error_message(body: &str) -> String {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "request failed".into());
    message.chars().take(300).collect()
}

/// Translates Anthropic stream events into [`LlmEvent`]s.
#[derive(Debug, Default)]
struct StreamState {
    usage: Usage,
    stop_reason: Option<String>,
}

impl StreamState {
    fn handle(&mut self, event: &SseEvent) -> Result<Vec<LlmEvent>, LlmError> {
        let kind = event.event.as_deref().unwrap_or_default();
        if kind == "ping" || event.data.is_empty() {
            return Ok(Vec::new());
        }
        let data: Value = serde_json::from_str(&event.data)
            .map_err(|e| LlmError::Protocol(format!("invalid event JSON: {e}")))?;
        match kind {
            "message_start" => {
                let usage = &data["message"]["usage"];
                let n = |k: &str| usage[k].as_u64().unwrap_or(0);
                self.usage.input_tokens = n("input_tokens")
                    + n("cache_creation_input_tokens")
                    + n("cache_read_input_tokens");
                self.usage.cached_tokens = n("cache_read_input_tokens");
                self.usage.output_tokens = usage["output_tokens"].as_u64().unwrap_or(0);
                Ok(vec![LlmEvent::Usage(self.usage)])
            }
            "content_block_delta" => match data["delta"]["type"].as_str() {
                Some("text_delta") => {
                    let text = data["delta"]["text"].as_str().unwrap_or_default();
                    Ok(if text.is_empty() {
                        vec![]
                    } else {
                        vec![LlmEvent::Text(text.to_owned())]
                    })
                }
                // Thinking (display omitted by default), signatures and other deltas are not shown.
                _ => Ok(Vec::new()),
            },
            "message_delta" => {
                if let Some(reason) = data["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(reason.to_owned());
                }
                if let Some(out) = data["usage"]["output_tokens"].as_u64() {
                    self.usage.output_tokens = out;
                }
                Ok(vec![LlmEvent::Usage(self.usage)])
            }
            "message_stop" => match self.stop_reason.as_deref() {
                Some("refusal") => Err(LlmError::Refused),
                Some("max_tokens") => Ok(vec![LlmEvent::Done(StopReason::MaxTokens)]),
                _ => Ok(vec![LlmEvent::Done(StopReason::EndTurn)]),
            },
            "error" => {
                let kind = data["error"]["type"].as_str().unwrap_or_default();
                let message = data["error"]["message"]
                    .as_str()
                    .unwrap_or("stream error")
                    .to_owned();
                let status = match kind {
                    "overloaded_error" => 529,
                    "rate_limit_error" => 429,
                    "api_error" => 500,
                    _ => 400,
                };
                Err(LlmError::from_status(status, message))
            }
            _ => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Secret;
    use crate::domain::settings::LlmProviderKind;
    use crate::llm::{JsonSchema, LlmTarget, Message};

    fn request(base_url: Option<&str>) -> LlmRequest {
        LlmRequest {
            target: LlmTarget {
                provider: LlmProviderKind::Anthropic,
                model: "claude-opus-5".into(),
                base_url: base_url.map(str::to_owned),
                api_key: Some(Secret::new("k".into())),
            },
            system: "sys".into(),
            messages: vec![Message::user("hi")],
            max_tokens: 64_000,
            json_schema: Some(JsonSchema {
                name: "x",
                schema: json!({"type": "object"}),
            }),
            effort: Some("low"),
            cacheable: false,
        }
    }

    #[test]
    fn body_has_required_shape() {
        let p = AnthropicProvider::new(reqwest::Client::new(), true);
        let body = p.body(&request(None));
        assert_eq!(body["thinking"], json!({"type": "adaptive"}));
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
        assert_eq!(body["output_config"]["effort"], "low");
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(body["stream"], true);
        for forbidden in ["temperature", "top_p", "budget_tokens"] {
            assert!(body.get(forbidden).is_none());
        }
        let proxied = p.body(&request(Some("https://proxy.internal")));
        assert!(
            proxied.get("fallbacks").is_none(),
            "fallbacks only on the first-party API"
        );
    }

    fn ev(kind: &str, data: Value) -> SseEvent {
        SseEvent {
            event: Some(kind.into()),
            data: data.to_string(),
        }
    }

    #[test]
    fn parses_a_streamed_message() {
        let mut s = StreamState::default();
        let mut out = Vec::new();
        for e in [
            ev(
                "message_start",
                json!({"type":"message_start","message":{"usage":{"input_tokens":12,"output_tokens":1}}}),
            ),
            ev(
                "content_block_start",
                json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking"}}),
            ),
            ev(
                "content_block_delta",
                json!({"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":""}}),
            ),
            ev(
                "content_block_delta",
                json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"Hi"}}),
            ),
            ev("ping", json!({})),
            ev(
                "message_delta",
                json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":7}}),
            ),
            ev("message_stop", json!({"type":"message_stop"})),
        ] {
            out.extend(s.handle(&e).unwrap());
        }
        assert_eq!(
            out,
            vec![
                LlmEvent::Usage(Usage {
                    input_tokens: 12,
                    output_tokens: 1,
                    cached_tokens: 0
                }),
                LlmEvent::Text("Hi".into()),
                LlmEvent::Usage(Usage {
                    input_tokens: 12,
                    output_tokens: 7,
                    cached_tokens: 0
                }),
                LlmEvent::Done(StopReason::EndTurn),
            ]
        );
    }

    #[test]
    fn refusal_and_truncation_and_errors() {
        let mut s = StreamState::default();
        s.handle(&ev(
            "message_delta",
            json!({"delta":{"stop_reason":"refusal"},"usage":{"output_tokens":0}}),
        ))
        .unwrap();
        assert!(matches!(
            s.handle(&ev("message_stop", json!({}))),
            Err(LlmError::Refused)
        ));

        let mut s = StreamState::default();
        s.handle(&ev(
            "message_delta",
            json!({"delta":{"stop_reason":"max_tokens"}}),
        ))
        .unwrap();
        assert_eq!(
            s.handle(&ev("message_stop", json!({}))).unwrap(),
            vec![LlmEvent::Done(StopReason::MaxTokens)]
        );

        let err = StreamState::default()
            .handle(&ev(
                "error",
                json!({"error":{"type":"overloaded_error","message":"Overloaded"}}),
            ))
            .unwrap_err();
        assert!(err.is_retryable());
        assert_eq!(
            error_message(r#"{"error":{"message":"bad key"}}"#),
            "bad key"
        );
    }

    #[tokio::test]
    async fn missing_key_fails_fast() {
        let p = AnthropicProvider::new(reqwest::Client::new(), true);
        let mut req = request(None);
        req.target.api_key = None;
        let err = crate::llm::collect(p.stream(req), |_| {})
            .await
            .unwrap_err();
        assert!(matches!(err, LlmError::MissingKey));
    }
}
