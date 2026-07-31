//! Schema migrations.
//!
//! Versioned through SQLite's own `user_version` pragma so upgrades are
//! deterministic and need no extra bookkeeping table. Migrations only ever
//! append — an existing database is never rewritten.

use crate::error::Result;
use rusqlite::Connection;

/// Ordered migrations. Index + 1 is the resulting `user_version`.
const MIGRATIONS: &[&str] = &[
    // v1 — the operation log and the changes each operation caused.
    r#"
    CREATE TABLE operations (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        action       TEXT    NOT NULL,
        kind         TEXT    NOT NULL,
        targets      TEXT    NOT NULL,
        command      TEXT    NOT NULL,
        started_at   INTEGER NOT NULL,
        finished_at  INTEGER,
        success      INTEGER NOT NULL DEFAULT 0,
        cancelled    INTEGER NOT NULL DEFAULT 0,
        error        TEXT
    );

    CREATE TABLE changes (
        id             INTEGER PRIMARY KEY AUTOINCREMENT,
        operation_id   INTEGER NOT NULL REFERENCES operations(id) ON DELETE CASCADE,
        kind           TEXT    NOT NULL,
        package        TEXT    NOT NULL,
        before_version TEXT,
        after_version  TEXT,
        change         TEXT    NOT NULL
    );

    CREATE INDEX idx_changes_operation ON changes(operation_id);
    CREATE INDEX idx_changes_package   ON changes(kind, package);
    CREATE INDEX idx_operations_started ON operations(started_at DESC);
    "#,
    // v2 — per-package update policy.
    r#"
    CREATE TABLE policies (
        kind        TEXT    NOT NULL,
        package     TEXT    NOT NULL,
        rule        TEXT    NOT NULL,
        bake_days   INTEGER,
        note        TEXT,
        updated_at  INTEGER NOT NULL,
        PRIMARY KEY (kind, package)
    );

    /* When a new version was first observed, so "bake for N days" has a
       reference point. Homebrew does not publish release timestamps. */
    CREATE TABLE version_sightings (
        kind       TEXT    NOT NULL,
        package    TEXT    NOT NULL,
        version    TEXT    NOT NULL,
        first_seen INTEGER NOT NULL,
        PRIMARY KEY (kind, package, version)
    );
    "#,
];

pub fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let current = current.max(0) as usize;

    if current > MIGRATIONS.len() {
        // A newer own-brew wrote this database. Refusing is safer than
        // guessing at a schema we do not understand.
        return Err(crate::Error::Catalog(format!(
            "this history database was written by a newer version of own-brew \
             (schema v{current}, this build understands v{})",
            MIGRATIONS.len()
        )));
    }

    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        conn.execute_batch(sql)?;
        conn.pragma_update(None, "user_version", (index + 1) as i64)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap()
    }

    #[test]
    fn migrates_a_fresh_database_to_the_latest_schema() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        assert_eq!(version(&conn), MIGRATIONS.len() as i64);

        for table in ["operations", "changes", "policies", "version_sightings"] {
            let count: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "{table} should exist");
        }
    }

    #[test]
    fn migrating_twice_is_a_no_op() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).expect("re-running migrations must not fail");
        assert_eq!(version(&conn), MIGRATIONS.len() as i64);
    }

    #[test]
    fn applies_only_the_missing_migrations() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1i64).unwrap();

        migrate(&conn).expect("should apply v2 onto a v1 database");
        assert_eq!(version(&conn), MIGRATIONS.len() as i64);
    }

    #[test]
    fn refuses_a_database_from_a_newer_build() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn.pragma_update(None, "user_version", 99i64).unwrap();

        let err = migrate(&conn).expect_err("a future schema must not be touched");
        assert!(err.to_string().contains("newer version"));
    }
}
