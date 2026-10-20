//! The trace spine: every action an agent takes during a run is a span, and
//! the spans of a run form a hash chain.
//!
//! A span is appended once and never changed. Its hash covers the previous
//! span's hash and its own canonical JSON ([`canonical`]), so the chain is
//! tamper-evident: changing any stored span, or removing one, makes
//! recomputation ([`verify`]) stop at the first span that no longer fits.
//! The backend computes the chain in the order it receives spans; the agent
//! runtime, which is untrusted, only reports them.

use chrono::{DateTime, SecondsFormat, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use utoipa::ToSchema;
use uuid::Uuid;

use super::string_enum;
use crate::kernel;

/// The hash before the first span of a run: 32 zero bytes.
pub const GENESIS: [u8; 32] = [0; 32];
/// Longest span name kept.
pub const NAME_MAX: usize = 200;
/// Longest principal kept.
pub const PRINCIPAL_MAX: usize = 200;
/// Longest string kept inside `attributes`.
pub const ATTRIBUTE_STRING_MAX: usize = 1024;
/// Most entries kept in an attribute array.
pub const ATTRIBUTE_ARRAY_MAX: usize = 256;
/// Most attributes kept on one span.
pub const ATTRIBUTES_MAX: usize = 64;
/// Longest span line (as sent by the runtime) whose attributes are kept.
pub const SPAN_LINE_MAX_BYTES: usize = 64 * 1024;
/// Principal of spans the backend records on its own behalf.
pub const SYSTEM_PRINCIPAL: &str = "system";

string_enum!(
    /// What kind of action a span records.
    SpanKind {
        /// One model turn.
        LlmCall => "llm_call",
        /// One tool invocation.
        ToolCall => "tool_call",
        /// A sub-agent was born.
        Spawn => "spawn",
        /// Data was given to an agent (document passages, memories, upstream outputs).
        DataRead => "data_read",
        /// A call to a host outside the platform.
        Egress => "egress",
        /// A policy decision.
        Policy => "policy",
        /// A human approval.
        Approval => "approval",
    }
);

string_enum!(
    /// How a span ended.
    SpanStatus {
        Ok => "ok",
        Error => "error",
    }
);

/// A span to append to a run's chain, before the chain assigns its position
/// and hashes. `id` is the runtime's when it reported one, else fresh.
#[derive(Debug, Clone)]
pub struct SpanDraft {
    pub id: Uuid,
    pub node_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub principal: String,
    pub parent_id: Option<Uuid>,
    pub kind: SpanKind,
    pub name: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub status: SpanStatus,
    pub attributes: Value,
}

/// One span of a run's trace, as stored and as served (and as an evidence
/// bundle carries it, to be read back by the offline verifier).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Span {
    pub id: Uuid,
    pub run_id: Uuid,
    pub node_id: Uuid,
    #[schema(required = true)]
    pub agent_id: Option<Uuid>,
    /// Who acted: the agent's name as the runtime reports it, `system` for the backend.
    pub principal: String,
    /// The turn a tool call belongs to; the spawn a sub-agent's turns belong to.
    #[schema(required = true)]
    pub parent_id: Option<Uuid>,
    pub kind: SpanKind,
    /// The tool, the model, or what was read.
    pub name: String,
    pub started_at: DateTime<Utc>,
    #[schema(required = true)]
    pub ended_at: Option<DateTime<Utc>>,
    pub status: SpanStatus,
    /// Digests, sizes, counts and ids. Never tool arguments, never secrets.
    #[schema(value_type = Object)]
    pub attributes: Value,
    /// Position in the run's chain (order of receipt).
    pub seq: i32,
    /// Hex SHA-256 of the previous span in the chain (zeros for the first).
    #[serde(with = "super::audit_chain::hex32")]
    #[schema(value_type = String)]
    pub prev_hash: [u8; 32],
    /// Hex SHA-256 of `prev_hash` and this span's canonical JSON.
    #[serde(with = "super::audit_chain::hex32")]
    #[schema(value_type = String)]
    pub hash: [u8; 32],
}

/// Timestamps are stored with microsecond precision (`timestamptz`); the
/// hash is computed over the stored value, so that is what is kept.
pub fn truncate_micros(at: DateTime<Utc>) -> DateTime<Utc> {
    at.with_nanosecond(at.nanosecond() / 1000 * 1000)
        .unwrap_or(at)
}

fn canonical_time(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// The fields a span's hash covers, declared in sorted key order so that
/// serialising it gives the canonical JSON without building a map.
/// `attributes` is a `serde_json::Value`, whose objects are sorted maps.
#[derive(Serialize)]
struct Canonical<'a> {
    agent_id: Option<Uuid>,
    attributes: &'a Value,
    ended_at: Option<String>,
    id: Uuid,
    kind: SpanKind,
    name: &'a str,
    node_id: Uuid,
    parent_id: Option<Uuid>,
    principal: &'a str,
    run_id: Uuid,
    seq: i32,
    started_at: String,
    status: SpanStatus,
}

fn canonical_into(span: &Span, out: &mut Vec<u8>) {
    let fields = Canonical {
        agent_id: span.agent_id,
        attributes: &span.attributes,
        ended_at: span.ended_at.map(canonical_time),
        id: span.id,
        kind: span.kind,
        name: &span.name,
        node_id: span.node_id,
        parent_id: span.parent_id,
        principal: &span.principal,
        run_id: span.run_id,
        seq: span.seq,
        started_at: canonical_time(span.started_at),
        status: span.status,
    };
    serde_json::to_writer(out, &fields).expect("a span serialises");
}

/// The bytes a span's hash covers: every stored field but the hashes, as
/// JSON with keys in sorted order and no whitespace.
pub fn canonical(span: &Span) -> Vec<u8> {
    let mut out = Vec::with_capacity(512);
    canonical_into(span, &mut out);
    out
}

/// `SHA-256(prev || canonical)`.
pub fn chain_hash(prev: &[u8; 32], canonical: &[u8]) -> [u8; 32] {
    let mut material = Vec::with_capacity(32 + canonical.len());
    material.extend_from_slice(prev);
    material.extend_from_slice(canonical);
    kernel::sha256(&material)
}

/// `SHA-256(prev || canonical(span))`, with `buf` as scratch space.
fn hash_with(prev: &[u8; 32], span: &Span, buf: &mut Vec<u8>) -> [u8; 32] {
    buf.clear();
    buf.extend_from_slice(prev);
    canonical_into(span, buf);
    kernel::sha256(buf)
}

/// Spans below this many are checked on the calling thread.
const PARALLEL_FROM: usize = 1024;

/// Whether each span hashes to its stored `hash` from its stored
/// `prev_hash`. Each span's check stands alone, so large chains are split
/// across the machine's cores; the links between spans are checked after.
fn contents_match(spans: &[Span]) -> Vec<bool> {
    let check = |chunk: &[Span], out: &mut [bool]| {
        let mut buf = Vec::with_capacity(1024);
        for (span, ok) in chunk.iter().zip(out.iter_mut()) {
            *ok = hash_with(&span.prev_hash, span, &mut buf) == span.hash;
        }
    };
    let mut out = vec![false; spans.len()];
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    if spans.len() < PARALLEL_FROM || threads == 1 {
        check(spans, &mut out);
        return out;
    }
    let size = spans.len().div_ceil(threads);
    std::thread::scope(|scope| {
        for (chunk, part) in spans.chunks(size).zip(out.chunks_mut(size)) {
            scope.spawn(move || check(chunk, part));
        }
    });
    out
}

/// Appends a draft to a chain whose last hash is `prev`, as position `seq`.
pub fn link(draft: SpanDraft, run_id: Uuid, seq: i32, prev: [u8; 32]) -> Span {
    let mut span = Span {
        id: draft.id,
        run_id,
        node_id: draft.node_id,
        agent_id: draft.agent_id,
        principal: draft.principal,
        parent_id: draft.parent_id,
        kind: draft.kind,
        name: draft.name,
        started_at: truncate_micros(draft.started_at),
        ended_at: draft.ended_at.map(truncate_micros),
        status: draft.status,
        attributes: draft.attributes,
        seq,
        prev_hash: prev,
        hash: GENESIS,
    };
    span.hash = hash_with(&prev, &span, &mut Vec::with_capacity(1024));
    span
}

/// Why a chain does not verify.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BrokenLink {
    /// The first span that does not fit the chain.
    pub span_id: Uuid,
    pub seq: i32,
    pub reason: String,
}

/// Result of recomputing a run's chain.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TraceVerification {
    /// True when every span hashes as stored, every link holds, and the
    /// sealed hash (if the run is finished) is the chain's last hash.
    pub ok: bool,
    /// Spans in the chain.
    pub spans: i64,
    /// Hex hash of the last span, `null` for an empty chain.
    #[schema(required = true)]
    pub hash: Option<String>,
    /// Hex hash written when the run finished, `null` while it runs or if nothing was traced.
    #[schema(required = true)]
    pub sealed_hash: Option<String>,
    /// The first span that does not fit, when `ok` is false.
    #[schema(required = true)]
    pub broken: Option<BrokenLink>,
    /// The check of the run's signed seal; `null` while it runs, when nothing
    /// was traced, and for runs sealed before seals were signed.
    #[schema(required = true)]
    pub seal: Option<super::seal::SealVerification>,
}

impl TraceVerification {
    /// Adds the check of the run's signed seal: the trace verifies only when it holds too.
    pub fn with_seal(mut self, seal: Option<super::seal::SealVerification>) -> Self {
        if let Some(check) = &seal {
            self.ok &= check.ok;
        }
        self.seal = seal;
        self
    }

    /// The chain as recomputed (span count and last hash), when it holds.
    pub fn chain(&self) -> Option<(i64, [u8; 32])> {
        let hash = hex::decode(self.hash.as_deref()?).ok()?.try_into().ok()?;
        (self.broken.is_none()).then_some((self.spans, hash))
    }
}

/// Recomputes the chain over `spans` (in `seq` order) and compares it with
/// what is stored, and with `sealed` when the run has been sealed.
pub fn verify(spans: &[Span], sealed: Option<[u8; 32]>) -> TraceVerification {
    let contents = contents_match(spans);
    let mut prev = GENESIS;
    let mut broken = None;
    for (position, span) in spans.iter().enumerate() {
        let expected_seq = position as i32;
        let reason = if span.seq != expected_seq {
            Some(format!("span {expected_seq} is missing from the chain"))
        } else if span.prev_hash != prev {
            Some("prev_hash does not link to the previous span".to_owned())
        } else if !contents[position] {
            Some("hash does not match the span's content".to_owned())
        } else {
            None
        };
        if let Some(reason) = reason {
            broken = Some(BrokenLink {
                span_id: span.id,
                seq: span.seq,
                reason,
            });
            break;
        }
        prev = span.hash;
    }
    let last = spans.last().map(|s| s.hash);
    let sealed_ok = match (sealed, last) {
        (Some(sealed), Some(last)) => sealed == last,
        (Some(_), None) => false,
        (None, _) => true,
    };
    if broken.is_none()
        && !sealed_ok
        && let Some(last_span) = spans.last()
    {
        broken = Some(BrokenLink {
            span_id: last_span.id,
            seq: last_span.seq,
            reason: "the chain's last hash is not the hash sealed when the run finished".into(),
        });
    }
    TraceVerification {
        ok: broken.is_none() && sealed_ok,
        spans: spans.len() as i64,
        hash: last.map(hex::encode),
        sealed_hash: sealed.map(hex::encode),
        broken,
        seal: None,
    }
}

/// A page of a run's trace.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TracePage {
    /// Spans in id order (UUID v7: the order they started).
    pub spans: Vec<Span>,
    /// Pass as `after` to get the next page; `null` on the last page.
    #[schema(required = true)]
    pub next: Option<Uuid>,
}

fn bounded(text: &str, max: usize) -> String {
    text.chars().filter(|c| *c != '\0').take(max).collect()
}

/// A name as stored: no NUL bytes (which `text` refuses), at most [`NAME_MAX`] characters.
pub fn clean_name(name: &str) -> String {
    bounded(name, NAME_MAX)
}

/// A principal as stored; empty becomes `agent`.
pub fn clean_principal(principal: &str) -> String {
    let cleaned = bounded(principal, PRINCIPAL_MAX);
    if cleaned.trim().is_empty() {
        "agent".to_owned()
    } else {
        cleaned
    }
}

fn clean_scalar(value: &Value) -> Option<Value> {
    match value {
        Value::Null | Value::Bool(_) => Some(value.clone()),
        Value::Number(n) => Some(match n.as_i64() {
            Some(i) => Value::from(i),
            // Floats and huge integers are kept as text: `jsonb` would reformat them,
            // and the hash must survive the round trip through the database.
            None => Value::String(n.to_string()),
        }),
        Value::String(s) => Some(Value::String(bounded(s, ATTRIBUTE_STRING_MAX))),
        Value::Array(_) | Value::Object(_) => None,
    }
}

/// Attributes as stored: a flat object of scalars and arrays of scalars,
/// bounded in size, with nothing that cannot round-trip through `jsonb`.
/// Anything else is dropped.
pub fn clean_attributes(value: &Value) -> Value {
    let Value::Object(given) = value else {
        return Value::Object(Map::new());
    };
    let mut out = Map::new();
    for (key, value) in given.iter().take(ATTRIBUTES_MAX) {
        let key = bounded(key, 100);
        if key.is_empty() {
            continue;
        }
        let cleaned = match value {
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .take(ATTRIBUTE_ARRAY_MAX)
                    .filter_map(clean_scalar)
                    .collect(),
            ),
            other => match clean_scalar(other) {
                Some(v) => v,
                None => continue,
            },
        };
        out.insert(key, cleaned);
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn draft(name: &str) -> SpanDraft {
        SpanDraft {
            id: Uuid::now_v7(),
            node_id: Uuid::now_v7(),
            agent_id: None,
            principal: "writer".into(),
            parent_id: None,
            kind: SpanKind::ToolCall,
            name: name.into(),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            status: SpanStatus::Ok,
            attributes: json!({"args_sha256": "ab", "result_bytes": 12}),
        }
    }

    fn chain(n: usize) -> Vec<Span> {
        let run = Uuid::now_v7();
        let mut prev = GENESIS;
        (0..n)
            .map(|i| {
                let span = link(draft(&format!("tool-{i}")), run, i as i32, prev);
                prev = span.hash;
                span
            })
            .collect()
    }

    #[test]
    fn a_chain_verifies_and_names_the_first_broken_span() {
        let spans = chain(5);
        assert_eq!(spans[0].prev_hash, GENESIS);
        for pair in spans.windows(2) {
            assert_eq!(pair[1].prev_hash, pair[0].hash);
        }
        let last = spans.last().unwrap().hash;
        assert!(verify(&spans, Some(last)).ok);
        assert!(verify(&spans, None).ok, "an unsealed run verifies");
        assert!(!verify(&spans, Some(GENESIS)).ok, "a wrong seal does not");

        let mut tampered = spans.clone();
        tampered[2].attributes = json!({"args_sha256": "cd", "result_bytes": 12});
        let result = verify(&tampered, Some(last));
        assert!(!result.ok);
        assert_eq!(result.broken.as_ref().unwrap().span_id, spans[2].id);
        assert_eq!(result.broken.as_ref().unwrap().seq, 2);

        let mut rehashed = spans.clone();
        rehashed[3].hash = GENESIS;
        let result = verify(&rehashed, Some(last));
        assert_eq!(result.broken.unwrap().span_id, spans[3].id);

        let mut removed = spans.clone();
        removed.remove(1);
        let result = verify(&removed, Some(last));
        assert_eq!(result.broken.unwrap().seq, 2);

        let empty = verify(&[], None);
        assert!(empty.ok && empty.hash.is_none() && empty.spans == 0);
    }

    #[test]
    fn large_chains_verify_in_parallel_and_still_name_the_first_break() {
        let mut spans = chain(PARALLEL_FROM * 3 + 7);
        let last = spans.last().unwrap().hash;
        assert!(verify(&spans, Some(last)).ok);
        let late = spans.len() - 5;
        spans[late].name.push('x');
        spans[PARALLEL_FROM + 1].name.push('x');
        let result = verify(&spans, Some(last));
        assert_eq!(result.broken.unwrap().seq, PARALLEL_FROM as i32 + 1);
    }

    #[test]
    fn canonical_json_is_the_sorted_map_of_the_fields() {
        let span = chain(1).remove(0);
        let by_map = serde_json::to_vec(&json!({
            "id": span.id, "run_id": span.run_id, "node_id": span.node_id,
            "agent_id": span.agent_id, "principal": span.principal, "parent_id": span.parent_id,
            "kind": span.kind, "name": span.name, "started_at": canonical_time(span.started_at),
            "ended_at": span.ended_at.map(canonical_time), "status": span.status,
            "attributes": span.attributes, "seq": span.seq,
        }))
        .unwrap();
        assert_eq!(canonical(&span), by_map);
        assert_eq!(span.hash, chain_hash(&span.prev_hash, &by_map));
    }

    #[test]
    fn canonical_json_is_sorted_and_micro_precise() {
        let mut span = chain(1).remove(0);
        span.started_at = "2026-10-17T12:00:00.123456789Z".parse().unwrap();
        let text = String::from_utf8(canonical(&span)).unwrap();
        assert!(text.starts_with("{\"agent_id\":"));
        assert!(!text.contains("123456789"), "nanoseconds are not stored");
        assert_eq!(
            canonical_time(truncate_micros(span.started_at)),
            "2026-10-17T12:00:00.123456Z"
        );
        assert!(!text.contains("\"hash\""));
    }

    #[test]
    fn attributes_are_flattened_and_bounded() {
        let cleaned = clean_attributes(&json!({
            "tool": "write_file",
            "bytes": 12,
            "ratio": 1.5,
            "flag": true,
            "ids": ["a", {"nested": 1}, 2],
            "nested": {"dropped": true},
            "long": "x".repeat(5000),
            "nul": "a\u{0}b",
        }));
        assert_eq!(cleaned["tool"], "write_file");
        assert_eq!(cleaned["bytes"], 12);
        assert_eq!(cleaned["ratio"], "1.5");
        assert_eq!(cleaned["ids"], json!(["a", 2]));
        assert!(cleaned.get("nested").is_none());
        assert_eq!(
            cleaned["long"].as_str().unwrap().len(),
            ATTRIBUTE_STRING_MAX
        );
        assert_eq!(cleaned["nul"], "ab");
        assert_eq!(clean_attributes(&json!([1, 2])), json!({}));
        assert_eq!(clean_principal("  "), "agent");
        assert_eq!(clean_name(&"n".repeat(500)).len(), NAME_MAX);
    }
}
