//! Okapi BM25 ranking over an in-memory inverted index.
//!
//! Building is O(total tokens); a query costs O(Σ posting-list lengths of its terms).

use std::collections::HashMap;

/// Term-frequency saturation.
const K1: f64 = 1.2;
/// Length normalisation.
const B: f64 = 0.75;

const STOP_WORDS: [&str; 32] = [
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "has", "in", "is", "it", "its",
    "of", "on", "or", "that", "the", "this", "to", "was", "were", "will", "with", "we", "you",
    "our", "your", "into", "about",
];

/// Lowercased alphanumeric tokens without stop words.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .filter(|t| !STOP_WORDS.contains(&t.as_str()))
        .collect()
}

/// Inverted index over a fixed set of documents `0..n`.
#[derive(Debug, Clone, Default)]
pub struct Bm25Index {
    postings: HashMap<String, Vec<(usize, u32)>>,
    doc_len: Vec<u32>,
    avg_len: f64,
}

impl Bm25Index {
    /// Indexes `docs`; document `i` is `docs[i]`.
    pub fn build<S: AsRef<str>>(docs: &[S]) -> Self {
        let mut postings: HashMap<String, Vec<(usize, u32)>> = HashMap::new();
        let mut doc_len = Vec::with_capacity(docs.len());
        for (i, doc) in docs.iter().enumerate() {
            let tokens = tokenize(doc.as_ref());
            doc_len.push(tokens.len() as u32);
            let mut tf: HashMap<String, u32> = HashMap::new();
            for t in tokens {
                *tf.entry(t).or_default() += 1;
            }
            for (term, count) in tf {
                postings.entry(term).or_default().push((i, count));
            }
        }
        let total: u64 = doc_len.iter().map(|&l| u64::from(l)).sum();
        let avg_len = if doc_len.is_empty() {
            0.0
        } else {
            total as f64 / doc_len.len() as f64
        };
        Bm25Index {
            postings,
            doc_len,
            avg_len,
        }
    }

    /// Number of indexed documents.
    pub fn len(&self) -> usize {
        self.doc_len.len()
    }

    /// True when no document is indexed.
    pub fn is_empty(&self) -> bool {
        self.doc_len.is_empty()
    }

    /// BM25 score of every document for `query` (0 for non-matching ones).
    pub fn scores(&self, query: &str) -> Vec<f64> {
        let mut scores = vec![0.0; self.len()];
        let n = self.len() as f64;
        let mut terms = tokenize(query);
        terms.sort();
        terms.dedup();
        for term in terms {
            let Some(list) = self.postings.get(&term) else {
                continue;
            };
            let df = list.len() as f64;
            let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
            for &(doc, tf) in list {
                let tf = f64::from(tf);
                let len_norm = 1.0 - B + B * f64::from(self.doc_len[doc]) / self.avg_len.max(1.0);
                scores[doc] += idf * tf * (K1 + 1.0) / (tf + K1 * len_norm);
            }
        }
        scores
    }

    /// Scores divided by the maximum, so the best match scores 1.
    pub fn normalized_scores(&self, query: &str) -> Vec<f64> {
        let mut scores = self.scores(query);
        let max = scores.iter().copied().fold(0.0, f64::max);
        if max > 0.0 {
            scores.iter_mut().for_each(|s| *s /= max);
        }
        scores
    }

    /// Terms shared by `query` and document `doc`, most informative first.
    pub fn shared_terms(&self, query: &str, doc: usize, limit: usize) -> Vec<String> {
        let mut terms = tokenize(query);
        terms.sort();
        terms.dedup();
        let mut shared: Vec<(usize, String)> = terms
            .into_iter()
            .filter_map(|t| {
                let list = self.postings.get(&t)?;
                list.iter()
                    .any(|&(d, _)| d == doc)
                    .then_some((list.len(), t))
            })
            .collect();
        shared.sort();
        shared.into_iter().take(limit).map(|(_, t)| t).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_relevant_documents_first() {
        let docs = [
            "rust async runtime tokio scheduler",
            "baking bread with sourdough starter",
            "tokio runtime internals and the work stealing scheduler in rust",
        ];
        let idx = Bm25Index::build(&docs);
        let s = idx.scores("tokio scheduler");
        assert!(s[0] > s[1] && s[2] > s[1]);
        assert_eq!(s[1], 0.0);
        let n = idx.normalized_scores("tokio scheduler");
        assert!(n.iter().all(|&v| (0.0..=1.0).contains(&v)));
        assert!(n.contains(&1.0));
        assert_eq!(
            idx.shared_terms("the tokio sourdough", 2, 5),
            vec!["tokio".to_owned()]
        );
    }

    #[test]
    fn tokenizer_drops_stop_words() {
        assert_eq!(tokenize("The Plan, for the Graph!"), vec!["plan", "graph"]);
        assert!(Bm25Index::build::<&str>(&[]).scores("x").is_empty());
    }
}
