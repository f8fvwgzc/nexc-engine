//! Incremental Server-Sent Events decoder for provider responses.

/// One decoded SSE event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Feeds raw bytes in arbitrary chunks and yields complete events.
#[derive(Debug, Default)]
pub struct SseDecoder {
    buf: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
}

/// Lines longer than this are a protocol violation (protects memory).
const MAX_LINE: usize = 16 * 1024 * 1024;

impl SseDecoder {
    /// Appends `chunk`; returns the events it completed.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, String> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buf.drain(..=pos).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line =
                String::from_utf8(line).map_err(|_| "invalid UTF-8 in event stream".to_owned())?;
            if let Some(event) = self.feed_line(&line) {
                out.push(event);
            }
        }
        if self.buf.len() > MAX_LINE {
            return Err("event stream line too long".into());
        }
        Ok(out)
    }

    fn feed_line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            if self.data.is_empty() && self.event.is_none() {
                return None;
            }
            let data = std::mem::take(&mut self.data).join("\n");
            return Some(SseEvent {
                event: self.event.take(),
                data,
            });
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => self.event = Some(value.to_owned()),
            "data" => self.data.push(value.to_owned()),
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_across_chunk_boundaries() {
        let mut d = SseDecoder::default();
        let raw = b"event: message_start\ndata: {\"a\":1}\n\n: comment\nevent: ping\r\ndata: {}\r\n\r\ndata: x\ndata: y\n\n";
        let mut events = Vec::new();
        for chunk in raw.chunks(7) {
            events.extend(d.push(chunk).unwrap());
        }
        assert_eq!(
            events,
            vec![
                SseEvent {
                    event: Some("message_start".into()),
                    data: "{\"a\":1}".into()
                },
                SseEvent {
                    event: Some("ping".into()),
                    data: "{}".into()
                },
                SseEvent {
                    event: None,
                    data: "x\ny".into()
                },
            ]
        );
    }
}
