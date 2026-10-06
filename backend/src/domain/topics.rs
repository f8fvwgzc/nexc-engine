//! Unsupervised topics: clustering passage embeddings and naming the
//! clusters by the words that set them apart. Pure and deterministic, so the
//! same passages always give the same topics.

use std::collections::{HashMap, HashSet};

/// Most topics a workspace is split into.
pub const TOPICS_MAX: usize = 40;
/// Words in a topic's label.
pub const LABEL_TERMS: usize = 3;
const ITERATIONS: usize = 25;

/// How many topics to look for among `passages` passages: about the square
/// root of half of them, so topics stay large enough to mean something.
pub fn topic_count(passages: usize) -> usize {
    if passages < 8 {
        return 1;
    }
    let k = ((passages as f64) / 2.0).sqrt().round() as usize;
    k.clamp(2, TOPICS_MAX)
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn normalise(v: &mut [f32]) {
    let norm = dot(v, v).sqrt();
    if norm > 0.0 {
        for x in v {
            *x /= norm;
        }
    }
}

/// The index of the centre nearest to `v` (largest cosine; vectors are unit length).
pub fn nearest(centres: &[Vec<f32>], v: &[f32]) -> usize {
    let mut best = (0, f32::MIN);
    for (i, centre) in centres.iter().enumerate() {
        let similarity = dot(centre, v);
        if similarity > best.1 {
            best = (i, similarity);
        }
    }
    best.0
}

/// Spherical k-means over unit vectors of one size. Starts from the vector
/// closest to the mean and then, each time, the vector farthest from every
/// centre chosen so far, which needs no random numbers. Returns the centres
/// (unit length, empty clusters dropped) and each vector's centre.
pub fn cluster(vectors: &[Vec<f32>], k: usize) -> (Vec<Vec<f32>>, Vec<usize>) {
    let Some(dims) = vectors.first().map(Vec::len) else {
        return (Vec::new(), Vec::new());
    };
    let k = k.clamp(1, vectors.len());
    let mut mean = vec![0f32; dims];
    for v in vectors {
        for (m, x) in mean.iter_mut().zip(v) {
            *m += x;
        }
    }
    normalise(&mut mean);
    let mut centres = vec![vectors[nearest(vectors, &mean)].clone()];
    // The best similarity of each vector to any centre so far.
    let mut closest: Vec<f32> = vectors.iter().map(|v| dot(&centres[0], v)).collect();
    while centres.len() < k {
        let (farthest, _) = closest
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(b.1))
            .unwrap_or((0, &0.0));
        let centre = vectors[farthest].clone();
        for (c, v) in closest.iter_mut().zip(vectors) {
            *c = c.max(dot(&centre, v));
        }
        centres.push(centre);
    }
    let mut assignment = vec![usize::MAX; vectors.len()];
    for _ in 0..ITERATIONS {
        let mut changed = false;
        for (slot, v) in assignment.iter_mut().zip(vectors) {
            let centre = nearest(&centres, v);
            if *slot != centre {
                *slot = centre;
                changed = true;
            }
        }
        if !changed {
            break;
        }
        let mut sums = vec![vec![0f32; dims]; centres.len()];
        for (centre, v) in assignment.iter().zip(vectors) {
            for (s, x) in sums[*centre].iter_mut().zip(v) {
                *s += x;
            }
        }
        for (centre, mut sum) in centres.iter_mut().zip(sums) {
            // A centre that lost every vector keeps its place until the end.
            if sum.iter().any(|x| *x != 0.0) {
                normalise(&mut sum);
                *centre = sum;
            }
        }
    }
    // Drop centres nothing belongs to and renumber.
    let used: Vec<usize> = (0..centres.len())
        .filter(|c| assignment.contains(c))
        .collect();
    let renumber: HashMap<usize, usize> = used.iter().enumerate().map(|(n, c)| (*c, n)).collect();
    let centres = used.iter().map(|c| centres[*c].clone()).collect();
    let assignment = assignment.iter().map(|c| renumber[c]).collect();
    (centres, assignment)
}

fn words(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 4 && !w.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_lowercase)
        .collect()
}

/// For each cluster, the words that set it apart: common among its passages
/// and rare in the others, most telling first.
pub fn distinctive_terms(
    texts: &[&str],
    assignment: &[usize],
    clusters: usize,
) -> Vec<Vec<String>> {
    let mut sizes = vec![0usize; clusters];
    let mut inside: Vec<HashMap<String, usize>> = vec![HashMap::new(); clusters];
    let mut overall: HashMap<String, usize> = HashMap::new();
    for (text, cluster) in texts.iter().zip(assignment) {
        sizes[*cluster] += 1;
        for word in words(text) {
            *overall.entry(word.clone()).or_default() += 1;
            *inside[*cluster].entry(word).or_default() += 1;
        }
    }
    let total = texts.len();
    (0..clusters)
        .map(|cluster| {
            let size = sizes[cluster].max(1) as f64;
            let rest = (total - sizes[cluster]).max(1) as f64;
            let mut scored: Vec<(f64, &String)> = inside[cluster]
                .iter()
                .map(|(word, count)| {
                    let share_in = *count as f64 / size;
                    let share_out = (overall[word] - count) as f64 / rest;
                    (share_in - share_out, word)
                })
                .filter(|(score, _)| *score > 0.0)
                .collect();
            scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(b.1)));
            scored
                .into_iter()
                .take(LABEL_TERMS + 3)
                .map(|(_, word)| word.clone())
                .collect()
        })
        .collect()
}

/// A topic's name from its terms: "customs · invoices · port".
pub fn label(terms: &[String]) -> String {
    if terms.is_empty() {
        return "Miscellaneous".to_owned();
    }
    terms
        .iter()
        .take(LABEL_TERMS)
        .cloned()
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Clusters whose terms give the same label are one topic to a reader: they
/// are merged, their centre the mean of the two.
pub fn merge_same_label(
    centres: Vec<Vec<f32>>,
    terms: Vec<Vec<String>>,
) -> (Vec<Vec<f32>>, Vec<Vec<String>>) {
    let mut merged: Vec<(String, Vec<f32>, Vec<String>)> = Vec::new();
    for (centre, terms) in centres.into_iter().zip(terms) {
        let name = label(&terms);
        match merged.iter_mut().find(|(existing, _, _)| *existing == name) {
            Some((_, sum, _)) => {
                for (s, x) in sum.iter_mut().zip(&centre) {
                    *s += x;
                }
            }
            None => merged.push((name, centre, terms)),
        }
    }
    merged
        .into_iter()
        .map(|(_, mut centre, terms)| {
            normalise(&mut centre);
            (centre, terms)
        })
        .unzip()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(v: &[f32]) -> Vec<f32> {
        let mut v = v.to_vec();
        normalise(&mut v);
        v
    }

    #[test]
    fn the_number_of_topics_grows_slowly_and_is_capped() {
        assert_eq!(topic_count(0), 1);
        assert_eq!(topic_count(7), 1);
        assert_eq!(topic_count(8), 2);
        assert_eq!(topic_count(200), 10);
        assert_eq!(topic_count(4_000_000), TOPICS_MAX);
    }

    #[test]
    fn vectors_that_point_the_same_way_share_a_topic() {
        let vectors = vec![
            unit(&[1.0, 0.05, 0.0]),
            unit(&[0.0, 1.0, 0.1]),
            unit(&[0.95, 0.0, 0.1]),
            unit(&[0.05, 0.9, 0.0]),
            unit(&[0.0, 0.1, 1.0]),
            unit(&[1.0, 0.1, 0.05]),
        ];
        let (centres, assignment) = cluster(&vectors, 3);
        assert_eq!(centres.len(), 3);
        assert_eq!(assignment[0], assignment[2]);
        assert_eq!(assignment[0], assignment[5]);
        assert_eq!(assignment[1], assignment[3]);
        assert_ne!(assignment[0], assignment[1]);
        assert_ne!(assignment[4], assignment[0]);
        assert_ne!(assignment[4], assignment[1]);
        // The same input gives the same topics.
        assert_eq!(cluster(&vectors, 3).1, assignment);
        // Asking for more topics than vectors, or for none, is harmless.
        assert_eq!(cluster(&vectors, 99).0.len(), 6);
        assert!(cluster(&[], 3).0.is_empty());
    }

    #[test]
    fn a_topic_is_named_by_what_sets_it_apart() {
        let texts = [
            "customs reference for invoices at the port",
            "invoices need customs clearance at the port",
            "office plants are watered on mondays",
            "watering plants in the office every monday",
        ];
        let terms = distinctive_terms(&texts, &[0, 0, 1, 1], 2);
        assert!(terms[0].contains(&"customs".to_owned()), "{terms:?}");
        assert!(terms[0].contains(&"invoices".to_owned()));
        assert!(terms[1].contains(&"plants".to_owned()));
        assert!(!terms[0].contains(&"plants".to_owned()));
        assert_eq!(label(&terms[1]).split(" · ").count(), 3);
        assert_eq!(label(&[]), "Miscellaneous");
        // Two clusters a reader could not tell apart become one topic.
        let same = vec![
            "depot".to_owned(),
            "freight".to_owned(),
            "pallet".to_owned(),
        ];
        let (centres, merged) = merge_same_label(
            vec![vec![1.0, 0.0], vec![0.0, 1.0], vec![0.6, 0.8]],
            vec![same.clone(), terms[1].clone(), same],
        );
        assert_eq!((centres.len(), merged.len()), (2, 2));
        let length = centres[0].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (length - 1.0).abs() < 1e-5,
            "the merged centre is unit length"
        );
    }
}
