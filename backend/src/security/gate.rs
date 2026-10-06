//! Ending access tokens before they expire.
//!
//! An access token is valid on its own for a few minutes, so suspending an
//! account or changing its platform role would otherwise only take hold at
//! the next refresh. Each token carries the account's session epoch; raising
//! the epoch in the database and here refuses every token issued before.
//! Only accounts whose epoch was raised within a token lifetime are kept:
//! older tokens have expired by themselves.

use std::time::{Duration, Instant};

use dashmap::DashMap;
use sqlx::PgPool;
use uuid::Uuid;

/// Extra time an account stays watched, over the access token lifetime.
const MARGIN: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy)]
struct Floor {
    epoch: i32,
    since: Instant,
}

/// The lowest session epoch still admitted, per recently changed account.
#[derive(Debug, Default)]
pub struct SessionGate {
    floors: DashMap<Uuid, Floor>,
}

impl SessionGate {
    /// Whether a token of `user` carrying `epoch` still works.
    pub fn admits(&self, user: Uuid, epoch: i32) -> bool {
        self.floors
            .get(&user)
            .is_none_or(|floor| epoch >= floor.epoch)
    }

    /// Refuses the tokens of `user` issued before `epoch`.
    pub fn raise(&self, user: Uuid, epoch: i32) {
        let now = Instant::now();
        self.floors
            .entry(user)
            .and_modify(|floor| {
                if epoch > floor.epoch {
                    *floor = Floor { epoch, since: now };
                }
            })
            .or_insert(Floor { epoch, since: now });
    }

    /// Takes over what other servers changed and forgets the accounts whose
    /// old tokens have expired. Called every few seconds.
    pub async fn sync(&self, db: &PgPool, access_ttl: Duration) -> Result<(), sqlx::Error> {
        let window = access_ttl + MARGIN;
        let rows: Vec<(Uuid, i32)> = sqlx::query_as(
            "SELECT id, session_epoch FROM users
             WHERE session_epoch_at > now() - make_interval(secs => $1)",
        )
        .bind(window.as_secs_f64())
        .fetch_all(db)
        .await?;
        for (user, epoch) in rows {
            self.raise(user, epoch);
        }
        // Twice the window: an entry the query above still returns is never dropped.
        self.floors
            .retain(|_, floor| floor.since.elapsed() < window * 2);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_tokens_older_than_the_raised_epoch() {
        let gate = SessionGate::default();
        let (user, other) = (Uuid::now_v7(), Uuid::now_v7());
        assert!(gate.admits(user, 0), "nothing was raised");
        gate.raise(user, 2);
        assert!(!gate.admits(user, 1));
        assert!(gate.admits(user, 2) && gate.admits(user, 3));
        assert!(gate.admits(other, 0), "other accounts are untouched");
        gate.raise(user, 1);
        assert!(!gate.admits(user, 1), "the floor never goes down");
    }
}
