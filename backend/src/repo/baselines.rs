//! Behaviour baselines (`agent_baselines`): one row per agent, replaced as runs finish.

use sqlx::PgExecutor;
use uuid::Uuid;

use crate::domain::risk::Baseline;

/// What the agent usually does, if any run was learned from yet.
pub async fn find(
    db: impl PgExecutor<'_>,
    agent_id: Uuid,
) -> Result<Option<Baseline>, sqlx::Error> {
    let row: Option<(i32, serde_json::Value, serde_json::Value)> = sqlx::query_as(
        "SELECT runs, features, signatures FROM agent_baselines WHERE agent_id = $1",
    )
    .bind(agent_id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(runs, features, signatures)| {
        // `features` carries the moments and the hosts and tools seen; `signatures` the MinHashes.
        let mut baseline: Baseline = serde_json::from_value(features).unwrap_or_default();
        baseline.runs = runs;
        baseline.signatures = serde_json::from_value::<Vec<Vec<String>>>(signatures)
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.iter().filter_map(|v| v.parse().ok()).collect())
            .collect();
        baseline
    }))
}

/// Replaces the agent's baseline.
pub async fn upsert(
    db: impl PgExecutor<'_>,
    agent_id: Uuid,
    baseline: &Baseline,
) -> Result<(), sqlx::Error> {
    // u64 does not survive jsonb (it reformats large numbers), so signatures travel as text.
    let signatures: Vec<Vec<String>> = baseline
        .signatures
        .iter()
        .map(|s| s.iter().map(|v| v.to_string()).collect())
        .collect();
    let mut features = serde_json::to_value(baseline).expect("a baseline serialises");
    if let Some(map) = features.as_object_mut() {
        map.remove("signatures");
        map.remove("runs");
    }
    sqlx::query(
        "INSERT INTO agent_baselines (agent_id, runs, features, signatures, updated_at)
         VALUES ($1, $2, $3, $4, now())
         ON CONFLICT (agent_id) DO UPDATE
            SET runs = EXCLUDED.runs, features = EXCLUDED.features, signatures = EXCLUDED.signatures,
                updated_at = now()",
    )
    .bind(agent_id)
    .bind(baseline.runs)
    .bind(features)
    .bind(serde_json::to_value(signatures).expect("signatures serialise"))
    .execute(db)
    .await?;
    Ok(())
}
