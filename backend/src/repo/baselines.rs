//! Behaviour baselines: one row per agent and scope, replaced as runs
//! finish. An agent's runs learn into `agent_baselines`; an external
//! agent's decision sessions into `agent_session_baselines`.

use sqlx::PgExecutor;
use uuid::Uuid;

use crate::domain::risk::{Baseline, Scope};

const FIND_RUNS: &str =
    "SELECT runs, features, signatures FROM agent_baselines WHERE agent_id = $1";
const FIND_SESSIONS: &str =
    "SELECT runs, features, signatures FROM agent_session_baselines WHERE agent_id = $1";
const LOCK_RUNS: &str =
    "SELECT runs, features, signatures FROM agent_baselines WHERE agent_id = $1 FOR UPDATE";
const LOCK_SESSIONS: &str =
    "SELECT runs, features, signatures FROM agent_session_baselines WHERE agent_id = $1 FOR UPDATE";
const UPSERT_RUNS: &str =
    "INSERT INTO agent_baselines (agent_id, runs, features, signatures, updated_at)
     VALUES ($1, $2, $3, $4, now())
     ON CONFLICT (agent_id) DO UPDATE
        SET runs = EXCLUDED.runs, features = EXCLUDED.features, signatures = EXCLUDED.signatures,
            updated_at = now()";
const UPSERT_SESSIONS: &str =
    "INSERT INTO agent_session_baselines (agent_id, runs, features, signatures, updated_at)
     VALUES ($1, $2, $3, $4, now())
     ON CONFLICT (agent_id) DO UPDATE
        SET runs = EXCLUDED.runs, features = EXCLUDED.features, signatures = EXCLUDED.signatures,
            updated_at = now()";

async fn read(
    db: impl PgExecutor<'_>,
    query: &'static str,
    agent_id: Uuid,
) -> Result<Option<Baseline>, sqlx::Error> {
    let row: Option<(i32, serde_json::Value, serde_json::Value)> = sqlx::query_as(query)
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

/// What the agent usually does in `scope`, if anything was learned from yet.
pub async fn find(
    db: impl PgExecutor<'_>,
    agent_id: Uuid,
    scope: Scope,
) -> Result<Option<Baseline>, sqlx::Error> {
    let query = match scope {
        Scope::Runs => FIND_RUNS,
        Scope::Sessions => FIND_SESSIONS,
    };
    read(db, query, agent_id).await
}

/// [`find`], locking the row until the transaction ends: two runs of one
/// agent finishing at once learn one after the other, and neither is lost.
pub async fn find_for_update(
    db: impl PgExecutor<'_>,
    agent_id: Uuid,
    scope: Scope,
) -> Result<Option<Baseline>, sqlx::Error> {
    let query = match scope {
        Scope::Runs => LOCK_RUNS,
        Scope::Sessions => LOCK_SESSIONS,
    };
    read(db, query, agent_id).await
}

/// Replaces the agent's baseline in `scope`.
pub async fn upsert(
    db: impl PgExecutor<'_>,
    agent_id: Uuid,
    scope: Scope,
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
    let query = match scope {
        Scope::Runs => UPSERT_RUNS,
        Scope::Sessions => UPSERT_SESSIONS,
    };
    sqlx::query(query)
        .bind(agent_id)
        .bind(baseline.runs)
        .bind(features)
        .bind(serde_json::to_value(signatures).expect("signatures serialise"))
        .execute(db)
        .await?;
    Ok(())
}
