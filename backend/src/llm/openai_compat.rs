//! OpenAI-compatible `/chat/completions` streaming (Ollama, vLLM, LM Studio,
//! llama.cpp server, …). The API key is optional for local servers.

use async_stream::try_stream;
use futures::StreamExt;
use serde_json::{Value, json};

use super::sse::SseDecoder;
use super::{LlmError, LlmEvent, LlmProvider, LlmRequest, LlmStream, StopReason, Usage};

/// Ollama's OpenAI-compatible endpoint.
pub const DEFAULT_BASE_URL: &str = "http://localhost:11434/v1";

/// Client for OpenAI-compatible servers.
#[derive(Clone)]
pub struct OpenAiCompatProvider {
    http: reqwest::Client,
}

impl OpenAiCompatProvider {
    /// Provider using the shared HTTP client.
    pub fn new(http: reqwest::Client) -> Self {
        OpenAiCompatProvider { http }
    }

    fn body(req: &LlmRequest) -> Value {
        let mut messages = Vec::with_capacity(req.messages.len() + 1);
        if !req.system.is_empty() {
            messages.push(json!({ "role": "system", "content": req.system }));
        }
        messages.extend(req.messages.iter().map(|m| json!(m)));
        let mut body = json!({
            "model": req.target.model,
            "messages": messages,
            "max_tokens": req.max_tokens,
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        if let Some(schema) = &req.json_schema {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": { "name": schema.name, "schema": schema.schema, "strict": true },
            });
        }
        body
    }
}

impl LlmProvider for OpenAiCompatProvider {
    fn stream(&self, req: LlmRequest) -> LlmStream {
        let http = self.http.clone();
        Box::pin(try_stream! {
            let base = req.target.base_url.as_deref().unwrap_or(DEFAULT_BASE_URL).trim_end_matches('/');
            let mut builder = http.post(format!("{base}/chat/completions")).json(&Self::body(&req));
            if let Some(key) = &req.target.api_key {
                builder = builder.bearer_auth(key.expose());
            }
            let resp = checked(builder.send().await.map_err(LlmError::from)?).await?;
            let mut bytes = resp.bytes_stream();
            let mut decoder = SseDecoder::default();
            let mut usage = Usage::default();
            let mut stop = StopReason::EndTurn;
            while let Some(chunk) = bytes.next().await {
                let chunk = chunk.map_err(LlmError::from)?;
                for event in decoder.push(&chunk).map_err(LlmError::Protocol)? {
                    if event.data.trim() == "[DONE]" {
                        yield LlmEvent::Usage(usage);
                        yield LlmEvent::Done(stop);
                        return;
                    }
                    let data: Value = serde_json::from_str(&event.data)
                        .map_err(|e| LlmError::Protocol(format!("invalid chunk JSON: {e}")))?;
                    if let Some(u) = data.get("usage").filter(|u| u.is_object()) {
                        usage.input_tokens = u["prompt_tokens"].as_u64().unwrap_or(usage.input_tokens);
                        usage.output_tokens = u["completion_tokens"].as_u64().unwrap_or(usage.output_tokens);
                    }
                    let choice = &data["choices"][0];
                    if let Some(text) = choice["delta"]["content"].as_str().filter(|t| !t.is_empty()) {
                        yield LlmEvent::Text(text.to_owned());
                    }
                    if choice["finish_reason"].as_str() == Some("length") {
                        stop = StopReason::MaxTokens;
                    }
                }
            }
            Err(LlmError::Protocol("stream ended without [DONE]".into()))?;
        })
    }
}

/// Turns a non-2xx response into an error carrying (part of) its body.
async fn checked(resp: reqwest::Response) -> Result<reqwest::Response, LlmError> {
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status().as_u16();
    let text: String = resp
        .text()
        .await
        .unwrap_or_default()
        .chars()
        .take(300)
        .collect();
    Err(LlmError::from_status(status, text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::settings::LlmProviderKind;
    use crate::llm::{JsonSchema, LlmTarget, Message};

    #[test]
    fn body_includes_system_and_schema() {
        let req = LlmRequest {
            target: LlmTarget {
                provider: LlmProviderKind::OpenaiCompatible,
                model: "llama3.1".into(),
                base_url: None,
                api_key: None,
            },
            system: "be brief".into(),
            messages: vec![Message::user("hi")],
            max_tokens: 100,
            json_schema: Some(JsonSchema {
                name: "plan_proposal",
                schema: json!({"type":"object"}),
            }),
            effort: None,
            cacheable: false,
        };
        let body = OpenAiCompatProvider::body(&req);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(
            body["messages"][1],
            json!({"role": "user", "content": "hi"})
        );
        assert_eq!(
            body["response_format"]["json_schema"]["name"],
            "plan_proposal"
        );
    }
}
