//! The operation log.
//!
//! Every state-changing operation is recorded together with the changes it
//! actually caused, so the user can see what happened to their machine and —
//! the point of the whole product — find the version to go back to.

pub mod diff;
mod schema;

pub use diff::{Change, ChangeKind};

use crate::error::Result;
use crate::model::entry::Kind;
use crate::ops::Action;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct History {
    conn: Mutex<Connection>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub id: i64,
    pub action: String,
    pub kind: Kind,
    pub targets: Vec<String>,
    pub command: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub success: bool,
    pub cancelled: bool,
    pub error: Option<String>,
    pub changes: Vec<Change>,
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

impl History {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::prepare(conn)
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        schema::migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Record an operation that is about to start; returns its id.
    pub fn begin(
        &self,
        action: Action,
        kind: Kind,
        targets: &[String],
        command: &str,
    ) -> Result<i64> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO operations (action, kind, targets, command, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                format!("{action:?}").to_lowercase(),
                kind.as_str(),
                serde_json::to_string(targets).unwrap_or_else(|_| "[]".into()),
                command,
                now(),
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Close out an operation and record what it changed.
    pub fn finish(
        &self,
        id: i64,
        success: bool,
        cancelled: bool,
        error: Option<&str>,
        changes: &[Change],
    ) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;

        tx.execute(
            "UPDATE operations
             SET finished_at = ?2, success = ?3, cancelled = ?4, error = ?5
             WHERE id = ?1",
            params![id, now(), success as i64, cancelled as i64, error],
        )?;

        {
            let mut insert = tx.prepare(
                "INSERT INTO changes
                     (operation_id, kind, package, before_version, after_version, change)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for change in changes {
                insert.execute(params![
                    id,
                    change.kind.as_str(),
                    change.package,
                    change.before_version,
                    change.after_version,
                    change.change.as_str(),
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// Most recent operations first.
    pub fn recent(&self, limit: usize) -> Result<Vec<Operation>> {
        let conn = self.lock();
        let mut statement = conn.prepare(
            "SELECT id, action, kind, targets, command, started_at, finished_at,
                    success, cancelled, error
             FROM operations
             ORDER BY started_at DESC, id DESC
             LIMIT ?1",
        )?;

        let rows = statement.query_map([limit as i64], |row| {
            Ok(Operation {
                id: row.get(0)?,
                action: row.get(1)?,
                kind: parse_kind(&row.get::<_, String>(2)?),
                targets: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
                command: row.get(4)?,
                started_at: row.get(5)?,
                finished_at: row.get(6)?,
                success: row.get::<_, i64>(7)? != 0,
                cancelled: row.get::<_, i64>(8)? != 0,
                error: row.get(9)?,
                changes: Vec::new(),
            })
        })?;

        let mut operations: Vec<Operation> = rows.collect::<rusqlite::Result<_>>()?;
        drop(statement);

        for operation in &mut operations {
            operation.changes = Self::changes_for(&conn, operation.id)?;
        }
        Ok(operations)
    }

    fn changes_for(conn: &Connection, operation_id: i64) -> Result<Vec<Change>> {
        let mut statement = conn.prepare(
            "SELECT kind, package, before_version, after_version, change
             FROM changes WHERE operation_id = ?1 ORDER BY package",
        )?;
        let rows = statement.query_map([operation_id], |row| {
            Ok(Change {
                kind: parse_kind(&row.get::<_, String>(0)?),
                package: row.get(1)?,
                before_version: row.get(2)?,
                after_version: row.get(3)?,
                change: ChangeKind::parse(&row.get::<_, String>(4)?),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Versions this package is known to have been on, most recent first.
    ///
    /// This is what lets own-brew offer "go back to what you had on Tuesday"
    /// even for a version no longer on disk.
    pub fn known_versions(&self, kind: Kind, package: &str) -> Result<Vec<KnownVersion>> {
        let conn = self.lock();
        let mut statement = conn.prepare(
            "SELECT c.before_version, o.started_at
             FROM changes c
             JOIN operations o ON o.id = c.operation_id
             WHERE c.kind = ?1 AND c.package = ?2 AND c.before_version IS NOT NULL
               AND o.success = 1
             -- id breaks ties: several operations can share a timestamp, and
             -- the most recent previous version must come first.
             ORDER BY o.started_at DESC, o.id DESC",
        )?;

        let rows = statement.query_map(params![kind.as_str(), package], |row| {
            Ok(KnownVersion {
                version: row.get(0)?,
                last_seen: row.get(1)?,
            })
        })?;

        let mut seen = std::collections::HashSet::new();
        Ok(rows
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|v| seen.insert(v.version.clone()))
            .collect())
    }

    /// Note that a version exists now, for the policy engine's bake timer.
    /// The first sighting wins; later ones are ignored.
    pub fn observe_version(&self, kind: Kind, package: &str, version: &str) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT OR IGNORE INTO version_sightings (kind, package, version, first_seen)
             VALUES (?1, ?2, ?3, ?4)",
            params![kind.as_str(), package, version, now()],
        )?;
        Ok(())
    }

    pub fn first_seen(&self, kind: Kind, package: &str, version: &str) -> Result<Option<i64>> {
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT first_seen FROM version_sightings
                 WHERE kind = ?1 AND package = ?2 AND version = ?3",
                params![kind.as_str(), package, version],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn connection(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.lock()
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownVersion {
    pub version: String,
    pub last_seen: i64,
}

fn parse_kind(raw: &str) -> Kind {
    match raw {
        "cask" => Kind::Cask,
        _ => Kind::Formula,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::InstalledPackage;

    fn pkg(id: &str, version: &str) -> InstalledPackage {
        InstalledPackage {
            kind: Kind::Formula,
            id: id.to_owned(),
            name: id.to_owned(),
            desc: None,
            version: Some(version.to_owned()),
            outdated: false,
            pinned: false,
            installed_on_request: true,
            installed_at: None,
            rollback_targets: Vec::new(),
            self_updating: false,
        }
    }

    fn history() -> History {
        History::in_memory().expect("in-memory database")
    }

    #[test]
    fn records_an_operation_and_its_changes() {
        let history = history();
        let id = history
            .begin(
                Action::Upgrade,
                Kind::Formula,
                &["jq".to_owned()],
                "brew upgrade jq",
            )
            .unwrap();

        let changes = diff::diff(&[pkg("jq", "1.8.1")], &[pkg("jq", "1.8.2")]);
        history.finish(id, true, false, None, &changes).unwrap();

        let recent = history.recent(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].command, "brew upgrade jq");
        assert!(recent[0].success);
        assert!(recent[0].finished_at.is_some());
        assert_eq!(recent[0].changes.len(), 1);
        assert_eq!(recent[0].changes[0].change, ChangeKind::Upgraded);
        assert_eq!(recent[0].targets, vec!["jq".to_owned()]);
    }

    #[test]
    fn a_failed_operation_keeps_its_error() {
        let history = history();
        let id = history
            .begin(
                Action::Install,
                Kind::Cask,
                &["x".into()],
                "brew install --cask x",
            )
            .unwrap();
        history.finish(id, false, false, Some("boom"), &[]).unwrap();

        let recent = history.recent(1).unwrap();
        assert!(!recent[0].success);
        assert_eq!(recent[0].error.as_deref(), Some("boom"));
    }

    #[test]
    fn recent_returns_newest_first_and_respects_the_limit() {
        let history = history();
        for name in ["a", "b", "c"] {
            let id = history
                .begin(Action::Install, Kind::Formula, &[name.into()], name)
                .unwrap();
            history.finish(id, true, false, None, &[]).unwrap();
        }
        let recent = history.recent(2).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].command, "c", "newest first");
    }

    #[test]
    fn known_versions_are_deduplicated_and_ordered() {
        let history = history();

        for (before, after) in [("1.0", "1.1"), ("1.1", "1.2"), ("1.2", "1.3")] {
            let id = history
                .begin(
                    Action::Upgrade,
                    Kind::Formula,
                    &["jq".into()],
                    "brew upgrade jq",
                )
                .unwrap();
            let changes = diff::diff(&[pkg("jq", before)], &[pkg("jq", after)]);
            history.finish(id, true, false, None, &changes).unwrap();
        }

        let versions = history.known_versions(Kind::Formula, "jq").unwrap();
        let names: Vec<_> = versions.iter().map(|v| v.version.as_str()).collect();
        assert_eq!(
            names,
            ["1.2", "1.1", "1.0"],
            "newest previous version first"
        );
    }

    #[test]
    fn failed_operations_do_not_contribute_rollback_targets() {
        let history = history();
        let id = history
            .begin(
                Action::Upgrade,
                Kind::Formula,
                &["jq".into()],
                "brew upgrade jq",
            )
            .unwrap();
        let changes = diff::diff(&[pkg("jq", "1.0")], &[pkg("jq", "1.1")]);
        history
            .finish(id, false, false, Some("failed"), &changes)
            .unwrap();

        assert!(history
            .known_versions(Kind::Formula, "jq")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn version_sightings_keep_the_first_timestamp() {
        let history = history();
        history
            .observe_version(Kind::Formula, "jq", "1.8.2")
            .unwrap();
        let first = history.first_seen(Kind::Formula, "jq", "1.8.2").unwrap();
        assert!(first.is_some());

        history
            .observe_version(Kind::Formula, "jq", "1.8.2")
            .unwrap();
        assert_eq!(
            history.first_seen(Kind::Formula, "jq", "1.8.2").unwrap(),
            first,
            "re-observing must not reset the bake clock"
        );

        assert!(history
            .first_seen(Kind::Formula, "jq", "9.9.9")
            .unwrap()
            .is_none());
    }

    #[test]
    fn deleting_an_operation_removes_its_changes() {
        let history = history();
        let id = history
            .begin(
                Action::Upgrade,
                Kind::Formula,
                &["jq".into()],
                "brew upgrade jq",
            )
            .unwrap();
        let changes = diff::diff(&[pkg("jq", "1.0")], &[pkg("jq", "1.1")]);
        history.finish(id, true, false, None, &changes).unwrap();

        let conn = history.connection();
        conn.execute("DELETE FROM operations WHERE id = ?1", [id])
            .unwrap();
        let orphans: i64 = conn
            .query_row("SELECT count(*) FROM changes", [], |row| row.get(0))
            .unwrap();
        assert_eq!(orphans, 0, "foreign keys should cascade");
    }
}
