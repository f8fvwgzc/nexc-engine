//! What happened to an account's access, as its holder reads it on their
//! profile: a record of sign-ins and of every change to how they get in.

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::string_enum;

/// How long an entry is kept.
pub const ACTIVITY_KEPT_DAYS: i32 = 180;
/// Most entries returned at once.
pub const ACTIVITY_PAGE_MAX: i64 = 100;

string_enum!(
    /// What an entry of an account's security activity records.
    AccountEventKind {
        Registered => "registered",
        SignedIn => "signed_in",
        SignInFailed => "sign_in_failed",
        PasswordChanged => "password_changed",
        /// A new password was set with a reset link.
        PasswordReset => "password_reset",
        /// A platform administrator created a reset link for the account.
        ResetLinkIssued => "reset_link_issued",
        /// The holder signed out everywhere.
        SessionsEnded => "sessions_ended",
        Suspended => "suspended",
        Reactivated => "reactivated",
        /// The account's platform role changed.
        RoleChanged => "role_changed",
    }
);

/// One entry of an account's security activity.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AccountEvent {
    pub id: Uuid,
    pub kind: AccountEventKind,
    /// The address the request came from; `null` for what an administrator
    /// did and when the address is not known.
    #[schema(required = true)]
    pub ip: Option<String>,
    /// What else there is to say: who did it, what changed.
    pub detail: String,
    pub created_at: DateTime<Utc>,
}
