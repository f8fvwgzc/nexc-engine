//! Artifacts kept by hand: folders, text files and their versions. The
//! content is a column of its version, so cascades clean up and nothing is
//! on disk. A file attached to an issue is read as the issue is (its team's
//! visibility); any other file, and every folder, by members but guests.
//! A private team's issue, or the team, takes its files along when it goes
//! (a trigger), so they never fall to the whole workspace.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgConnection, PgExecutor, Row};
use uuid::Uuid;

use crate::domain::artifact_file::{
    ArtifactContent, ArtifactFile, ArtifactFolder, ArtifactVersion, extension_of, kind_and_language,
};
use crate::domain::issue::{IssuePerson, IssueRef};

impl FromRow<'_, PgRow> for ArtifactFolder {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(ArtifactFolder {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            parent_id: row.try_get("parent_id")?,
            name: row.try_get("name")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

/// Someone a row names by id, with the name joined as `<prefix>_name`.
fn person(row: &PgRow, id: &str, name: &str) -> Result<Option<IssuePerson>, sqlx::Error> {
    let user_id: Option<Uuid> = row.try_get(id)?;
    let name: Option<String> = row.try_get(name)?;
    Ok(user_id.map(|user_id| IssuePerson {
        user_id,
        name: name.unwrap_or_default(),
    }))
}

impl FromRow<'_, PgRow> for ArtifactFile {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let name: String = row.try_get("name")?;
        let extension = extension_of(&name).unwrap_or_default();
        let (kind, language) = kind_and_language(&name, &extension);
        let issue_id: Option<Uuid> = row.try_get("issue_id")?;
        let issue = match issue_id {
            Some(id) => {
                let key: String = row.try_get("issue_key")?;
                let number: i32 = row.try_get("issue_number")?;
                Some(IssueRef {
                    id,
                    identifier: format!("{key}-{number}"),
                    title: row.try_get("issue_title")?,
                })
            }
            None => None,
        };
        Ok(ArtifactFile {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            folder_id: row.try_get("folder_id")?,
            name,
            extension,
            kind,
            language: language.to_owned(),
            issue,
            current_version: row.try_get("current_version")?,
            size: row.try_get("size")?,
            sha256: row.try_get("sha256")?,
            duplicated_from: row.try_get("duplicated_from")?,
            created_by: person(row, "created_by", "created_by_name")?,
            updated_by: person(row, "updated_by", "updated_by_name")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

impl FromRow<'_, PgRow> for ArtifactVersion {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(ArtifactVersion {
            version: row.try_get("version")?,
            size: row.try_get("size")?,
            sha256: row.try_get("sha256")?,
            created_by: person(row, "created_by", "created_by_name")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

impl FromRow<'_, PgRow> for ArtifactContent {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(ArtifactContent {
            file_id: row.try_get("file_id")?,
            version: row.try_get("version")?,
            content: row.try_get("content")?,
        })
    }
}

/// SQL predicate: user `$1`, whose workspace membership row is aliased `wm`,
/// may read the file aliased `f`, whose issue's team is aliased `it`.
macro_rules! file_visible {
    () => {
        concat!(
            "(CASE WHEN f.issue_id IS NULL THEN wm.role <> 'guest' ELSE ",
            team_visible!("it", "wm", "$1"),
            " END)"
        )
    };
}

/// A file with its issue and the names of who made and last saved it, for
/// user `$1` who must be a member of its workspace and able to read it.
macro_rules! file_for_user {
    ($tail:literal) => {
        concat!(
            "SELECT f.id, f.workspace_id, f.folder_id, f.name, f.issue_id, it.key AS issue_key,
                    i.number AS issue_number, i.title AS issue_title, f.current_version, f.size,
                    f.sha256, f.duplicated_from, f.created_by, cu.name AS created_by_name,
                    f.updated_by, uu.name AS updated_by_name, f.created_at, f.updated_at
             FROM artifact_files f
             JOIN workspace_members wm ON wm.workspace_id = f.workspace_id AND wm.user_id = $1
             LEFT JOIN issues i ON i.id = f.issue_id
             LEFT JOIN teams it ON it.id = i.team_id
             LEFT JOIN users cu ON cu.id = f.created_by
             LEFT JOIN users uu ON uu.id = f.updated_by
             WHERE ",
            file_visible!(),
            " AND ",
            $tail
        )
    };
}

// ---------- folders ----------

/// The folders of a workspace, by name (the client builds the tree).
pub async fn folders(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Vec<ArtifactFolder>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM artifact_folders WHERE workspace_id = $1 ORDER BY lower(name), id",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await
}

pub async fn count_folders(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM artifact_folders WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_one(db)
        .await
}

/// One folder of a workspace.
pub async fn find_folder(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<ArtifactFolder>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM artifact_folders WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Adds a folder. Names are unique among siblings, ignoring case.
pub async fn create_folder(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    parent_id: Option<Uuid>,
    name: &str,
    created_by: Uuid,
) -> Result<ArtifactFolder, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO artifact_folders (id, workspace_id, parent_id, name, created_by)
         VALUES ($1, $2, $3, $4, $5) RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(workspace_id)
    .bind(parent_id)
    .bind(name)
    .bind(created_by)
    .fetch_one(db)
    .await
}

/// Holds the folder tree of a workspace still until the transaction ends:
/// folders are added, moved and deleted one at a time, so two moves that
/// cross (A into B while B goes into A) cannot both pass the loop check, and
/// no folder is added under one while it is deleted. File writes do not take
/// it; a delete holds what it takes, and what it notes on issues, with
/// [`lock_under`].
pub async fn lock_folders(db: impl PgExecutor<'_>, workspace_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtext('nexc.artifact_folders'), hashtext($1::text))",
    )
    .bind(workspace_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Renames or moves a folder.
pub async fn save_folder(
    db: impl PgExecutor<'_>,
    folder: &ArtifactFolder,
) -> Result<ArtifactFolder, sqlx::Error> {
    sqlx::query_as(
        "UPDATE artifact_folders SET name = $2, parent_id = $3, updated_at = now()
         WHERE id = $1 RETURNING *",
    )
    .bind(folder.id)
    .bind(&folder.name)
    .bind(folder.parent_id)
    .fetch_one(db)
    .await
}

/// Whether `candidate` is folder `id` itself or one of the folders under it,
/// which makes it unusable as `id`'s parent.
pub async fn folder_is_within(
    db: impl PgExecutor<'_>,
    id: Uuid,
    candidate: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "WITH RECURSIVE under AS (
             SELECT $1::uuid AS id
             UNION
             SELECT c.id FROM artifact_folders c JOIN under u ON c.parent_id = u.id)
         SELECT EXISTS (SELECT 1 FROM under WHERE id = $2)",
    )
    .bind(id)
    .bind(candidate)
    .fetch_one(db)
    .await
}

/// Locks, until the transaction ends, what a folder delete takes, in the
/// order a team or issue delete takes it (the team, its issues, their
/// files): the teams and then the issues of the files in the folder and the
/// folders under it, against being deleted only; those files, against
/// changes; then those folders. So a team or issue delete that would leave
/// or take those files finishes first or waits for the folder delete, and
/// never holds a file the folder delete waits on while the folder delete
/// notes the file's going on its issue.
///
/// What the delete counts is then what it takes: a save, rename, move or
/// re-attachment of one of those files finishes first or waits, and so does
/// a file put into one of those folders (its foreign key shares the folder's
/// row, which this holds for update). The files are held only against
/// changes, so a copy being made of one (which shares its row) goes on; and
/// they come before the folders, so a file moving between two of them never
/// waits on the delete while the delete waits on it. Call after
/// [`lock_folders`], which keeps the tree as it is.
pub async fn lock_under(db: &mut PgConnection, folder_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "WITH RECURSIVE under AS (
             SELECT $1::uuid AS id
             UNION
             SELECT c.id FROM artifact_folders c JOIN under u ON c.parent_id = u.id)
         SELECT 1 FROM teams WHERE id IN (
             SELECT i.team_id FROM issues i JOIN artifact_files f ON f.issue_id = i.id
             WHERE f.folder_id IN (SELECT id FROM under))
         ORDER BY id FOR KEY SHARE",
    )
    .bind(folder_id)
    .execute(&mut *db)
    .await?;
    sqlx::query(
        "WITH RECURSIVE under AS (
             SELECT $1::uuid AS id
             UNION
             SELECT c.id FROM artifact_folders c JOIN under u ON c.parent_id = u.id)
         SELECT 1 FROM issues WHERE id IN (
             SELECT issue_id FROM artifact_files WHERE folder_id IN (SELECT id FROM under))
         ORDER BY id FOR KEY SHARE",
    )
    .bind(folder_id)
    .execute(&mut *db)
    .await?;
    sqlx::query(
        "WITH RECURSIVE under AS (
             SELECT $1::uuid AS id
             UNION
             SELECT c.id FROM artifact_folders c JOIN under u ON c.parent_id = u.id)
         SELECT 1 FROM artifact_files WHERE folder_id IN (SELECT id FROM under)
         FOR NO KEY UPDATE",
    )
    .bind(folder_id)
    .execute(&mut *db)
    .await?;
    sqlx::query(
        "WITH RECURSIVE under AS (
             SELECT $1::uuid AS id
             UNION
             SELECT c.id FROM artifact_folders c JOIN under u ON c.parent_id = u.id)
         SELECT 1 FROM artifact_folders WHERE id IN (SELECT id FROM under) FOR UPDATE",
    )
    .bind(folder_id)
    .execute(&mut *db)
    .await?;
    Ok(())
}

/// How many files in a folder or the folders under it user `$1` cannot
/// read (they are attached to issues of teams hidden from them).
pub async fn hidden_files_under(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    folder_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(concat!(
        "WITH RECURSIVE under AS (
             SELECT $2::uuid AS id
             UNION
             SELECT c.id FROM artifact_folders c JOIN under u ON c.parent_id = u.id)
         SELECT count(*) FROM artifact_files f
         JOIN workspace_members wm ON wm.workspace_id = f.workspace_id AND wm.user_id = $1
         LEFT JOIN issues i ON i.id = f.issue_id
         LEFT JOIN teams it ON it.id = i.team_id
         WHERE f.folder_id IN (SELECT id FROM under) AND NOT ",
        file_visible!()
    ))
    .bind(user_id)
    .bind(folder_id)
    .fetch_one(db)
    .await
}

/// The files in a folder or the folders under it that are attached to an
/// issue: `(issue, name, current version)`, to tell the issues they go.
pub async fn issue_files_under(
    db: impl PgExecutor<'_>,
    folder_id: Uuid,
) -> Result<Vec<(Uuid, String, i32)>, sqlx::Error> {
    sqlx::query_as(
        "WITH RECURSIVE under AS (
             SELECT $1::uuid AS id
             UNION
             SELECT c.id FROM artifact_folders c JOIN under u ON c.parent_id = u.id)
         SELECT f.issue_id, f.name, f.current_version FROM artifact_files f
         WHERE f.folder_id IN (SELECT id FROM under) AND f.issue_id IS NOT NULL
         ORDER BY f.issue_id, lower(f.name)",
    )
    .bind(folder_id)
    .fetch_all(db)
    .await
}

/// Deletes a folder with the folders and files under it.
pub async fn delete_folder(db: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM artifact_folders WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

// ---------- files ----------

/// What narrows a file list; `None` leaves a dimension open.
#[derive(Debug, Clone, Default)]
pub struct FileFilter {
    /// Only the files directly in this folder; `Some(None)` for those at the top level.
    pub folder: Option<Option<Uuid>>,
    /// Only the files attached to this issue, in any folder.
    pub issue_id: Option<Uuid>,
    /// Matches the name, case-insensitively.
    pub q: Option<String>,
    /// By name rather than most recently saved first.
    pub by_name: bool,
    /// Most recently saved first: only those saved before this `(updated_at, id)`.
    pub before: Option<(DateTime<Utc>, Uuid)>,
    pub limit: i64,
}

/// Files of a workspace that `user_id` can read: those of a folder or an
/// issue by name, or every one most recently saved first. Each is its own
/// statement, so each reads its own index: `artifact_files_name_idx` (the
/// folder and the issue are matched as that index spells them) or
/// `artifact_files_updated_idx`.
pub async fn files(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
    f: &FileFilter,
) -> Result<Vec<ArtifactFile>, sqlx::Error> {
    if f.by_name {
        return sqlx::query_as(file_for_user!(
            "f.workspace_id = $2
             AND (NOT $3 OR COALESCE(f.folder_id, '00000000-0000-0000-0000-000000000000'::uuid)
                          = COALESCE($4, '00000000-0000-0000-0000-000000000000'::uuid))
             AND ($5::uuid IS NULL OR f.issue_id = $5)
             AND ($6::text IS NULL OR f.name ILIKE '%' || $6 || '%')
             ORDER BY lower(f.name), f.updated_at DESC, f.id LIMIT $7"
        ))
        .bind(user_id)
        .bind(workspace_id)
        .bind(f.folder.is_some())
        .bind(f.folder.flatten())
        .bind(f.issue_id)
        .bind(f.q.as_deref())
        .bind(f.limit)
        .fetch_all(db)
        .await;
    }
    let (before_at, before_id) = f.before.unzip();
    sqlx::query_as(file_for_user!(
        "f.workspace_id = $2
         AND ($3::text IS NULL OR f.name ILIKE '%' || $3 || '%')
         AND ($4::timestamptz IS NULL OR (f.updated_at, f.id) < ($4, $5))
         ORDER BY f.updated_at DESC, f.id DESC LIMIT $6"
    ))
    .bind(user_id)
    .bind(workspace_id)
    .bind(f.q.as_deref())
    .bind(before_at)
    .bind(before_id)
    .bind(f.limit)
    .fetch_all(db)
    .await
}

/// One file, if `user_id` can read it.
pub async fn find_file(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    id: Uuid,
) -> Result<Option<ArtifactFile>, sqlx::Error> {
    sqlx::query_as(file_for_user!("f.id = $2"))
        .bind(user_id)
        .bind(id)
        .fetch_optional(db)
        .await
}

/// The names a file of an issue (`None`: of no issue) cannot take in a
/// folder of a workspace (`None`: the top level), lower case: those of the
/// files of the same issue there, which whoever may put it there can see.
pub async fn names_in(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    folder_id: Option<Uuid>,
    issue_id: Option<Uuid>,
) -> Result<HashSet<String>, sqlx::Error> {
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT lower(name) FROM artifact_files
         WHERE workspace_id = $1 AND folder_id IS NOT DISTINCT FROM $2
           AND issue_id IS NOT DISTINCT FROM $3",
    )
    .bind(workspace_id)
    .bind(folder_id)
    .bind(issue_id)
    .fetch_all(db)
    .await?;
    Ok(names.into_iter().collect())
}

/// A file to insert, with its first version.
#[derive(Debug, Clone)]
pub struct NewFile<'a> {
    pub workspace_id: Uuid,
    pub folder_id: Option<Uuid>,
    pub issue_id: Option<Uuid>,
    pub name: &'a str,
    pub content: &'a str,
    /// Hex SHA-256 of the content.
    pub sha256: &'a str,
    pub duplicated_from: Option<Uuid>,
    pub created_by: Uuid,
}

/// Inserts a file and its version 1; returns its id. Must run in a
/// transaction so the file never exists without its content.
pub async fn create_file(db: &mut PgConnection, n: &NewFile<'_>) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    let size = n.content.len() as i64;
    sqlx::query(
        "INSERT INTO artifact_files (id, workspace_id, folder_id, issue_id, name, current_version,
                                     size, sha256, duplicated_from, created_by, updated_by)
         VALUES ($1, $2, $3, $4, $5, 1, $6, $7, $8, $9, $9)",
    )
    .bind(id)
    .bind(n.workspace_id)
    .bind(n.folder_id)
    .bind(n.issue_id)
    .bind(n.name)
    .bind(size)
    .bind(n.sha256)
    .bind(n.duplicated_from)
    .bind(n.created_by)
    .execute(&mut *db)
    .await?;
    sqlx::query(
        "INSERT INTO artifact_file_versions (file_id, version, content, size, sha256, created_by)
         VALUES ($1, 1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(n.content)
    .bind(size)
    .bind(n.sha256)
    .bind(n.created_by)
    .execute(&mut *db)
    .await?;
    Ok(id)
}

/// Saves a new version of a file on top of `base_version`; returns its
/// number, or `None` when the file has moved past `base_version` (someone
/// else saved first). Must run in a transaction.
pub async fn add_version(
    db: &mut PgConnection,
    file_id: Uuid,
    base_version: i32,
    content: &str,
    sha256: &str,
    user_id: Uuid,
) -> Result<Option<i32>, sqlx::Error> {
    let size = content.len() as i64;
    let version: Option<i32> = sqlx::query_scalar(
        "UPDATE artifact_files SET current_version = current_version + 1, size = $3, sha256 = $4,
                updated_by = $5, updated_at = now()
         WHERE id = $1 AND current_version = $2 RETURNING current_version",
    )
    .bind(file_id)
    .bind(base_version)
    .bind(size)
    .bind(sha256)
    .bind(user_id)
    .fetch_optional(&mut *db)
    .await?;
    let Some(version) = version else {
        return Ok(None);
    };
    sqlx::query(
        "INSERT INTO artifact_file_versions (file_id, version, content, size, sha256, created_by)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(file_id)
    .bind(version)
    .bind(content)
    .bind(size)
    .bind(sha256)
    .bind(user_id)
    .execute(&mut *db)
    .await?;
    Ok(Some(version))
}

/// The versions of a file, newest first.
pub async fn versions(
    db: impl PgExecutor<'_>,
    file_id: Uuid,
) -> Result<Vec<ArtifactVersion>, sqlx::Error> {
    sqlx::query_as(
        "SELECT v.version, v.size, v.sha256, v.created_by, u.name AS created_by_name, v.created_at
         FROM artifact_file_versions v LEFT JOIN users u ON u.id = v.created_by
         WHERE v.file_id = $1 ORDER BY v.version DESC",
    )
    .bind(file_id)
    .fetch_all(db)
    .await
}

/// The text of one version of a file (`None`: the current one).
pub async fn content(
    db: impl PgExecutor<'_>,
    file_id: Uuid,
    version: Option<i32>,
) -> Result<Option<ArtifactContent>, sqlx::Error> {
    sqlx::query_as(
        "SELECT v.file_id, v.version, v.content
         FROM artifact_file_versions v JOIN artifact_files f ON f.id = v.file_id
         WHERE v.file_id = $1 AND v.version = COALESCE($2, f.current_version)",
    )
    .bind(file_id)
    .bind(version)
    .fetch_optional(db)
    .await
}

/// Renames a file, moves it to another folder, or attaches it to another
/// issue (or none). Its versions and when they were saved stay as they are.
pub async fn save_file(
    db: impl PgExecutor<'_>,
    id: Uuid,
    name: &str,
    folder_id: Option<Uuid>,
    issue_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE artifact_files SET name = $2, folder_id = $3, issue_id = $4 WHERE id = $1")
        .bind(id)
        .bind(name)
        .bind(folder_id)
        .bind(issue_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Deletes a file with its versions.
pub async fn delete_file(db: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM artifact_files WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}
