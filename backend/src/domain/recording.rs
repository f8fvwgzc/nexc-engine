//! Recording: what a span said, kept apart from the span when a workspace
//! turned it on. A span carries digests only; a payload carries the
//! prompt or completion of a model turn, or the input or output of a tool
//! call, after the secret scrubber and the label redaction ran over it.

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::capabilities::DataLabel;
use super::guardrails::Recording;
use super::string_enum;
use super::trace::SpanKind;

/// Most bytes kept of one payload; longer content is cut and says so.
pub const PAYLOAD_MAX_BYTES: usize = 1024 * 1024;
/// What a cut payload ends with.
pub const CUT_MARKER: &str = "\n\n[… cut: the payload was longer than 1 MiB]";

string_enum!(
    /// What a payload holds.
    PayloadKind {
        /// What a model turn was given (the system prompt and the messages since the last turn).
        Prompt => "prompt",
        /// What the model answered: its text and the tool calls it asked for.
        Completion => "completion",
        /// The arguments of a tool call.
        ToolInput => "tool_input",
        /// What the tool answered, or the error the model saw.
        ToolOutput => "tool_output",
    }
);

impl Recording {
    /// Whether anything is recorded at all.
    pub fn is_on(&self) -> bool {
        self.prompts || self.completions || self.tool_io
    }

    /// Whether payloads of `kind` are recorded.
    pub fn wants(&self, kind: PayloadKind) -> bool {
        match kind {
            PayloadKind::Prompt => self.prompts,
            PayloadKind::Completion => self.completions,
            PayloadKind::ToolInput | PayloadKind::ToolOutput => self.tool_io,
        }
    }

    /// The kinds recorded, as the `data_read` span notes them.
    pub fn kinds(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.prompts {
            out.push("prompts");
        }
        if self.completions {
            out.push("completions");
        }
        if self.tool_io {
            out.push("tool_io");
        }
        out
    }

    /// The setting in words, for the audit log.
    pub fn describe(&self) -> String {
        if !self.is_on() {
            return "recording off".to_owned();
        }
        let labels: Vec<&str> = self.redact_labels.iter().map(|l| l.as_str()).collect();
        format!(
            "recording {}; kept {} day(s); redacted labels: {}",
            self.kinds().join(", "),
            self.retention_days,
            if labels.is_empty() {
                "none".to_owned()
            } else {
                labels.join(", ")
            }
        )
    }
}

/// A piece of the node's context whose text is replaced in every payload:
/// a passage or memory of a label the workspace redacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redaction {
    /// The text as it was given to the agent.
    pub text: String,
    /// What stands in for it: the citation id.
    pub replacement: String,
}

impl Redaction {
    /// The redaction of a document passage given to the agent.
    pub fn passage(chunk_id: Uuid, label: DataLabel, text: String) -> Self {
        Redaction {
            text,
            replacement: format!("[redacted {label} passage {chunk_id}]"),
        }
    }

    /// The redaction of a memory given to the agent.
    pub fn memory(memory_id: Uuid, label: DataLabel, text: String) -> Self {
        Redaction {
            text,
            replacement: format!("[redacted {label} memory {memory_id}]"),
        }
    }
}

/// Replaces every occurrence of each redaction's text in `content`; the
/// content and how many pieces were replaced. A passage is matched whole and
/// also by its body alone (the agent may have been given it without its
/// citation line), longest first so that a passage inside another never
/// leaves a fragment behind.
pub fn redact(content: &str, redactions: &[Redaction]) -> (String, usize) {
    let mut needles: Vec<(&str, &str)> = Vec::new();
    for r in redactions {
        if r.text.trim().is_empty() {
            continue;
        }
        needles.push((r.text.as_str(), r.replacement.as_str()));
        if let Some((head, body)) = r.text.split_once('\n')
            && head.starts_with('[')
            && head.ends_with(']')
            && body.trim().len() >= 16
        {
            needles.push((body.trim(), r.replacement.as_str()));
        }
    }
    needles.sort_by_key(|(text, _)| std::cmp::Reverse(text.len()));
    let mut out = content.to_owned();
    let mut count = 0;
    for (text, replacement) in needles {
        let found = out.matches(text).count();
        if found > 0 {
            out = out.replace(text, replacement);
            count += found;
        }
    }
    (out, count)
}

/// Cuts `content` to [`PAYLOAD_MAX_BYTES`] on a character boundary, with a marker.
pub fn bounded(content: String) -> String {
    if content.len() <= PAYLOAD_MAX_BYTES {
        return content;
    }
    let mut end = PAYLOAD_MAX_BYTES - CUT_MARKER.len();
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = content[..end].to_owned();
    out.push_str(CUT_MARKER);
    out
}

/// One recorded payload of a span, decrypted.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SpanPayload {
    pub kind: PayloadKind,
    pub content: String,
    /// Bytes of the content as kept.
    pub size: i64,
    /// Passages and memories replaced by their citation, plus credentials scrubbed.
    pub redactions: i64,
    pub created_at: DateTime<Utc>,
}

/// What `GET /spans/{id}/payload` answers: the span's payloads with the
/// classifications of the data its node was given, for the banner.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SpanPayloads {
    pub span_id: Uuid,
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub kind: SpanKind,
    /// The classifications of the data the node was given: what the content may be derived from.
    pub labels: Vec<DataLabel>,
    /// Whether anything was recorded for the span; empty when recording was off.
    pub payloads: Vec<SpanPayload>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_knows_what_it_records() {
        let off = Recording::default();
        assert!(!off.is_on());
        assert!(!off.wants(PayloadKind::Prompt));
        assert_eq!(off.describe(), "recording off");
        assert_eq!(off.retention_days, 30);
        let on = Recording {
            prompts: true,
            tool_io: true,
            retention_days: 7,
            redact_labels: vec![DataLabel::Restricted],
            ..Recording::default()
        };
        assert!(on.wants(PayloadKind::Prompt) && on.wants(PayloadKind::ToolOutput));
        assert!(!on.wants(PayloadKind::Completion));
        assert_eq!(on.kinds(), ["prompts", "tool_io"]);
        assert_eq!(
            on.describe(),
            "recording prompts, tool_io; kept 7 day(s); redacted labels: restricted"
        );
    }

    #[test]
    fn redaction_replaces_passages_whole_or_by_their_body() {
        let chunk = Uuid::now_v7();
        let passage = "[board.pdf, p. 3]\nThe CEO earns 4.2M; the CFO 2.1M, both reviewed yearly.";
        let redactions = [
            Redaction::passage(chunk, DataLabel::Restricted, passage.to_owned()),
            Redaction::memory(Uuid::now_v7(), DataLabel::Confidential, String::new()),
        ];
        let prompt = format!(
            "<document source=\"board.pdf, p. 3\">\nThe CEO earns 4.2M; the CFO 2.1M, both reviewed yearly.\n</document>\n\nAlso: {passage}"
        );
        let (out, count) = redact(&prompt, &redactions);
        assert_eq!(count, 2, "{out}");
        assert!(!out.contains("4.2M"), "{out}");
        assert!(out.contains(&format!("[redacted restricted passage {chunk}]")));
        assert_eq!(
            redact("nothing here", &redactions),
            ("nothing here".to_owned(), 0)
        );
        assert_eq!(redact("x", &[]), ("x".to_owned(), 0));
    }

    #[test]
    fn payloads_are_cut_at_the_limit() {
        let long = "é".repeat(PAYLOAD_MAX_BYTES);
        let cut = bounded(long);
        assert!(cut.len() <= PAYLOAD_MAX_BYTES);
        assert!(cut.ends_with(CUT_MARKER));
        assert_eq!(bounded("short".into()), "short");
    }
}
