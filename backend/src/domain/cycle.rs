//! Cycles: the time boxes a team works in.

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::string_enum;

pub const CYCLE_NAME_MAX: usize = 60;
pub const CYCLE_MAX_DAYS: i64 = 90;
pub const CYCLES_MAX: i64 = 500;

string_enum!(
    /// Where a cycle is relative to today.
    CycleStatus {
        Upcoming => "upcoming",
        Active => "active",
        Completed => "completed",
    }
);

impl CycleStatus {
    /// The status of a cycle running from `starts_on` to `ends_on`, both included.
    pub fn on(today: NaiveDate, starts_on: NaiveDate, ends_on: NaiveDate) -> Self {
        if today < starts_on {
            CycleStatus::Upcoming
        } else if today > ends_on {
            CycleStatus::Completed
        } else {
            CycleStatus::Active
        }
    }
}

/// A cycle of a team, with how far its issues are.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Cycle {
    pub id: Uuid,
    pub team_id: Uuid,
    /// Sequential within the team.
    pub number: i32,
    /// Optional; a cycle without a name is shown as "Cycle {number}".
    pub name: String,
    /// First day, included.
    pub starts_on: NaiveDate,
    /// Last day, included.
    pub ends_on: NaiveDate,
    pub status: CycleStatus,
    pub issue_count: i64,
    pub closed_count: i64,
    pub created_at: DateTime<Utc>,
}

/// The cycle an issue is in, as much as the issue shows.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct CycleRef {
    pub id: Uuid,
    pub number: i32,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cycle_is_active_on_its_first_and_last_day() {
        let day = |d| NaiveDate::from_ymd_opt(2026, 10, d).unwrap();
        let status = |today| CycleStatus::on(day(today), day(5), day(18));
        assert_eq!(status(4), CycleStatus::Upcoming);
        assert_eq!(status(5), CycleStatus::Active);
        assert_eq!(status(18), CycleStatus::Active);
        assert_eq!(status(19), CycleStatus::Completed);
    }
}
