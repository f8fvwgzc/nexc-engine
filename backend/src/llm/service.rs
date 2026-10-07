//! The LLM entry point used by the engine: provider + response cache + metrics.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_stream::stream;
use futures::StreamExt;
use sha2::{Digest, Sha256};

use super::{Completion, LlmEvent, LlmProvider, LlmRequest, LlmStream, StopReason, Usage};
use crate::dsa::lru::LruCache;
use crate::observability::metrics::Metrics;

const CACHE_CAPACITY: usize = 512;
const CACHE_TTL: Duration = Duration::from_secs(6 * 3600);

/// Streams completions through the configured provider, serving identical
/// cacheable requests from an LRU keyed by `sha256(provider, model, system,
/// messages, schema, limits)`.
pub struct LlmService {
    provider: Arc<dyn LlmProvider>,
    cache: Arc<Mutex<LruCache<String, Completion>>>,
    metrics: Arc<Metrics>,
}

impl LlmService {
    /// Service over `provider`.
    pub fn new(provider: Arc<dyn LlmProvider>, metrics: Arc<Metrics>) -> Self {
        LlmService {
            provider,
            cache: Arc::new(Mutex::new(LruCache::new(CACHE_CAPACITY, CACHE_TTL))),
            metrics,
        }
    }

    /// Starts a streamed completion.
    pub fn stream(&self, request: LlmRequest) -> LlmStream {
        if !request.cacheable {
            return self.metered(self.provider.stream(request), None);
        }
        let key = cache_key(&request);
        let hit = self
            .cache
            .lock()
            .expect("llm cache lock")
            .get(&key)
            .cloned();
        if let Some(done) = hit {
            self.metrics.llm_cache_hits.add(1);
            return Box::pin(futures::stream::iter([
                Ok(LlmEvent::Text(done.text)),
                Ok(LlmEvent::Usage(Usage::default())),
                Ok(LlmEvent::Done(StopReason::EndTurn)),
            ]));
        }
        self.metrics.llm_cache_misses.add(1);
        self.metered(self.provider.stream(request), Some(key))
    }

    /// Counts tokens and, when `cache_key` is set, stores complete responses.
    fn metered(&self, mut inner: LlmStream, cache_key: Option<String>) -> LlmStream {
        let metrics = self.metrics.clone();
        let cache = self.cache.clone();
        Box::pin(stream! {
            let mut text = String::new();
            let mut usage = Usage::default();
            while let Some(event) = inner.next().await {
                match &event {
                    Ok(LlmEvent::Text(t)) if cache_key.is_some() => text.push_str(t),
                    Ok(LlmEvent::Usage(u)) => usage = *u,
                    Ok(LlmEvent::Done(reason)) => {
                        metrics.llm_tokens.add(&[("direction", "input")], usage.input_tokens);
                        metrics.llm_tokens.add(&[("direction", "output")], usage.output_tokens);
                        if let (Some(key), StopReason::EndTurn) = (&cache_key, reason) {
                            let done = Completion { text: std::mem::take(&mut text), usage, truncated: false };
                            cache.lock().expect("llm cache lock").put(key.clone(), done);
                        }
                    }
                    _ => {}
                }
                yield event;
            }
        })
    }
}

fn cache_key(r: &LlmRequest) -> String {
    let payload = serde_json::json!({
        "provider": r.target.provider,
        "model": r.target.model,
        "base_url": r.target.base_url,
        "system": r.system,
        "messages": r.messages,
        "schema": r.json_schema,
        "max_tokens": r.max_tokens,
        "effort": r.effort,
    });
    hex::encode(Sha256::digest(payload.to_string().as_bytes()))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::domain::settings::LlmProviderKind;
    use crate::llm::{LlmTarget, Message, collect};

    struct Counting(AtomicUsize);

    impl LlmProvider for Counting {
        fn stream(&self, _: LlmRequest) -> LlmStream {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(futures::stream::iter([
                Ok(LlmEvent::Text("answer".into())),
                Ok(LlmEvent::Usage(Usage {
                    input_tokens: 3,
                    output_tokens: 1,
                    cached_tokens: 0,
                })),
                Ok(LlmEvent::Done(StopReason::EndTurn)),
            ]))
        }
    }

    fn request(cacheable: bool) -> LlmRequest {
        LlmRequest {
            target: LlmTarget {
                provider: LlmProviderKind::Demo,
                model: "m".into(),
                base_url: None,
                api_key: None,
            },
            system: "s".into(),
            messages: vec![Message::user("q")],
            max_tokens: 10,
            json_schema: None,
            effort: None,
            cacheable,
        }
    }

    #[tokio::test]
    async fn caches_identical_requests() {
        let provider = Arc::new(Counting(AtomicUsize::new(0)));
        let metrics = Arc::new(Metrics::default());
        let svc = LlmService::new(provider.clone(), metrics.clone());
        for _ in 0..3 {
            let c = collect(svc.stream(request(true)), |_| {}).await.unwrap();
            assert_eq!(c.text, "answer");
        }
        assert_eq!(provider.0.load(Ordering::SeqCst), 1);
        collect(svc.stream(request(false)), |_| {}).await.unwrap();
        assert_eq!(
            provider.0.load(Ordering::SeqCst),
            2,
            "non-cacheable requests always hit the provider"
        );
        assert!(metrics.render().contains("nexc_llm_cache_hits_total 2"));
    }
}
