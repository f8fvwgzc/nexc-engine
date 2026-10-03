//! Incremental extraction of array elements from a JSON document that is
//! still being streamed, so that plan nodes can be shown as soon as the LLM
//! has finished writing each one.

/// Finds complete object elements of the top-level array field `key`
/// (e.g. `"nodes": [ {…}, {…`) in a growing buffer.
#[derive(Debug)]
pub struct ArrayScanner {
    key: &'static str,
    emitted: usize,
}

impl ArrayScanner {
    /// Scanner for the array stored under `key` in the root object.
    pub fn new(key: &'static str) -> Self {
        ArrayScanner { key, emitted: 0 }
    }

    /// Raw JSON of the elements completed since the previous call.
    /// `buf` must be the whole document received so far.
    pub fn scan<'a>(&mut self, buf: &'a str) -> Vec<&'a str> {
        let bytes = buf.as_bytes();
        let (mut depth, mut in_str, mut escaped) = (0usize, false, false);
        let (mut str_start, mut last_key) = (0usize, None::<(usize, usize)>);
        let (mut expecting_array, mut in_array) = (false, false);
        let mut element_start = None;
        let mut count = 0;
        let mut out = Vec::new();
        for (i, &c) in bytes.iter().enumerate() {
            if in_str {
                if escaped {
                    escaped = false;
                } else if c == b'\\' {
                    escaped = true;
                } else if c == b'"' {
                    in_str = false;
                    if depth == 1 {
                        last_key = Some((str_start, i));
                    }
                }
                continue;
            }
            match c {
                b'"' => {
                    in_str = true;
                    str_start = i + 1;
                }
                b':' if depth == 1 => {
                    expecting_array = last_key.is_some_and(|(s, e)| &buf[s..e] == self.key);
                }
                b'[' => {
                    depth += 1;
                    in_array |= expecting_array && depth == 2;
                    expecting_array = false;
                }
                b'{' => {
                    depth += 1;
                    if in_array && depth == 3 {
                        element_start = Some(i);
                    }
                }
                b'}' => {
                    if in_array
                        && depth == 3
                        && let Some(start) = element_start.take()
                    {
                        if count >= self.emitted {
                            out.push(&buf[start..=i]);
                        }
                        count += 1;
                    }
                    depth = depth.saturating_sub(1);
                }
                b']' => {
                    if in_array && depth == 2 {
                        in_array = false;
                    }
                    depth = depth.saturating_sub(1);
                }
                b',' if depth == 1 => last_key = None,
                _ => {}
            }
        }
        self.emitted = self.emitted.max(count);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_each_element_once_as_it_completes() {
        let doc = r#"{"summary":"a \"nodes\": [ {} ]","nodes":[{"ref":"a","tags":["x"],"o":{"k":"}"}},{"ref":"b"}],"edges":[{"source_ref":"a"}]}"#;
        let mut nodes = ArrayScanner::new("nodes");
        let mut edges = ArrayScanner::new("edges");
        let (mut got_nodes, mut got_edges) = (Vec::new(), Vec::new());
        for end in 1..=doc.len() {
            got_nodes.extend(nodes.scan(&doc[..end]).into_iter().map(str::to_owned));
            got_edges.extend(edges.scan(&doc[..end]).into_iter().map(str::to_owned));
        }
        assert_eq!(
            got_nodes,
            vec![
                r#"{"ref":"a","tags":["x"],"o":{"k":"}"}}"#,
                r#"{"ref":"b"}"#
            ]
        );
        assert_eq!(got_edges, vec![r#"{"source_ref":"a"}"#]);
    }
}
