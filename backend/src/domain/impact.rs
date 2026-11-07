//! Versions and change impact of issues and projects.
//!
//! Every issue and project carries a semantic version that the change-impact
//! engine (the `assessment-engine` service, reached over gRPC with
//! [`assessment_grpc`]; its types are [`assessment_core`]'s) moves on each change:
//! MAJOR when what others planned against it broke, MINOR for compatible
//! additions, PATCH for a cosmetic edit. An issue pins the version of what it
//! was planned against (its parent issue, or its project when it has none), so
//! a MAJOR bump there makes its plan incompatible.
//!
//! This module is the mapping between nexc's rows and the engine's
//! [`ac::model::Entity`] — the table in the engine's README, "Wiring it into
//! nexc-engine" — and what an assessment says as the API returns it, with the
//! re-plan it proposed. It does no I/O; `repo::impact` reads and writes,
//! `engine::impact` calls the engine and `engine::replan` drafts the re-plans.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use assessment_core as ac;
use assessment_core::model::{BasisPin, Entity, FieldValue, field};
use assessment_core::semver::{Compatibility, SemVer};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::issue::{IssuePerson, ProjectStatus, StateCategory};
use super::plan::PlanStatus;
use super::string_enum;

/// Most impacts an assessment keeps, most affected first; the engine reports how many there were.
pub const IMPACTS_MAX: usize = 50;
/// Most assessments `GET …/assessments` returns, newest first.
pub const ASSESSMENTS_MAX: i64 = 50;
/// How many earlier assessments of an item the engine reads as its change history (volatility
/// and how unusual a change is).
pub const HISTORY_MAX: i64 = 50;

string_enum!(
    /// What was assessed.
    ImpactEntityKind {
        Issue => "issue",
        Project => "project",
    }
);

string_enum!(
    /// What happened to the item that made the engine assess it.
    AssessmentCause {
        Created => "created",
        Updated => "updated",
        Deleted => "deleted",
        /// A comment: it is scanned for blockers, scope changes, risks and slips, and moves no
        /// version.
        Comment => "comment",
    }
);

string_enum!(
    /// How much a version moved.
    VersionBump {
        /// Nothing versioned changed (a comment).
        None => "none",
        /// Cosmetic: nothing anyone planned against moved.
        Patch => "patch",
        /// Compatible additions or moderate changes.
        Minor => "minor",
        /// Breaking: plans made against the previous version need revisiting.
        Major => "major",
        /// The item was created.
        Initial => "initial",
    }
);

string_enum!(
    /// How much attention a change needs, by score band.
    ImpactLevel {
        None => "none",
        Low => "low",
        Medium => "medium",
        High => "high",
        Critical => "critical",
    }
);

string_enum!(
    /// What to do about a change as a whole, from least to most.
    Recommendation {
        /// Nothing anyone planned moves.
        NoAction => "no_action",
        /// The owners of the affected items should know.
        Notify => "notify",
        /// The owners of the affected items should review them against the change.
        Review => "review",
        /// Re-plan the subtree under `replan_root`.
        PartialReplan => "partial_replan",
        /// Re-plan the whole effort.
        FullReplan => "full_replan",
    }
);

string_enum!(
    /// What to do about one affected item.
    ImpactAction {
        None => "none",
        Notify => "notify",
        Review => "review",
        /// Text it depends on gained requirements.
        UpdateScope => "update_scope",
        /// Effort under it moved.
        ReEstimate => "re_estimate",
        /// A date it depends on moved into conflict.
        Reschedule => "reschedule",
        /// Its parent was reopened: re-verify its completed work.
        Reverify => "reverify",
        /// Text it was based on was removed: it may no longer be needed.
        ObsoleteCandidate => "obsolete_candidate",
        /// Its parent was canceled or deleted: cancel it or move it.
        CancelOrRehome => "cancel_or_rehome",
        /// Priorities around it changed.
        Reprioritize => "reprioritize",
        /// It changed so much that its own plan needs redoing.
        Replan => "replan",
        /// It is part of a plan conflict (a cycle, a closed blocker).
        ResolveConflict => "resolve_conflict",
    }
);

string_enum!(
    /// Whether an item's plan still matches the version of what it was planned against.
    BasisStatus {
        /// It pins nothing that changed.
        NotPinned => "not_pinned",
        /// The pinned version is current.
        Current => "current",
        /// Only patch-level changes since the pin.
        Compatible => "compatible",
        /// Compatible additions since the pin: worth a look.
        ReviewSuggested => "review_suggested",
        /// A breaking change since the pin: the plan is out of date.
        Incompatible => "incompatible",
        /// What it pinned was deleted.
        Removed => "removed",
    }
);

string_enum!(
    /// Where a re-plan proposal stands.
    ReplanStatus {
        /// Waiting for changes to the same item to settle (or for a plan still being drafted on
        /// its graph) before the planner starts.
        Pending => "pending",
        /// A backend is starting the planner.
        Starting => "starting",
        /// The planner was started: `plan_status` says whether the plan is ready to review.
        Started => "started",
        /// A newer change asked for a re-plan of the same item and replaced this one.
        Superseded => "superseded",
        /// No plan could be drafted; `error` says why.
        Skipped => "skipped",
    }
);

impl From<ac::assessment::ImpactLevel> for ImpactLevel {
    fn from(level: ac::assessment::ImpactLevel) -> Self {
        use ac::assessment::ImpactLevel as L;
        match level {
            L::None => ImpactLevel::None,
            L::Low => ImpactLevel::Low,
            L::Medium => ImpactLevel::Medium,
            L::High => ImpactLevel::High,
            L::Critical => ImpactLevel::Critical,
        }
    }
}

impl From<ac::assessment::Recommendation> for Recommendation {
    fn from(r: ac::assessment::Recommendation) -> Self {
        use ac::assessment::Recommendation as R;
        match r {
            R::NoAction => Recommendation::NoAction,
            R::Notify => Recommendation::Notify,
            R::Review => Recommendation::Review,
            R::PartialReplan => Recommendation::PartialReplan,
            R::FullReplan => Recommendation::FullReplan,
        }
    }
}

impl From<ac::assessment::Action> for ImpactAction {
    fn from(a: ac::assessment::Action) -> Self {
        use ac::assessment::Action as A;
        match a {
            A::None => ImpactAction::None,
            A::Notify => ImpactAction::Notify,
            A::Review => ImpactAction::Review,
            A::UpdateScope => ImpactAction::UpdateScope,
            A::ReEstimate => ImpactAction::ReEstimate,
            A::Reschedule => ImpactAction::Reschedule,
            A::Reverify => ImpactAction::Reverify,
            A::ObsoleteCandidate => ImpactAction::ObsoleteCandidate,
            A::CancelOrRehome => ImpactAction::CancelOrRehome,
            A::Reprioritize => ImpactAction::Reprioritize,
            A::Replan => ImpactAction::Replan,
            A::ResolveConflict => ImpactAction::ResolveConflict,
        }
    }
}

impl From<ac::assessment::BasisStatus> for BasisStatus {
    fn from(s: ac::assessment::BasisStatus) -> Self {
        use ac::assessment::BasisStatus as B;
        match s {
            B::NotPinned => BasisStatus::NotPinned,
            B::Current => BasisStatus::Current,
            B::Compatible => BasisStatus::Compatible,
            B::ReviewSuggested => BasisStatus::ReviewSuggested,
            B::Incompatible => BasisStatus::Incompatible,
            B::Removed => BasisStatus::Removed,
        }
    }
}

impl From<ac::semver::VersionBump> for VersionBump {
    fn from(b: ac::semver::VersionBump) -> Self {
        use ac::semver::VersionBump as V;
        match b {
            V::None => VersionBump::None,
            V::Patch => VersionBump::Patch,
            V::Minor => VersionBump::Minor,
            V::Major => VersionBump::Major,
            V::Initial => VersionBump::Initial,
        }
    }
}

impl BasisStatus {
    /// Where a plan made against `pinned` stands now that the item it pins is at `current`.
    pub fn of(pinned: SemVer, current: SemVer) -> Self {
        match pinned.compatibility(current) {
            Compatibility::Identical => BasisStatus::Current,
            Compatibility::Patch => BasisStatus::Compatible,
            Compatibility::Additive => BasisStatus::ReviewSuggested,
            Compatibility::Breaking => BasisStatus::Incompatible,
        }
    }
}

// ---------- nexc → engine ----------

/// Linear's priority (`0` none, `1` urgent, `2` high, `3` medium, `4` low) as
/// the engine's rank, where higher is more important: `[0, 4, 3, 2, 1][p]` on
/// a scale of 4. `None` for a value outside `0..=4`.
pub fn priority_rank(priority: i16) -> Option<u32> {
    const RANKS: [u32; 5] = [0, 4, 3, 2, 1];
    usize::try_from(priority)
        .ok()
        .and_then(|p| RANKS.get(p).copied())
}

/// A workflow state's category as the engine's.
pub fn status_category(category: StateCategory) -> ac::model::StatusCategory {
    use ac::model::StatusCategory as S;
    match category {
        StateCategory::Backlog => S::Backlog,
        StateCategory::Unstarted => S::Unstarted,
        StateCategory::Started => S::Started,
        StateCategory::Completed => S::Completed,
        StateCategory::Canceled => S::Canceled,
    }
}

/// A project's status as the engine's workflow category: a paused project is
/// back in the backlog, work is still expected of it.
pub fn project_status_category(status: ProjectStatus) -> ac::model::StatusCategory {
    use ac::model::StatusCategory as S;
    match status {
        ProjectStatus::Planned => S::Unstarted,
        ProjectStatus::Started => S::Started,
        ProjectStatus::Paused => S::Backlog,
        ProjectStatus::Completed => S::Completed,
        ProjectStatus::Canceled => S::Canceled,
    }
}

/// A due day as the engine's timestamp: 00:00 UTC that day.
fn day(date: NaiveDate) -> FieldValue {
    FieldValue::Timestamp(date.and_time(chrono::NaiveTime::MIN).and_utc())
}

/// An issue as the engine sees it, read with everything the mapping needs.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueNode {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub team_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub cycle_id: Option<Uuid>,
    pub assignee_id: Option<Uuid>,
    pub creator_id: Option<Uuid>,
    pub title: String,
    pub description: String,
    pub state_name: String,
    pub state_category: StateCategory,
    pub priority: i16,
    /// Label names.
    pub labels: Vec<String>,
    pub due_date: Option<NaiveDate>,
    pub updated_at: DateTime<Utc>,
    pub version: SemVer,
    /// The pin stored on the row: what it was planned against and at which
    /// version; `None` pins [`SemVer::INITIAL`] of whatever it is part of.
    pub stored_basis: Option<(Uuid, SemVer)>,
}

impl IssueNode {
    /// What the issue is part of: its parent issue, else its project.
    pub fn basis_target(&self) -> Option<(ImpactEntityKind, Uuid)> {
        match (self.parent_id, self.project_id) {
            (Some(parent), _) => Some((ImpactEntityKind::Issue, parent)),
            (None, Some(project)) => Some((ImpactEntityKind::Project, project)),
            (None, None) => None,
        }
    }

    /// The version of what it is part of that the issue was planned against:
    /// the stored pin when it is on that item, 1.0.0 when nothing was stored
    /// (rows from before versions, still on their first parent), none when the
    /// stored pin is on something it is no longer part of.
    pub fn pinned(&self) -> Option<(Uuid, SemVer)> {
        let (_, target) = self.basis_target()?;
        match self.stored_basis {
            None => Some((target, SemVer::INITIAL)),
            Some((id, version)) if id == target => Some((id, version)),
            Some(_) => None,
        }
    }

    /// The engine's kind: a sub-issue when it is part of another issue.
    pub fn kind(&self) -> &'static str {
        if self.parent_id.is_some() {
            "sub_issue"
        } else {
            "issue"
        }
    }

    /// The issue as the engine's entity, mapped as the engine's README says.
    pub fn entity(&self) -> Entity {
        let mut e = Entity::new(self.id.to_string(), self.kind())
            .with_version(self.version)
            .with_title(self.title.clone())
            .with_status(
                status_category(self.state_category),
                self.state_name.clone(),
            );
        e.parent_id = self.parent_id.or(self.project_id).map(|id| id.to_string());
        e.updated_at = Some(self.updated_at);
        let fields = &mut e.fields;
        // An absent field is null: an empty description or no labels is "none", so filling one in
        // reads as an addition rather than a rewrite of nothing.
        if !self.description.trim().is_empty() {
            fields.insert(
                field::DESCRIPTION.into(),
                FieldValue::Text(self.description.clone()),
            );
        }
        if let Some(rank) = priority_rank(self.priority) {
            fields.insert(field::PRIORITY.into(), FieldValue::ordinal(rank, 4));
        }
        insert_reference(fields, field::ASSIGNEE, self.assignee_id);
        insert_reference(fields, field::PROJECT, self.project_id);
        insert_reference(fields, field::CYCLE, self.cycle_id);
        insert_reference(fields, field::TEAM, Some(self.team_id));
        if let Some(due) = self.due_date {
            fields.insert(field::DUE_DATE.into(), day(due));
        }
        if !self.labels.is_empty() {
            fields.insert(
                field::LABELS.into(),
                FieldValue::set(self.labels.iter().cloned()),
            );
        }
        if let Some((target, version)) = self.pinned() {
            e.basis.push(BasisPin {
                entity_id: target.to_string(),
                version,
            });
        }
        e
    }
}

/// A project as the engine sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectNode {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub team_id: Option<Uuid>,
    pub name: String,
    pub description: String,
    pub status: ProjectStatus,
    pub lead_id: Option<Uuid>,
    pub target_date: Option<NaiveDate>,
    pub updated_at: DateTime<Utc>,
    pub version: SemVer,
}

impl ProjectNode {
    /// The project as the engine's entity: its name is the title, its lead the
    /// assignee and its target date the due date.
    pub fn entity(&self) -> Entity {
        let mut e = Entity::new(self.id.to_string(), "project")
            .with_version(self.version)
            .with_title(self.name.clone())
            .with_status(
                project_status_category(self.status),
                self.status.as_str().to_owned(),
            );
        e.updated_at = Some(self.updated_at);
        let fields = &mut e.fields;
        if !self.description.trim().is_empty() {
            fields.insert(
                field::DESCRIPTION.into(),
                FieldValue::Text(self.description.clone()),
            );
        }
        insert_reference(fields, field::ASSIGNEE, self.lead_id);
        insert_reference(fields, field::TEAM, self.team_id);
        if let Some(target) = self.target_date {
            fields.insert(field::DUE_DATE.into(), day(target));
        }
        e
    }
}

fn insert_reference(fields: &mut BTreeMap<String, FieldValue>, name: &str, id: Option<Uuid>) {
    if let Some(id) = id {
        fields.insert(name.into(), FieldValue::reference(id.to_string()));
    }
}

// ---------- engine → API ----------

/// One item a change affects (the changed item included, as `changed`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Impact {
    pub entity_kind: ImpactEntityKind,
    pub entity_id: Uuid,
    /// 1 = most affected.
    pub rank: u32,
    /// `[0, 1]`.
    pub score: f64,
    pub level: ImpactLevel,
    pub action: ImpactAction,
    /// How the impact reached it: `self`, `down` (from its parent), `up` (from a sub-issue),
    /// `sibling`, `dependency`, `upstream` or `related`.
    pub channel: String,
    /// Hops from the changed item (0 = the changed item).
    pub distance: u32,
    pub changed: bool,
    pub basis_status: BasisStatus,
    /// Why, in words.
    pub reasons: Vec<String>,
}

/// A plan conflict among the affected items (a sub-issue due after its parent,
/// active work under a canceled parent, …).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PlanViolation {
    /// `orphaned_work`, `schedule_conflict`, `premature_close`, `overdue`, ….
    pub kind: String,
    pub entities: Vec<Uuid>,
    /// `[0, 1]`.
    pub severity: f64,
    /// Whether this change introduced it.
    pub introduced: bool,
    pub message: String,
}

/// An assessed change to an issue or a project.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ChangeAssessment {
    pub id: Uuid,
    pub entity_kind: ImpactEntityKind,
    pub entity_id: Uuid,
    /// Who made the change; `null` for one nobody made and once the account is gone.
    #[schema(required = true)]
    pub actor: Option<IssuePerson>,
    pub cause: AssessmentCause,
    pub bump: VersionBump,
    #[schema(required = true)]
    pub previous_version: Option<String>,
    /// The item's version after the change; the previous one for a comment, none for a deletion.
    #[schema(required = true)]
    pub next_version: Option<String>,
    /// The re-plan score, `[0, 1]`.
    pub score: f64,
    pub level: ImpactLevel,
    pub recommendation: Recommendation,
    /// How much the engine trusts its answer, `[0, 1]`.
    pub confidence: f64,
    /// The smallest subtree to re-plan (an issue or a project); set for a re-plan.
    #[schema(required = true)]
    pub replan_root: Option<Uuid>,
    pub summary: String,
    /// The affected items the reader can see, most affected first.
    pub impacts: Vec<Impact>,
    pub violations: Vec<PlanViolation>,
    /// The re-plan of `replan_root` this assessment asked for, as nexc's planner drafts it;
    /// `null` when it asked for none (or for one of an item the reader cannot see).
    #[schema(required = true)]
    pub replan: Option<ReplanProposal>,
    pub created_at: DateTime<Utc>,
}

/// A re-plan the engine called for, drafted by the planner and never applied
/// on its own: once `plan_status` is `ready`, review the plan on its graph and
/// apply it there (`POST /graphs/{graph_id}/plans/{plan_id}/apply`).
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ReplanProposal {
    pub id: Uuid,
    pub status: ReplanStatus,
    /// What is re-planned: an issue (its graph) or a project.
    pub root_kind: ImpactEntityKind,
    pub root_id: Uuid,
    /// The graph the plan is drafted in, once started: the issue's own (given one if it had none).
    #[schema(required = true)]
    pub graph_id: Option<Uuid>,
    #[schema(required = true)]
    pub plan_id: Option<Uuid>,
    /// The drafted plan's own state: `streaming`, `ready` (to review and apply), `failed` or
    /// `applied`.
    #[schema(required = true)]
    pub plan_status: Option<PlanStatus>,
    /// Why no plan was drafted (`skipped`), or why the plan failed.
    #[schema(required = true)]
    pub error: Option<String>,
    /// The earliest the planner starts; further changes to the same item push it back.
    pub starts_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// What a change assessment stores, before it has an id and a time.
#[derive(Debug, Clone, PartialEq)]
pub struct NewAssessment {
    pub workspace_id: Uuid,
    pub team_id: Option<Uuid>,
    pub entity_kind: ImpactEntityKind,
    pub entity_id: Uuid,
    pub actor_id: Option<Uuid>,
    pub cause: AssessmentCause,
    pub bump: VersionBump,
    pub previous_version: Option<SemVer>,
    pub next_version: Option<SemVer>,
    pub raw_score: f64,
    pub score: f64,
    pub level: ImpactLevel,
    pub recommendation: Recommendation,
    pub confidence: f64,
    pub replan_root: Option<Uuid>,
    pub summary: String,
    pub impacts: Vec<Impact>,
    pub violations: Vec<PlanViolation>,
    /// The re-plan proposal that took up the re-plan it recommends, once stored.
    pub replan_id: Option<Uuid>,
}

impl NewAssessment {
    /// The engine's answer about the change to `entity_id`, as stored: the
    /// change's own bump and versions, the overall score and recommendation,
    /// at most [`IMPACTS_MAX`] impacts. Ids the engine reports that are not
    /// nexc's (none should be) are left out.
    pub fn from_engine(
        a: &ac::Assessment,
        entity_kind: ImpactEntityKind,
        entity_id: Uuid,
        cause: AssessmentCause,
    ) -> Self {
        let key = entity_id.to_string();
        let change = a.changes.iter().find(|c| c.entity_id == key);
        let impacts = a
            .impacts
            .iter()
            .filter_map(|i| {
                Some(Impact {
                    entity_kind: if i.kind.as_str() == "project" {
                        ImpactEntityKind::Project
                    } else {
                        ImpactEntityKind::Issue
                    },
                    entity_id: Uuid::parse_str(&i.entity_id).ok()?,
                    rank: i.rank,
                    score: i.score,
                    level: i.level.into(),
                    action: i.action.into(),
                    channel: i.channel.as_str().to_owned(),
                    distance: i.distance,
                    changed: i.changed,
                    basis_status: i.basis_status.into(),
                    reasons: i.reasons.clone(),
                })
            })
            .take(IMPACTS_MAX)
            .collect();
        let violations = a
            .violations
            .iter()
            .map(|v| PlanViolation {
                kind: v.kind.as_str().to_owned(),
                entities: v
                    .entities
                    .iter()
                    .filter_map(|id| Uuid::parse_str(id).ok())
                    .collect(),
                severity: v.severity,
                introduced: v.introduced,
                message: v.message.clone(),
            })
            .collect();
        NewAssessment {
            workspace_id: Uuid::nil(),
            team_id: None,
            entity_kind,
            entity_id,
            actor_id: None,
            cause,
            bump: change.map_or(VersionBump::None, |c| c.bump.into()),
            previous_version: change.and_then(|c| c.previous_version),
            next_version: change.and_then(|c| c.next_version),
            raw_score: change.map_or(0.0, |c| c.raw_score).clamp(0.0, 1.0),
            score: a.score.clamp(0.0, 1.0),
            level: a.level.into(),
            recommendation: a.recommendation.into(),
            confidence: a.confidence.clamp(0.0, 1.0),
            replan_root: a
                .replan_root
                .as_deref()
                .and_then(|id| Uuid::parse_str(id).ok()),
            summary: a.summary.clone(),
            impacts,
            violations,
            replan_id: None,
        }
    }

    /// The other items this change asks something of: every affected item but
    /// the changed one whose action is more than nothing.
    pub fn impacted(&self) -> Vec<Uuid> {
        self.impacts
            .iter()
            .filter(|i| !i.changed && i.action != ImpactAction::None)
            .map(|i| i.entity_id)
            .collect()
    }

    /// Whether the assessment is worth keeping: it moved a version or asks someone for something.
    pub fn worth_keeping(&self) -> bool {
        !matches!(self.bump, VersionBump::None) || self.recommendation > Recommendation::NoAction
    }

    /// Whether the owners of the affected items should hear of it: a review or a re-plan.
    pub fn asks_for_review(&self) -> bool {
        self.recommendation >= Recommendation::Review
    }

    /// The item to re-plan, when the engine recommends a partial or full re-plan of one.
    pub fn replan_target(&self) -> Option<(ImpactEntityKind, Uuid)> {
        if self.recommendation < Recommendation::PartialReplan {
            return None;
        }
        let root = self.replan_root?;
        let kind = if root == self.entity_id {
            self.entity_kind
        } else {
            self.impacts
                .iter()
                .find(|i| i.entity_id == root)
                .map_or(ImpactEntityKind::Issue, |i| i.entity_kind)
        };
        Some((kind, root))
    }
}

impl ChangeAssessment {
    /// The assessment as `viewer` may read it: only the impacts and conflicts
    /// on items in `visible`; when that hides any, a summary that names no
    /// item (the engine's names the most affected ones) and no re-plan root
    /// they cannot see.
    pub fn redacted(mut self, visible: &dyn Fn(Uuid) -> bool) -> Self {
        let before = self.impacts.len();
        self.impacts.retain(|i| visible(i.entity_id));
        let mut hidden = self.impacts.len() != before;
        let violations = self.violations.len();
        self.violations
            .retain(|v| v.entities.iter().all(|id| visible(*id)));
        hidden |= self.violations.len() != violations;
        if self.replan_root.is_some_and(|root| !visible(root)) {
            self.replan_root = None;
            hidden = true;
        }
        if self.replan.as_ref().is_some_and(|r| !visible(r.root_id)) {
            self.replan = None;
        }
        if hidden {
            self.summary = format!(
                "{} impact; recommendation: {}",
                self.level.as_str(),
                self.recommendation.as_str().replace('_', " ")
            );
        }
        self
    }
}

// ---------- re-plan proposals ----------

/// Most characters of a re-plan's instructions: what `POST /graphs/{gid}/plan` accepts.
pub const REPLAN_INSTRUCTIONS_MAX: usize = 4_000;
/// Most affected items a re-plan's instructions list, most affected first.
const REPLAN_ITEMS_MAX: usize = 12;
/// Most characters of the re-planned issue's text the instructions quote.
const REPLAN_ROOT_TEXT_MAX: usize = 1_200;
/// What the planner is asked to do with the rest of the instructions.
const REPLAN_ASK: &str = "Keep the steps that still fit the item as it reads now, rework or drop the \
    ones the change made obsolete or incompatible, and add the steps the new scope needs.";

/// What the planner is told when it re-plans after a change.
#[derive(Debug, Clone, Copy)]
pub struct ReplanSeed<'a> {
    /// The assessment, already redacted to what every reader of the plan may see
    /// ([`ChangeAssessment::redacted`]).
    pub assessment: &'a ChangeAssessment,
    /// How the item to re-plan is named (`ENG-12 "Authentication"`).
    pub root: &'a str,
    /// What it reads now (its title and description): the graph's goal may be older.
    pub root_text: &'a str,
    /// How the changed item and the affected ones are named, with their state, by id.
    pub names: &'a HashMap<Uuid, (String, String)>,
    /// How many affected items were left out because not every reader of the plan may see them.
    pub hidden: usize,
}

/// `text`, cut to at most `max` characters, the cut marked with an ellipsis.
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// The planner's instructions for a re-plan: what changed and how much, what
/// the item reads now, what each affected item needs and the plan conflicts
/// the change introduced; at most [`REPLAN_INSTRUCTIONS_MAX`] characters.
pub fn replan_instructions(seed: &ReplanSeed<'_>) -> String {
    let a = seed.assessment;
    let name = |id: Uuid| -> String {
        seed.names.get(&id).map_or_else(
            || "an item that is gone".to_owned(),
            |(label, _)| label.clone(),
        )
    };
    let what = match a.cause {
        AssessmentCause::Created => "was filed",
        AssessmentCause::Updated => "was changed",
        AssessmentCause::Deleted => "was deleted",
        AssessmentCause::Comment => "got a comment",
    };
    let versions = match (&a.previous_version, &a.next_version) {
        (Some(from), Some(to)) if from != to => format!(" ({from} → {to}, {})", a.bump.as_str()),
        _ => String::new(),
    };
    let mut out = format!(
        "Re-plan {}. The change-impact engine recommends a {} after {} {what}{versions}: {} impact, \
         score {:.2}, confidence {:.2}.\nEngine summary: {}\n",
        seed.root,
        a.recommendation.as_str().replace('_', " "),
        name(a.entity_id),
        a.level.as_str(),
        a.score,
        a.confidence,
        a.summary,
    );
    if !seed.root_text.trim().is_empty() {
        let _ = write!(
            out,
            "\n{} now reads:\n{}\n",
            seed.root,
            clip(seed.root_text.trim(), REPLAN_ROOT_TEXT_MAX)
        );
    }
    let affected: Vec<&Impact> = a
        .impacts
        .iter()
        .filter(|i| !i.changed && i.action != ImpactAction::None)
        .take(REPLAN_ITEMS_MAX)
        .collect();
    if !affected.is_empty() {
        out.push_str("\nWhat each affected item needs:\n");
        for i in affected {
            let state = seed
                .names
                .get(&i.entity_id)
                .map(|(_, state)| format!(" ({state})"))
                .unwrap_or_default();
            let reasons = i
                .reasons
                .iter()
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ");
            let _ = writeln!(
                out,
                "- {}{state}: {} ({}, score {:.2}, basis {}){}",
                name(i.entity_id),
                i.action.as_str().replace('_', " "),
                i.level.as_str(),
                i.score,
                i.basis_status.as_str().replace('_', " "),
                if reasons.is_empty() {
                    String::new()
                } else {
                    format!(": {reasons}")
                }
            );
        }
    }
    let conflicts: Vec<&PlanViolation> = a.violations.iter().filter(|v| v.introduced).collect();
    if !conflicts.is_empty() {
        out.push_str("\nPlan conflicts the change introduced:\n");
        for v in conflicts.iter().take(REPLAN_ITEMS_MAX) {
            let _ = writeln!(
                out,
                "- {} (severity {:.2}): {}",
                v.kind.replace('_', " "),
                v.severity,
                v.message
            );
        }
    }
    if seed.hidden > 0 {
        let _ = writeln!(
            out,
            "\n{} more affected item(s) are left out: not everyone who reads this plan may see them.",
            seed.hidden
        );
    }
    let room = REPLAN_INSTRUCTIONS_MAX.saturating_sub(REPLAN_ASK.chars().count() + 2);
    let mut text = clip(out.trim_end(), room);
    text.push_str("\n\n");
    text.push_str(REPLAN_ASK);
    text
}

/// An issue's version and where its plan stands against what it is part of.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct IssueVersion {
    pub issue_id: Uuid,
    pub version: String,
    /// What the issue was planned against: its parent issue, or its project when it has none;
    /// `null` for an issue that is part of nothing, or whose pin is on something it left.
    #[schema(required = true)]
    pub basis: Option<Basis>,
}

/// The version of its parent an issue was planned against, and the parent's version now.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Basis {
    pub entity_kind: ImpactEntityKind,
    pub entity_id: Uuid,
    pub pinned_version: String,
    pub current_version: String,
    /// `current`, `compatible`, `review_suggested` or `incompatible` (a breaking change since
    /// the pin: re-plan, then acknowledge).
    pub status: BasisStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node() -> IssueNode {
        IssueNode {
            id: Uuid::from_u128(2),
            workspace_id: Uuid::from_u128(100),
            team_id: Uuid::from_u128(10),
            parent_id: Some(Uuid::from_u128(1)),
            project_id: Some(Uuid::from_u128(50)),
            cycle_id: Some(Uuid::from_u128(60)),
            assignee_id: Some(Uuid::from_u128(70)),
            creator_id: None,
            title: "Password reset email flow".into(),
            description: "- [ ] Send the email".into(),
            state_name: "In Progress".into(),
            state_category: StateCategory::Started,
            priority: 2,
            labels: vec!["auth".into(), "email".into()],
            due_date: NaiveDate::from_ymd_opt(2026, 11, 20),
            updated_at: Utc::now(),
            version: SemVer::new(1, 2, 0),
            stored_basis: Some((Uuid::from_u128(1), SemVer::new(2, 0, 0))),
        }
    }

    #[test]
    fn priorities_map_to_ranks_where_higher_is_more_important() {
        // 0 none, 1 urgent, 2 high, 3 medium, 4 low → [0, 4, 3, 2, 1].
        let ranks: Vec<Option<u32>> = (0..=4).map(priority_rank).collect();
        assert_eq!(ranks, [Some(0), Some(4), Some(3), Some(2), Some(1)]);
        assert_eq!(priority_rank(5), None);
        assert_eq!(priority_rank(-1), None);
    }

    #[test]
    fn an_issue_maps_as_the_engine_readme_says() {
        let n = node();
        let e = n.entity();
        assert_eq!(e.id, n.id.to_string());
        assert_eq!(e.kind.as_str(), "sub_issue");
        assert_eq!(
            e.parent_id.as_deref(),
            Some(&*Uuid::from_u128(1).to_string())
        );
        assert_eq!(e.version, Some(SemVer::new(1, 2, 0)));
        assert_eq!(e.title(), "Password reset email flow");
        let status = e.status().unwrap();
        assert_eq!(status.category, ac::model::StatusCategory::Started);
        assert_eq!(status.name, "In Progress");
        assert_eq!(e.field(field::PRIORITY), Some(&FieldValue::ordinal(3, 4)));
        assert_eq!(e.assignee(), Some(&*Uuid::from_u128(70).to_string()));
        assert_eq!(
            e.reference(field::PROJECT),
            Some(&*Uuid::from_u128(50).to_string())
        );
        assert_eq!(
            e.reference(field::CYCLE),
            Some(&*Uuid::from_u128(60).to_string())
        );
        assert_eq!(
            e.reference(field::TEAM),
            Some(&*Uuid::from_u128(10).to_string())
        );
        assert_eq!(
            e.due().map(|d| d.to_rfc3339()),
            Some("2026-11-20T00:00:00+00:00".into())
        );
        assert_eq!(
            e.field(field::LABELS),
            Some(&FieldValue::set(["auth", "email"]))
        );
        assert_eq!(e.basis.len(), 1);
        assert_eq!(e.basis[0].version, SemVer::new(2, 0, 0));
    }

    #[test]
    fn an_issue_without_a_parent_is_part_of_its_project() {
        let mut n = node();
        n.parent_id = None;
        n.stored_basis = None;
        let e = n.entity();
        assert_eq!(e.kind.as_str(), "issue");
        assert_eq!(
            e.parent_id.as_deref(),
            Some(&*Uuid::from_u128(50).to_string())
        );
        // Nothing stored: 1.0.0 of the project, where every row starts.
        assert_eq!(n.pinned(), Some((Uuid::from_u128(50), SemVer::INITIAL)));
        // A pin on something it is no longer part of pins nothing.
        n.stored_basis = Some((Uuid::from_u128(1), SemVer::new(3, 0, 0)));
        assert_eq!(n.pinned(), None);
        assert!(n.entity().basis.is_empty());
        // Part of nothing: no parent and no pin.
        n.project_id = None;
        assert_eq!(n.entity().parent_id, None);
        assert_eq!(n.basis_target(), None);
    }

    #[test]
    fn empty_text_and_no_labels_are_null() {
        let mut n = node();
        n.description = "  \n".into();
        n.labels.clear();
        n.due_date = None;
        n.assignee_id = None;
        let e = n.entity();
        for f in [
            field::DESCRIPTION,
            field::LABELS,
            field::DUE_DATE,
            field::ASSIGNEE,
        ] {
            assert!(e.field(f).is_none(), "{f}");
        }
    }

    #[test]
    fn a_project_maps_its_lead_and_target_date() {
        let p = ProjectNode {
            id: Uuid::from_u128(50),
            workspace_id: Uuid::from_u128(100),
            team_id: None,
            name: "Customer portal".into(),
            description: String::new(),
            status: ProjectStatus::Paused,
            lead_id: Some(Uuid::from_u128(70)),
            target_date: NaiveDate::from_ymd_opt(2026, 12, 1),
            updated_at: Utc::now(),
            version: SemVer::INITIAL,
        };
        let e = p.entity();
        assert_eq!(e.kind.as_str(), "project");
        assert_eq!(e.parent_id, None);
        assert_eq!(e.title(), "Customer portal");
        assert_eq!(
            e.status_category(),
            Some(ac::model::StatusCategory::Backlog)
        );
        assert_eq!(e.assignee(), Some(&*Uuid::from_u128(70).to_string()));
        assert!(e.due().is_some());
        assert!(e.field(field::TEAM).is_none());
    }

    #[test]
    fn a_basis_reads_like_a_caret_requirement() {
        let v = SemVer::new;
        assert_eq!(
            BasisStatus::of(v(1, 2, 0), v(1, 2, 0)),
            BasisStatus::Current
        );
        assert_eq!(
            BasisStatus::of(v(1, 2, 0), v(1, 2, 3)),
            BasisStatus::Compatible
        );
        assert_eq!(
            BasisStatus::of(v(1, 2, 0), v(1, 3, 0)),
            BasisStatus::ReviewSuggested
        );
        assert_eq!(
            BasisStatus::of(v(1, 2, 0), v(2, 0, 0)),
            BasisStatus::Incompatible
        );
    }

    #[test]
    fn a_reader_sees_only_the_impacts_on_what_they_can_see() {
        let impact = |n: u128, changed: bool| Impact {
            entity_kind: ImpactEntityKind::Issue,
            entity_id: Uuid::from_u128(n),
            rank: 1,
            score: 0.5,
            level: ImpactLevel::High,
            action: if changed {
                ImpactAction::Replan
            } else {
                ImpactAction::Review
            },
            channel: "down".into(),
            distance: 1,
            changed,
            basis_status: BasisStatus::Incompatible,
            reasons: vec![],
        };
        let a = ChangeAssessment {
            id: Uuid::nil(),
            entity_kind: ImpactEntityKind::Issue,
            entity_id: Uuid::from_u128(1),
            actor: None,
            cause: AssessmentCause::Updated,
            bump: VersionBump::Major,
            previous_version: Some("1.0.0".into()),
            next_version: Some("2.0.0".into()),
            score: 0.8,
            level: ImpactLevel::High,
            recommendation: Recommendation::PartialReplan,
            confidence: 0.9,
            replan_root: Some(Uuid::from_u128(1)),
            summary: "breaking change to \"Secret plan\"".into(),
            impacts: vec![impact(1, true), impact(2, false), impact(3, false)],
            violations: vec![],
            replan: None,
            created_at: Utc::now(),
        };
        let all = a.clone().redacted(&|_| true);
        assert_eq!(all.impacts.len(), 3);
        assert_eq!(all.summary, a.summary);
        let some = a.clone().redacted(&|id| id != Uuid::from_u128(3));
        assert_eq!(some.impacts.len(), 2);
        assert_eq!(some.summary, "high impact; recommendation: partial replan");
        assert_eq!(some.replan_root, Some(Uuid::from_u128(1)));

        // The re-plan of an item the reader cannot see is not theirs to open.
        let mut with_replan = a;
        with_replan.replan = Some(proposal(1));
        let shown = with_replan.clone().redacted(&|id| id != Uuid::from_u128(3));
        assert_eq!(shown.replan.map(|r| r.root_id), Some(Uuid::from_u128(1)));
        let hidden = with_replan.redacted(&|id| id != Uuid::from_u128(1));
        assert_eq!((hidden.replan, hidden.replan_root), (None, None));
    }

    fn proposal(root: u128) -> ReplanProposal {
        ReplanProposal {
            id: Uuid::from_u128(900),
            status: ReplanStatus::Started,
            root_kind: ImpactEntityKind::Issue,
            root_id: Uuid::from_u128(root),
            graph_id: Some(Uuid::from_u128(901)),
            plan_id: Some(Uuid::from_u128(902)),
            plan_status: Some(PlanStatus::Ready),
            error: None,
            starts_at: Utc::now(),
            created_at: Utc::now(),
        }
    }

    fn assessed(impacts: Vec<Impact>, violations: Vec<PlanViolation>) -> ChangeAssessment {
        ChangeAssessment {
            id: Uuid::nil(),
            entity_kind: ImpactEntityKind::Issue,
            entity_id: Uuid::from_u128(1),
            actor: None,
            cause: AssessmentCause::Updated,
            bump: VersionBump::Major,
            previous_version: Some("1.0.0".into()),
            next_version: Some("2.0.0".into()),
            score: 0.83,
            level: ImpactLevel::Critical,
            recommendation: Recommendation::FullReplan,
            confidence: 0.9,
            replan_root: Some(Uuid::from_u128(1)),
            summary: "critical impact; breaking change".into(),
            impacts,
            violations,
            replan: None,
            created_at: Utc::now(),
        }
    }

    fn affected(n: u128, action: ImpactAction) -> Impact {
        Impact {
            entity_kind: ImpactEntityKind::Issue,
            entity_id: Uuid::from_u128(n),
            rank: 2,
            score: 0.71,
            level: ImpactLevel::High,
            action,
            channel: "down".into(),
            distance: 1,
            changed: false,
            basis_status: BasisStatus::Incompatible,
            reasons: vec!["the text it was based on was removed".into()],
        }
    }

    #[test]
    fn a_replan_tells_the_planner_what_changed_and_what_each_item_needs() {
        let names: HashMap<Uuid, (String, String)> = [
            (1, "ENG-12 \"Authentication\"", "In Progress"),
            (2, "ENG-14 \"Password reset email flow\"", "Todo"),
            (3, "ENG-15 \"Session expiry\"", "Todo"),
        ]
        .into_iter()
        .map(|(n, label, state)| (Uuid::from_u128(n), (label.to_owned(), state.to_owned())))
        .collect();
        let conflict = PlanViolation {
            kind: "orphaned_work".into(),
            entities: vec![Uuid::from_u128(2)],
            severity: 0.6,
            introduced: true,
            message: "active work under a rewritten scope".into(),
        };
        let a = assessed(
            vec![
                affected(2, ImpactAction::ObsoleteCandidate),
                affected(3, ImpactAction::None),
            ],
            vec![conflict],
        );
        let text = replan_instructions(&ReplanSeed {
            assessment: &a,
            root: "ENG-12 \"Authentication\"",
            root_text: "Authentication\n\nGoogle OAuth login only.",
            names: &names,
            hidden: 1,
        });
        assert!(
            text.starts_with(
                "Re-plan ENG-12 \"Authentication\". The change-impact engine recommends a full replan after ENG-12 \"Authentication\" was changed (1.0.0 → 2.0.0, major)"
            ),
            "{text}"
        );
        for part in [
            "Google OAuth login only.",
            "- ENG-14 \"Password reset email flow\" (Todo): obsolete candidate (high, score 0.71, basis incompatible): the text it was based on was removed",
            "- orphaned work (severity 0.60): active work under a rewritten scope",
            "1 more affected item(s) are left out",
            REPLAN_ASK,
        ] {
            assert!(text.contains(part), "{part}\n---\n{text}");
        }
        // An item asked nothing of is not listed.
        assert!(!text.contains("Session expiry"), "{text}");
    }

    #[test]
    fn a_replan_fits_what_the_planner_takes() {
        let names = HashMap::new();
        let a = assessed(
            (2..200)
                .map(|n| Impact {
                    reasons: vec!["r".repeat(900), "s".repeat(900)],
                    ..affected(n, ImpactAction::Review)
                })
                .collect(),
            vec![],
        );
        let long = "x".repeat(50_000);
        let text = replan_instructions(&ReplanSeed {
            assessment: &a,
            root: "ENG-12 \"Authentication\"",
            root_text: &long,
            names: &names,
            hidden: 0,
        });
        assert_eq!(text.chars().count(), REPLAN_INSTRUCTIONS_MAX);
        assert!(text.ends_with(&format!("…\n\n{REPLAN_ASK}")), "{text}");
        assert_eq!(clip("abcdef", 4), "abc…");
        assert_eq!(clip("abcd", 4), "abcd");
        assert!(text.contains("an item that is gone"));
    }
}
