//! Topics of a workspace's memory: memories are clustered by their embeddings
//! and each cluster is named by the words that set it apart, so a member can
//! see what the workspace knows about and read one subject at a time.
//!
//! The topics are found in, and named from, the memories every member of the
//! workspace may read. Personal notes and the memories of private teams are
//! then placed under the nearest topic, but never shape a name.

use std::collections::HashMap;

use sqlx::PgPool;
use uuid::Uuid;

use super::reader;
use crate::domain::memory::MemoryTopic;
use crate::domain::topics as cluster;
use crate::kernel;
use crate::repo::memories;

/// Memories the topics are found in; the rest are placed under the nearest topic.
const SAMPLE: i64 = 10_000;
/// Memories placed per statement.
const BATCH: i64 = 2_000;
/// A workspace gets its first topics once it has this many shared memories.
pub const TOPICS_FROM: usize = 20;

fn floats(bytes: &[u8]) -> Option<Vec<f32>> {
    kernel::embedding_from_bytes(bytes).map(|e| e.to_vec())
}

/// Places memories under the topic whose centre is nearest: all of them, or
/// only those that have none yet.
async fn place(
    db: &PgPool,
    workspace: Uuid,
    centres: &[(Uuid, Vec<f32>)],
    only_unplaced: bool,
) -> Result<(), sqlx::Error> {
    if centres.is_empty() {
        return Ok(());
    }
    let vectors: Vec<Vec<f32>> = centres.iter().map(|(_, c)| c.clone()).collect();
    let mut after = None;
    loop {
        let batch = memories::embeddings_after(db, workspace, after, only_unplaced, BATCH).await?;
        let Some(last) = batch.last().map(|(id, _)| *id) else {
            return Ok(());
        };
        after = Some(last);
        let mut memory_ids = Vec::with_capacity(batch.len());
        let mut topic_ids = Vec::with_capacity(batch.len());
        for (id, bytes) in &batch {
            if let Some(v) = floats(bytes) {
                memory_ids.push(*id);
                topic_ids.push(centres[cluster::nearest(&vectors, &v)].0);
            }
        }
        memories::assign_topics(db, &memory_ids, &topic_ids).await?;
    }
}

/// Finds the topics of a workspace's memory afresh and places every memory.
/// Returns how many topics there are.
pub async fn rebuild(db: &PgPool, workspace: Uuid) -> anyhow::Result<usize> {
    let sample: Vec<(String, Vec<f32>)> = memories::sample_shared(db, workspace, SAMPLE)
        .await?
        .into_iter()
        .filter_map(|(_, content, bytes)| Some((content, floats(&bytes)?)))
        .collect();
    if sample.is_empty() {
        let mut tx = db.begin().await?;
        memories::replace_topics(&mut tx, workspace, &[]).await?;
        tx.commit().await?;
        return Ok(0);
    }
    // Clustering is CPU work; it must not hold up the async runtime.
    let (centres, terms) = tokio::task::spawn_blocking(move || {
        let vectors: Vec<Vec<f32>> = sample.iter().map(|(_, v)| v.clone()).collect();
        let (centres, assignment) = cluster::cluster(&vectors, cluster::topic_count(vectors.len()));
        let texts: Vec<&str> = sample.iter().map(|(content, _)| content.as_str()).collect();
        let terms = cluster::distinctive_terms(&texts, &assignment, centres.len());
        cluster::merge_same_label(centres, terms)
    })
    .await?;
    let new: Vec<(Uuid, String, Vec<String>, Vec<u8>)> = centres
        .iter()
        .zip(&terms)
        .map(|(centre, terms)| {
            let bytes = centre.iter().flat_map(|x| x.to_le_bytes()).collect();
            (Uuid::now_v7(), cluster::label(terms), terms.clone(), bytes)
        })
        .collect();
    let mut tx = db.begin().await?;
    memories::replace_topics(&mut tx, workspace, &new).await?;
    tx.commit().await?;
    let centres: Vec<(Uuid, Vec<f32>)> = new.iter().map(|t| t.0).zip(centres).collect();
    place(db, workspace, &centres, false).await?;
    Ok(new.len())
}

/// After memories were stored: the first topics are found once the workspace
/// has enough shared memories; afterwards new memories join the nearest topic.
pub async fn place_new(db: &PgPool, workspace: Uuid) -> anyhow::Result<()> {
    let existing = memories::topics(db, workspace).await?;
    if existing.is_empty() {
        let shared = memories::sample_shared(db, workspace, TOPICS_FROM as i64).await?;
        if shared.len() >= TOPICS_FROM {
            rebuild(db, workspace).await?;
        }
        return Ok(());
    }
    let centres: Vec<(Uuid, Vec<f32>)> = existing
        .iter()
        .filter_map(|t| Some((t.id, floats(&t.centroid)?)))
        .collect();
    Ok(place(db, workspace, &centres, true).await?)
}

/// The topics of a workspace with how many memories of each `user` may read,
/// largest first; a topic with none for this reader is left out.
pub async fn list(db: &PgPool, user: Uuid, workspace: Uuid) -> anyhow::Result<Vec<MemoryTopic>> {
    let reader = reader(db, user, workspace).await?;
    let graphs: Vec<Uuid> = reader.graphs.iter().copied().collect();
    let counts: HashMap<Uuid, i64> = memories::topic_counts(db, workspace, &graphs, user)
        .await?
        .into_iter()
        .collect();
    let mut topics: Vec<MemoryTopic> = memories::topics(db, workspace)
        .await?
        .into_iter()
        .filter_map(|t| {
            let memory_count = *counts.get(&t.id)?;
            Some(MemoryTopic {
                id: t.id,
                label: t.label,
                terms: t.terms,
                memory_count,
            })
        })
        .collect();
    topics.sort_by(|a, b| {
        b.memory_count
            .cmp(&a.memory_count)
            .then_with(|| a.label.cmp(&b.label))
    });
    Ok(topics)
}
