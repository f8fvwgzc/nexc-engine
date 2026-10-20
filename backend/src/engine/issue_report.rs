//! A finished run reports back to the issues its graph executes.
//!
//! An issue hands its work to a graph (`POST /issues/{id}/graph`); when a run
//! of that graph ends, the issue's timeline gets the result, a successful run
//! moves the issue to review, and the people on the issue find it in their
//! inbox. It happens in the transaction that finishes the run, which is
//! fenced on the run's lease, so a replica that lost the run cannot report it
//! a second time.

use sqlx::PgConnection;
use uuid::Uuid;

use crate::domain::issue::run_report;
use crate::repo;
use crate::repo::runs::RunRow;

/// Reports the finished run `row` to the issues its graph executes; returns
/// the issues it reported to (none for a graph no issue hands work to).
pub async fn report(db: &mut PgConnection, row: &RunRow) -> Result<Vec<Uuid>, sqlx::Error> {
    if !repo::issues::any_of_graph(&mut *db, row.graph_id).await? {
        return Ok(Vec::new());
    }
    let (results, failures) = repo::issues::run_outcomes(&mut *db, row.id).await?;
    let body = run_report(&results, &failures);
    repo::issues::report_run(
        &mut *db,
        &repo::issues::RunReport {
            run_id: row.id,
            graph_id: row.graph_id,
            owner_id: row.owner_id,
            status: row.status,
            body: &body,
        },
    )
    .await
}
