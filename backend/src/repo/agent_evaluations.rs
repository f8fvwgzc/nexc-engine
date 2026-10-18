//! The competency score of an agent made from a role template
//! (`agent_evaluations`): written once when the agent is made, read on its card.

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use crate::domain::agent_template::AgentEvaluation;

impl FromRow<'_, PgRow> for AgentEvaluation {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let questions: serde_json::Value = row.try_get("questions")?;
        Ok(AgentEvaluation {
            template: row.try_get("template")?,
            score: row.try_get("score")?,
            total: row.try_get("total")?,
            model: row.try_get("model")?,
            questions: serde_json::from_value(questions).unwrap_or_default(),
            evaluated_at: row.try_get("evaluated_at")?,
        })
    }
}

/// Keeps the evaluation of `agent_id` (replacing an earlier one).
pub async fn upsert(
    db: impl PgExecutor<'_>,
    agent_id: Uuid,
    evaluation: &AgentEvaluation,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO agent_evaluations (agent_id, template, score, total, model, questions, evaluated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (agent_id) DO UPDATE SET template = EXCLUDED.template, score = EXCLUDED.score,
             total = EXCLUDED.total, model = EXCLUDED.model, questions = EXCLUDED.questions,
             evaluated_at = EXCLUDED.evaluated_at",
    )
    .bind(agent_id)
    .bind(&evaluation.template)
    .bind(evaluation.score)
    .bind(evaluation.total)
    .bind(&evaluation.model)
    .bind(serde_json::to_value(&evaluation.questions).expect("questions serialise"))
    .bind(evaluation.evaluated_at)
    .execute(db)
    .await?;
    Ok(())
}

/// The evaluation of `agent_id`, if it was made from a template.
pub async fn find(
    db: impl PgExecutor<'_>,
    agent_id: Uuid,
) -> Result<Option<AgentEvaluation>, sqlx::Error> {
    sqlx::query_as(
        "SELECT template, score, total, model, questions, evaluated_at
         FROM agent_evaluations WHERE agent_id = $1",
    )
    .bind(agent_id)
    .fetch_optional(db)
    .await
}
