//! Fitting upstream context into a prompt.
//!
//! A node's prompt carries the outputs of the nodes before it. Those are
//! often far longer than the node needs, and they used to be cut after a
//! fixed number of characters, which keeps an introduction and drops the
//! findings. [`fit`] instead removes what carries no information and, when
//! the text is still too long, keeps the passages most relevant to the task.

use crate::dsa::bm25::Bm25Index;

/// The intro of a document frames the rest; it gets a head start in selection.
const FIRST_BLOCK_BONUS: f64 = 0.3;
/// Conclusions and summaries tend to sit at the end.
const LAST_BLOCK_BONUS: f64 = 0.15;

/// A text fitted into a budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fitted {
    pub text: String,
    /// Length of the text that was given, in characters.
    pub original_chars: usize,
}

impl Fitted {
    /// Characters that did not have to be sent.
    pub fn saved_chars(&self) -> usize {
        self.original_chars
            .saturating_sub(self.text.chars().count())
    }
}

/// Removes trailing whitespace and collapses runs of blank lines. Fenced
/// code blocks are left exactly as they are.
pub fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    let mut blank_run = 0;
    for line in text.lines() {
        let fence = line.trim_start().starts_with("```");
        if in_fence || fence {
            if fence {
                in_fence = !in_fence;
            }
            blank_run = 0;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let line = line.trim_end();
        if line.is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_owned()
}

/// Splits tidied text into passages: paragraphs separated by a blank line,
/// with a heading kept together with the paragraph it introduces and a
/// fenced code block never split.
fn passages(text: &str) -> Vec<String> {
    let mut blocks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_fence = false;
    let mut heading_only = false;
    for line in text.lines() {
        let fence = line.trim_start().starts_with("```");
        if fence {
            in_fence = !in_fence;
        }
        if line.is_empty() && !in_fence {
            // A heading waits for its paragraph instead of standing alone.
            if !current.is_empty() && !heading_only {
                blocks.push(std::mem::take(&mut current));
            }
            continue;
        }
        if !current.is_empty() {
            current.push('\n');
        }
        heading_only = (current.is_empty() || heading_only) && line.starts_with('#') && !in_fence;
        current.push_str(line);
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

fn omitted(chars: usize) -> String {
    format!("[… {chars} characters omitted …]")
}

/// Fits `text` into `budget` characters for a task described by `query`.
/// Text that fits after [`tidy`] is returned whole. Otherwise the passages
/// that best match the task are kept, in their original order, with a marker
/// wherever something was left out.
pub fn fit(text: &str, query: &str, budget: usize) -> Fitted {
    let original_chars = text.chars().count();
    let tidied = tidy(text);
    if tidied.chars().count() <= budget {
        return Fitted {
            text: tidied,
            original_chars,
        };
    }
    let blocks = passages(&tidied);
    let lengths: Vec<usize> = blocks.iter().map(|b| b.chars().count()).collect();
    let relevance = Bm25Index::build(&blocks).normalized_scores(query);
    let last = blocks.len().saturating_sub(1);
    let mut order: Vec<usize> = (0..blocks.len()).collect();
    let score = |i: usize| {
        relevance[i]
            + if i == 0 { FIRST_BLOCK_BONUS } else { 0.0 }
            + if i == last && last > 0 {
                LAST_BLOCK_BONUS
            } else {
                0.0
            }
    };
    // Best first; ties keep document order, so text with no match degrades to its head.
    order.sort_by(|&a, &b| score(b).total_cmp(&score(a)).then(a.cmp(&b)));

    // Every gap may need a marker; reserve room for them up front.
    let marker_room = omitted(original_chars).chars().count() + 2;
    let mut keep = vec![false; blocks.len()];
    let mut used = 0;
    for i in order {
        let cost = lengths[i] + 2 + marker_room;
        if used + cost <= budget {
            keep[i] = true;
            used += cost;
        }
    }
    if !keep.iter().any(|k| *k) {
        // One passage larger than the whole budget: its beginning is the best we can do.
        let head: String = tidied
            .chars()
            .take(budget.saturating_sub(marker_room))
            .collect();
        let dropped = tidied.chars().count() - head.chars().count();
        return Fitted {
            text: format!("{head}\n\n{}", omitted(dropped)),
            original_chars,
        };
    }

    let mut out = String::new();
    let mut gap = 0;
    for (i, block) in blocks.iter().enumerate() {
        if !keep[i] {
            gap += lengths[i];
            continue;
        }
        if gap > 0 {
            out.push_str(&omitted(gap));
            out.push_str("\n\n");
            gap = 0;
        }
        out.push_str(block);
        out.push_str("\n\n");
    }
    if gap > 0 {
        out.push_str(&omitted(gap));
    }
    Fitted {
        text: out.trim_end().to_owned(),
        original_chars,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidying_keeps_code_and_drops_padding() {
        let text = "Title   \n\n\n\nBody  \n```\n  keep   \n\n\n  this\n```\n\n\n";
        assert_eq!(tidy(text), "Title\n\nBody\n```\n  keep   \n\n\n  this\n```");
        assert_eq!(tidy("  \n\n"), "");
    }

    #[test]
    fn text_that_fits_is_only_tidied() {
        let fitted = fit("a  \n\n\n\nb", "anything", 100);
        assert_eq!(fitted.text, "a\n\nb");
        assert_eq!(fitted.original_chars, 8);
        assert_eq!(fitted.saved_chars(), 4);
    }

    #[test]
    fn keeps_the_passages_the_task_needs() {
        let filler = |topic: &str| format!("{topic} background. ").repeat(12).trim().to_owned();
        let text = [
            "# Report\n\nThis report covers several unrelated areas of the business.".to_owned(),
            format!(
                "## Catering\n\n{}",
                filler("Catering menus and lunch orders")
            ),
            format!(
                "## Parking\n\n{}",
                filler("Parking permits and bicycle racks")
            ),
            "## Gold\n\nReal yields and the dollar index drive the gold price; a falling ten-year \
             real yield has preceded each rally."
                .to_owned(),
            format!("## Travel\n\n{}", filler("Travel booking and hotel policy")),
            "## Conclusion\n\nRecommendations follow in the appendix.".to_owned(),
        ]
        .join("\n\n");
        let budget = 700;
        assert!(text.chars().count() > 2 * budget);
        let fitted = fit(
            &text,
            "Explain what drives the gold price and real yields",
            budget,
        );

        assert!(
            fitted.text.chars().count() <= budget,
            "{}",
            fitted.text.chars().count()
        );
        assert!(
            fitted.text.contains("falling ten-year"),
            "the relevant passage survives"
        );
        assert!(fitted.text.contains("## Gold"), "with its heading");
        assert!(fitted.text.starts_with("# Report"), "and the introduction");
        assert!(fitted.text.contains("characters omitted"));
        assert!(
            !fitted.text.contains("Parking permits") || !fitted.text.contains("Catering menus")
        );
        // Head truncation, which this replaces, would have lost the passage.
        let head: String = text.chars().take(budget).collect();
        assert!(!head.contains("falling ten-year"));
        assert!(fitted.saved_chars() > budget);
    }

    #[test]
    fn a_single_oversized_passage_keeps_its_beginning() {
        let text = "word ".repeat(400);
        let fitted = fit(&text, "word", 300);
        assert!(fitted.text.chars().count() <= 300);
        assert!(fitted.text.starts_with("word word"));
        assert!(fitted.text.ends_with("omitted …]"));
    }

    #[test]
    fn code_blocks_are_never_split() {
        let code = format!("```rust\n{}\n```", "let x = 1;\n\nlet y = 2;\n".repeat(3));
        let blocks = passages(&tidy(&format!("Intro\n\n{code}\n\nOutro")));
        assert_eq!(blocks.len(), 3);
        assert!(blocks[1].starts_with("```rust") && blocks[1].ends_with("```"));
    }
}
