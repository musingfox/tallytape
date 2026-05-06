use anyhow::Context;
use rusqlite::{OptionalExtension, Row, TransactionBehavior};

use crate::Database;

/// A session row from the `sessions` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: i64,
    pub source: String,
    pub external_id: String,
    pub cwd: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub metadata: Option<String>,
}

/// Data needed to insert a new session row.
#[derive(Debug, Clone)]
pub struct NewSession {
    pub source: String,
    pub external_id: String,
    pub cwd: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub metadata: Option<String>,
}

/// Data-access object for the `sessions` table.
pub struct SessionRepository {
    db: Database,
}

impl SessionRepository {
    /// Create a new repository holding `db` by value.
    ///
    /// `Database` is `Clone` over an `Arc<Mutex<Connection>>`, so holding it
    /// by value is cheap and gives the repository shared access to the same
    /// underlying connection.
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Map a `sessions` result row to a [`Session`].
    fn map_session_row(row: &Row<'_>) -> rusqlite::Result<Session> {
        Ok(Session {
            id: row.get(0)?,
            source: row.get(1)?,
            external_id: row.get(2)?,
            cwd: row.get(3)?,
            started_at: row.get(4)?,
            ended_at: row.get(5)?,
            metadata: row.get(6)?,
        })
    }

    /// Insert a session if one does not already exist for `(source, external_id)`,
    /// then return the current session row (first-write-wins).
    ///
    /// # Atomicity
    ///
    /// Uses `BEGIN IMMEDIATE` so the SELECT that follows the insert is part of
    /// the same transaction, and no concurrent writer can interleave.
    ///
    /// # Errors
    ///
    /// Returns `Err` on lock poison, transaction begin/commit failure, insert
    /// failure, or if the row is missing after upsert (should never happen).
    pub fn upsert(&self, new: NewSession) -> anyhow::Result<Session> {
        let mut conn = self.db.lock();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context("SessionRepository::upsert: begin")?;

        tx.execute(
            "INSERT INTO sessions (source, external_id, cwd, started_at, ended_at, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(source, external_id) DO NOTHING",
            rusqlite::params![
                new.source,
                new.external_id,
                new.cwd,
                new.started_at,
                new.ended_at,
                new.metadata,
            ],
        )
        .context("SessionRepository::upsert: insert")?;

        let session = tx
            .query_row(
                "SELECT id, source, external_id, cwd, started_at, ended_at, metadata \
                 FROM sessions WHERE source = ?1 AND external_id = ?2",
                rusqlite::params![new.source, new.external_id],
                Self::map_session_row,
            )
            .context("SessionRepository::upsert: row missing after upsert")?;

        tx.commit().context("SessionRepository::upsert: commit")?;

        Ok(session)
    }

    /// Look up a session by `(source, external_id)`.
    ///
    /// Returns `Ok(None)` when no matching row exists.
    pub fn find_by_source_and_external_id(
        &self,
        source: &str,
        external_id: &str,
    ) -> anyhow::Result<Option<Session>> {
        let conn = self.db.lock();
        conn.query_row(
            "SELECT id, source, external_id, cwd, started_at, ended_at, metadata \
             FROM sessions WHERE source = ?1 AND external_id = ?2",
            rusqlite::params![source, external_id],
            Self::map_session_row,
        )
        .optional()
        .context("SessionRepository::find_by_source_and_external_id: query failed")
    }

    /// Look up a session by its primary key.
    ///
    /// Returns `Ok(None)` when no row with that `id` exists.
    pub fn find_by_id(&self, id: i64) -> anyhow::Result<Option<Session>> {
        let conn = self.db.lock();
        conn.query_row(
            "SELECT id, source, external_id, cwd, started_at, ended_at, metadata \
             FROM sessions WHERE id = ?1",
            rusqlite::params![id],
            Self::map_session_row,
        )
        .optional()
        .context("SessionRepository::find_by_id: query failed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Open a fresh in-memory database for each test.
    fn open_db() -> Database {
        Database::open(":memory:").expect("in-memory db should open")
    }

    // -------------------------------------------------------------------------
    // 1. upsert_fresh_insert_round_trips_all_fields
    // -------------------------------------------------------------------------
    #[test]
    fn upsert_fresh_insert_round_trips_all_fields() {
        let db = open_db();
        let repo = SessionRepository::new(db);

        let new = NewSession {
            source: "claude-code".to_string(),
            external_id: "ext-1".to_string(),
            cwd: Some("/tmp".to_string()),
            started_at: 1_700_000_000,
            ended_at: None,
            metadata: None,
        };

        let session = repo.upsert(new).expect("upsert should succeed");

        assert_eq!(session.id, 1);
        assert_eq!(session.source, "claude-code");
        assert_eq!(session.external_id, "ext-1");
        assert_eq!(session.cwd, Some("/tmp".to_string()));
        assert_eq!(session.started_at, 1_700_000_000);
        assert_eq!(session.ended_at, None);
        assert_eq!(session.metadata, None);
    }

    // -------------------------------------------------------------------------
    // 2. upsert_is_idempotent_first_write_wins
    // -------------------------------------------------------------------------
    #[test]
    fn upsert_is_idempotent_first_write_wins() {
        let db = open_db();
        let repo = SessionRepository::new(db.clone());

        let first = NewSession {
            source: "claude-code".to_string(),
            external_id: "ext-1".to_string(),
            cwd: Some("/tmp".to_string()),
            started_at: 1_700_000_000,
            ended_at: None,
            metadata: None,
        };
        let second = NewSession {
            source: "claude-code".to_string(),
            external_id: "ext-1".to_string(),
            cwd: Some("/other".to_string()),
            started_at: 1_700_000_000,
            ended_at: None,
            metadata: None,
        };

        let s1 = repo.upsert(first).expect("first upsert should succeed");
        let s2 = repo.upsert(second).expect("second upsert should succeed");

        assert_eq!(s1.id, s2.id, "both calls must return the same id");
        assert_eq!(
            s1.cwd,
            Some("/tmp".to_string()),
            "cwd from first call must be preserved"
        );
        assert_eq!(
            s2.cwd,
            Some("/tmp".to_string()),
            "second call must also return first cwd"
        );

        let conn = db.lock();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "exactly one session row must exist");
    }

    // -------------------------------------------------------------------------
    // 3. upsert_same_external_id_different_source_creates_two_rows
    // -------------------------------------------------------------------------
    #[test]
    fn upsert_same_external_id_different_source_creates_two_rows() {
        let db = open_db();
        let repo = SessionRepository::new(db.clone());

        let a = NewSession {
            source: "a".to_string(),
            external_id: "ext-1".to_string(),
            cwd: None,
            started_at: 1_700_000_000,
            ended_at: None,
            metadata: None,
        };
        let b = NewSession {
            source: "b".to_string(),
            external_id: "ext-1".to_string(),
            cwd: None,
            started_at: 1_700_000_000,
            ended_at: None,
            metadata: None,
        };

        let sa = repo.upsert(a).expect("upsert a should succeed");
        let sb = repo.upsert(b).expect("upsert b should succeed");

        assert_ne!(sa.id, sb.id, "different sources must produce distinct rows");

        let conn = db.lock();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2, "two distinct session rows must exist");
    }

    // -------------------------------------------------------------------------
    // 4. find_by_source_and_external_id_hit
    // -------------------------------------------------------------------------
    #[test]
    fn find_by_source_and_external_id_hit() {
        let db = open_db();
        let repo = SessionRepository::new(db);

        let new = NewSession {
            source: "claude-code".to_string(),
            external_id: "ext-1".to_string(),
            cwd: Some("/tmp".to_string()),
            started_at: 1_700_000_000,
            ended_at: None,
            metadata: None,
        };
        let upserted = repo.upsert(new).expect("upsert should succeed");

        let found = repo
            .find_by_source_and_external_id("claude-code", "ext-1")
            .expect("find should not error");

        assert_eq!(found, Some(upserted));
    }

    // -------------------------------------------------------------------------
    // 5. find_by_source_and_external_id_miss_external_id
    // -------------------------------------------------------------------------
    #[test]
    fn find_by_source_and_external_id_miss_external_id() {
        let db = open_db();
        let repo = SessionRepository::new(db);

        let result = repo
            .find_by_source_and_external_id("claude-code", "missing")
            .expect("find on empty db should not error");

        assert_eq!(result, None);
    }

    // -------------------------------------------------------------------------
    // 6. find_by_source_and_external_id_miss_source
    // -------------------------------------------------------------------------
    #[test]
    fn find_by_source_and_external_id_miss_source() {
        let db = open_db();
        let repo = SessionRepository::new(db);

        let new = NewSession {
            source: "claude-code".to_string(),
            external_id: "ext-1".to_string(),
            cwd: None,
            started_at: 1_700_000_000,
            ended_at: None,
            metadata: None,
        };
        repo.upsert(new).expect("upsert should succeed");

        let result = repo
            .find_by_source_and_external_id("other", "ext-1")
            .expect("find should not error");

        assert_eq!(result, None);
    }

    // -------------------------------------------------------------------------
    // 7. find_by_id_hit
    // -------------------------------------------------------------------------
    #[test]
    fn find_by_id_hit() {
        let db = open_db();
        let repo = SessionRepository::new(db);

        let new = NewSession {
            source: "claude-code".to_string(),
            external_id: "ext-1".to_string(),
            cwd: Some("/tmp".to_string()),
            started_at: 1_700_000_000,
            ended_at: None,
            metadata: None,
        };
        let upserted = repo.upsert(new).expect("upsert should succeed");

        let found = repo
            .find_by_id(upserted.id)
            .expect("find_by_id should not error");

        assert_eq!(found, Some(upserted));
    }

    // -------------------------------------------------------------------------
    // 8. find_by_id_miss
    // -------------------------------------------------------------------------
    #[test]
    fn find_by_id_miss() {
        let db = open_db();
        let repo = SessionRepository::new(db);

        let result = repo
            .find_by_id(999)
            .expect("find_by_id on empty db should not error");

        assert_eq!(result, None);
    }
}
