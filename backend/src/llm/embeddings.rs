//! Text embeddings for the knowledge base.
//!
//! A workspace (or the server) names an OpenAI-compatible `/embeddings`
//! endpoint; without one the built-in feature-hashing embedding of the C
//! kernel is used, which matches shared words rather than meaning but needs
//! no service and no key.

use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use crate::config::Secret;
use crate::domain::knowledge::BUILTIN_EMBED_MODEL;
use crate::kernel;

/// Inputs sent per request.
pub const BATCH: usize = 64;
const ATTEMPTS: u32 = 4;
const TIMEOUT: Duration = Duration::from_secs(60);

/// Where embeddings come from.
#[derive(Debug, Clone)]
pub struct EmbedTarget {
    /// `None` is the built-in embedding.
    pub base_url: Option<String>,
    pub model: String,
    pub dims: Option<u32>,
    pub api_key: Option<Secret<String>>,
}

impl EmbedTarget {
    pub fn builtin() -> Self {
        EmbedTarget {
            base_url: None,
            model: BUILTIN_EMBED_MODEL.to_owned(),
            dims: None,
            api_key: None,
        }
    }

    /// Whether the embedding understands meaning (the built-in one does not).
    pub fn is_semantic(&self) -> bool {
        self.base_url.is_some()
    }
}

#[derive(Deserialize)]
struct Response {
    data: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    index: usize,
    embedding: Vec<f32>,
}

/// The vectors of a response in the order of the inputs, L2-normalised so
/// that a dot product is a cosine. Fails unless there is one per input.
fn vectors(body: &str, inputs: usize) -> anyhow::Result<Vec<Vec<f32>>> {
    let mut response: Response = serde_json::from_str(body)?;
    anyhow::ensure!(
        response.data.len() == inputs,
        "the embeddings endpoint returned {} vectors for {inputs} inputs",
        response.data.len()
    );
    response.data.sort_by_key(|item| item.index);
    Ok(response
        .data
        .into_iter()
        .map(|item| normalised(item.embedding))
        .collect())
}

fn normalised(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

/// Embeds `inputs` (at most [`BATCH`] per call is sensible), in order.
/// Rate limits and server errors are retried with a growing pause.
pub async fn embed(
    http: &reqwest::Client,
    target: &EmbedTarget,
    inputs: &[String],
) -> anyhow::Result<Vec<Vec<f32>>> {
    let Some(base) = target.base_url.as_deref() else {
        return Ok(inputs
            .iter()
            .map(|text| kernel::embed(text).to_vec())
            .collect());
    };
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    let mut body = json!({ "model": target.model, "input": inputs });
    if let Some(dims) = target.dims {
        body["dimensions"] = json!(dims);
    }
    let url = format!("{}/embeddings", base.trim_end_matches('/'));
    let mut pause = Duration::from_millis(500);
    for attempt in 1..=ATTEMPTS {
        let mut request = http.post(&url).timeout(TIMEOUT).json(&body);
        if let Some(key) = &target.api_key {
            request = request.bearer_auth(key.expose());
        }
        let retryable = match request.send().await {
            Ok(response) => {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                if status.is_success() {
                    return vectors(&text, inputs.len());
                }
                let detail: String = text.chars().take(300).collect();
                if !(status.as_u16() == 429 || status.is_server_error()) {
                    anyhow::bail!("the embeddings endpoint answered {status}: {detail}");
                }
                format!("{status}: {detail}")
            }
            Err(err) => err.without_url().to_string(),
        };
        if attempt == ATTEMPTS {
            anyhow::bail!("the embeddings endpoint kept failing: {retryable}");
        }
        tokio::time::sleep(pause).await;
        pause *= 3;
    }
    unreachable!("the loop returns or bails on its last attempt")
}

/// Vectors as stored: little-endian f32s.
pub fn to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// The dot product of a stored vector with `query`, or `None` when they are
/// not the same length (a different model).
pub fn dot_bytes(stored: &[u8], query: &[f32]) -> Option<f32> {
    if stored.len() != query.len() * 4 {
        return None;
    }
    Some(
        stored
            .as_chunks::<4>()
            .0
            .iter()
            .zip(query)
            .map(|(b, q)| f32::from_le_bytes(*b) * q)
            .sum(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_vectors_are_ordered_and_normalised() {
        let body = r#"{"data":[{"index":1,"embedding":[0,2]},{"index":0,"embedding":[3,4]}]}"#;
        let v = vectors(body, 2).unwrap();
        assert_eq!(v, vec![vec![0.6, 0.8], vec![0.0, 1.0]]);
        assert!(vectors(body, 3).is_err(), "one vector per input");
    }

    #[test]
    fn stored_vectors_compare_only_with_their_own_size() {
        let stored = to_bytes(&[0.6, 0.8]);
        assert_eq!(dot_bytes(&stored, &[1.0, 0.0]), Some(0.6));
        assert_eq!(dot_bytes(&stored, &[1.0, 0.0, 0.0]), None);
    }

    #[tokio::test]
    async fn the_builtin_embedding_needs_no_service() {
        let http = reqwest::Client::new();
        let v = embed(&http, &EmbedTarget::builtin(), &["hello world".into()])
            .await
            .unwrap();
        assert_eq!(v[0].len(), kernel::EMBED_DIM);
        assert!(!EmbedTarget::builtin().is_semantic());
    }
}
