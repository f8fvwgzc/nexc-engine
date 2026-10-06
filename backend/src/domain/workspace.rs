//! Workspaces (organisations), teams and who may do what in them.
//!
//! The model follows Linear. A workspace has members with one of four roles;
//! work happens in teams, which are public to the workspace or private to
//! their members. Authorization is decided here, by pure functions over the
//! caller's roles, so that the rules can be read and tested in one place.

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::string_enum;

/// Maximum workspace and team name length.
pub const NAME_MAX: usize = 80;
/// Maximum team description length.
pub const DESCRIPTION_MAX: usize = 500;
/// Maximum team key length (`ENG`).
pub const TEAM_KEY_MAX: usize = 7;
/// Maximum slug length.
pub const SLUG_MAX: usize = 48;

string_enum!(
    /// Role of a user in a workspace, from most to least privileged.
    WorkspaceRole {
        /// Full control, including deleting the workspace and naming other owners.
        Owner => "owner",
        /// Manages members, teams and workspace settings.
        Admin => "admin",
        /// Works in every public team and can create teams.
        Member => "member",
        /// Sees only the teams they were added to.
        Guest => "guest",
    }
);

string_enum!(
    /// Role of a user in a team.
    TeamRole {
        /// Manages the team's settings and members.
        Owner => "owner",
        Member => "member",
    }
);

impl WorkspaceRole {
    /// Privilege rank; higher may do more.
    fn rank(self) -> u8 {
        match self {
            WorkspaceRole::Owner => 3,
            WorkspaceRole::Admin => 2,
            WorkspaceRole::Member => 1,
            WorkspaceRole::Guest => 0,
        }
    }

    /// True for owners and admins.
    pub fn is_admin(self) -> bool {
        self.rank() >= WorkspaceRole::Admin.rank()
    }

    /// True for everyone but guests.
    pub fn is_member(self) -> bool {
        self.rank() >= WorkspaceRole::Member.rank()
    }
}

/// Something a workspace member may try to do to the workspace itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceAction {
    /// List members and pending invites.
    ViewMembers,
    /// Rename the workspace or change its settings.
    UpdateSettings,
    /// Invite, remove or change the role of non-owners.
    ManageMembers,
    /// Create a team.
    CreateTeam,
    /// Delete the workspace.
    Delete,
}

/// Whether `role` may perform `action` on its workspace.
pub fn can(role: WorkspaceRole, action: WorkspaceAction) -> bool {
    match action {
        WorkspaceAction::ViewMembers | WorkspaceAction::CreateTeam => role.is_member(),
        WorkspaceAction::UpdateSettings | WorkspaceAction::ManageMembers => role.is_admin(),
        WorkspaceAction::Delete => role == WorkspaceRole::Owner,
    }
}

/// Whether `actor` may give `target` (currently `current`, or `None` for a
/// new member) the role `new`. Admins manage everyone below owner; only
/// owners touch owners or hand out ownership.
pub fn can_assign_role(
    actor: WorkspaceRole,
    current: Option<WorkspaceRole>,
    new: WorkspaceRole,
) -> bool {
    let touches_owner = current == Some(WorkspaceRole::Owner) || new == WorkspaceRole::Owner;
    if touches_owner {
        actor == WorkspaceRole::Owner
    } else {
        actor.is_admin()
    }
}

/// Whether `actor` may remove a member whose role is `target`.
pub fn can_remove_member(actor: WorkspaceRole, target: WorkspaceRole) -> bool {
    can_assign_role(actor, Some(target), WorkspaceRole::Guest)
}

/// The caller's standing towards one team.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeamAccess {
    pub workspace_role: WorkspaceRole,
    pub team_role: Option<TeamRole>,
    pub private: bool,
}

impl TeamAccess {
    /// The team exists as far as the caller is concerned: its members always
    /// see it; other workspace members see public teams; workspace admins see
    /// every team so that none can become unmanageable.
    pub fn can_view(self) -> bool {
        self.team_role.is_some()
            || self.workspace_role.is_admin()
            || (!self.private && self.workspace_role.is_member())
    }

    /// Work in the team (its issues, projects, graphs): team members only.
    pub fn can_contribute(self) -> bool {
        self.team_role.is_some()
    }

    /// File and edit the team's issues: its members, plus everyone in the
    /// workspace who can see the team and is more than a guest (as in Linear,
    /// where anyone may file an issue with a public team).
    pub fn can_file_issues(self) -> bool {
        self.team_role.is_some() || (self.can_view() && self.workspace_role.is_member())
    }

    /// Join without an invitation: public teams, workspace members only.
    pub fn can_join(self) -> bool {
        self.team_role.is_none() && !self.private && self.workspace_role.is_member()
    }

    /// Change the team's settings and membership, or delete it.
    pub fn can_manage(self) -> bool {
        self.team_role == Some(TeamRole::Owner) || self.workspace_role.is_admin()
    }
}

/// A workspace as seen by one of its members.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    /// URL-safe unique identifier.
    pub slug: String,
    /// The caller's role.
    pub role: WorkspaceRole,
    pub member_count: i64,
    pub created_at: DateTime<Utc>,
}

/// A member of a workspace.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WorkspaceMember {
    pub user_id: Uuid,
    pub name: String,
    pub email: String,
    pub role: WorkspaceRole,
    pub joined_at: DateTime<Utc>,
}

/// An invitation that waits for its recipient to sign up.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WorkspaceInvite {
    pub id: Uuid,
    pub email: String,
    pub role: WorkspaceRole,
    pub created_at: DateTime<Utc>,
}

/// A team as seen by the caller.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Team {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    /// Short identifier that prefixes the team's issues, e.g. `ENG`.
    pub key: String,
    pub description: String,
    pub private: bool,
    /// The caller's role in the team, if they are a member.
    #[schema(required = true)]
    pub role: Option<TeamRole>,
    pub member_count: i64,
    pub created_at: DateTime<Utc>,
}

/// A member of a team.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct TeamMember {
    pub user_id: Uuid,
    pub name: String,
    pub email: String,
    pub role: TeamRole,
    pub joined_at: DateTime<Utc>,
}

/// URL slug of a name: lowercase letters and digits joined by single dashes.
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for c in name.trim().chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-')
        .chars()
        .take(SLUG_MAX - 9)
        .collect()
}

/// Team key suggested for a name: the initials of its words, or its first
/// letters when it is a single word (`Core Platform` -> `CP`, `Design` -> `DES`).
pub fn suggest_team_key(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.chars().next().is_some_and(|c| c.is_ascii_alphabetic()))
        .collect();
    let key: String = match words.as_slice() {
        [single] => single.chars().take(3).collect(),
        many => many.iter().filter_map(|w| w.chars().next()).collect(),
    };
    key.to_ascii_uppercase()
        .chars()
        .take(TEAM_KEY_MAX)
        .collect()
}

/// Whether `key` is a valid team key: an uppercase letter followed by up to
/// six uppercase letters or digits.
pub fn is_valid_team_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && key.len() <= TEAM_KEY_MAX
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::WorkspaceAction::*;
    use super::WorkspaceRole::*;
    use super::*;

    #[test]
    fn workspace_permissions_follow_the_role_ladder() {
        for (action, allowed) in [
            (ViewMembers, vec![Owner, Admin, Member]),
            (CreateTeam, vec![Owner, Admin, Member]),
            (UpdateSettings, vec![Owner, Admin]),
            (ManageMembers, vec![Owner, Admin]),
            (Delete, vec![Owner]),
        ] {
            for role in [Owner, Admin, Member, Guest] {
                assert_eq!(
                    can(role, action),
                    allowed.contains(&role),
                    "{role} {action:?}"
                );
            }
        }
    }

    #[test]
    fn only_owners_touch_owners() {
        assert!(can_assign_role(Owner, Some(Admin), Owner));
        assert!(can_assign_role(Owner, Some(Owner), Member));
        assert!(!can_assign_role(Admin, Some(Member), Owner));
        assert!(!can_assign_role(Admin, Some(Owner), Admin));
        assert!(can_assign_role(Admin, Some(Member), Admin));
        assert!(can_assign_role(Admin, None, Guest));
        assert!(!can_assign_role(Member, None, Guest));
        assert!(can_remove_member(Admin, Member));
        assert!(!can_remove_member(Admin, Owner));
        assert!(!can_remove_member(Member, Guest));
    }

    #[test]
    fn team_visibility_and_management() {
        let access = |workspace_role, team_role, private| TeamAccess {
            workspace_role,
            team_role,
            private,
        };
        // Public team: members see and may join it, guests do not.
        assert!(access(Member, None, false).can_view());
        assert!(access(Member, None, false).can_join());
        assert!(!access(Member, None, false).can_contribute());
        assert!(!access(Guest, None, false).can_view());
        assert!(!access(Guest, None, false).can_join());
        // A guest added to a team works in it like anyone else.
        assert!(access(Guest, Some(TeamRole::Member), true).can_view());
        assert!(access(Guest, Some(TeamRole::Member), true).can_contribute());
        assert!(!access(Guest, Some(TeamRole::Member), true).can_manage());
        // Private team: hidden from other members, never joinable, but
        // workspace admins can still manage it.
        assert!(!access(Member, None, true).can_view());
        assert!(!access(Member, None, true).can_join());
        assert!(access(Admin, None, true).can_view());
        assert!(access(Admin, None, true).can_manage());
        assert!(!access(Admin, None, true).can_contribute());
        assert!(access(Member, Some(TeamRole::Owner), true).can_manage());
        // Issues: anyone who sees the team and is more than a guest; guests only in their teams.
        assert!(access(Member, None, false).can_file_issues());
        assert!(!access(Member, None, true).can_file_issues());
        assert!(access(Admin, None, true).can_file_issues());
        assert!(!access(Guest, None, false).can_file_issues());
        assert!(access(Guest, Some(TeamRole::Member), true).can_file_issues());
        assert!(!access(Member, Some(TeamRole::Member), false).can_manage());
    }

    #[test]
    fn slugs_and_team_keys() {
        assert_eq!(slugify("  Acme, Inc. "), "acme-inc");
        assert_eq!(slugify("Ünïcode"), "n-code");
        assert_eq!(suggest_team_key("Core Platform"), "CP");
        assert_eq!(suggest_team_key("Design"), "DES");
        assert_eq!(suggest_team_key("3D & ml ops"), "MO");
        assert!(is_valid_team_key("ENG"));
        assert!(is_valid_team_key("A1"));
        assert!(!is_valid_team_key("eng"));
        assert!(!is_valid_team_key("1A"));
        assert!(!is_valid_team_key("TOOLONGKEY"));
        assert!(!is_valid_team_key(""));
    }
}
