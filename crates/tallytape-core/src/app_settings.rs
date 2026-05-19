//! Key/value application settings repository.
//!
//! Provides last-write-wins scalar persistence for arbitrary string-keyed
//! settings that survive across restarts. Backed by the `app_settings` table
//! (migration 0004).

use anyhow::Context;
use rusqlite::OptionalExtension;

use crate::Database;

/// Persisted cursor: the largest `receipts.id` the UI has already
/// surfaced to the user (either via live arrival or boot catch-up).
pub const LAST_SEEN_RECEIPT_ID: &str = "last_seen_receipt_id";

/// Persisted cursor: the largest `receipts.updated_at` the UI has
/// already surfaced. Paired with [`LAST_SEEN_RECEIPT_ID`] so that an
/// updated old row (id ≤ cursor) still surfaces on next boot.
pub const LAST_SEEN_MAX_UPDATED_AT: &str = "last_seen_max_updated_at";

/// Repository for reading and writing application settings.
pub struct AppSettingsRepository {
    db: Database,
}

impl AppSettingsRepository {
    /// Create a new repository sharing the provided database handle.
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Return the value associated with `key`, or `None` if absent.
    ///
    /// # Errors
    ///
    /// Returns `Err` on lock poison or SQL failure.
    pub fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let conn = self.db.lock();
        conn.query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            rusqlite::params![key],
            |row| row.get(0),
        )
        .optional()
        .context("AppSettingsRepository::get: query failed")
    }

    /// Insert or overwrite the value for `key`.
    ///
    /// Uses `ON CONFLICT(key) DO UPDATE` (last-write-wins). Returns `Ok(())`
    /// on success.
    ///
    /// # Errors
    ///
    /// Returns `Err` on lock poison or SQL failure.
    pub fn set(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value],
        )
        .context("AppSettingsRepository::set: execute failed")?;
        Ok(())
    }

    /// Return the value associated with `key` parsed as `i64`, or `None`
    /// if the row is absent. Returns `Err` if the row is present but the
    /// stored value cannot be parsed.
    ///
    /// Distinguishing "absent" from "present with value 0" matters for
    /// the catch-up cursors: an absent cursor means the app has never
    /// booted before and should be seeded to the current top-of-table.
    pub fn get_i64(&self, key: &str) -> anyhow::Result<Option<i64>> {
        match self.get(key)? {
            None => Ok(None),
            Some(raw) => raw
                .parse::<i64>()
                .map(Some)
                .with_context(|| format!("AppSettingsRepository::get_i64: '{key}' = {raw:?}")),
        }
    }

    /// Insert `value` under `key` formatted as a decimal integer.
    pub fn set_i64(&self, key: &str, value: i64) -> anyhow::Result<()> {
        self.set(key, &value.to_string())
    }

    /// Atomically update the value at `key` to `max(existing, candidate)`.
    ///
    /// If no row exists at `key`, inserts `candidate` directly. If a row
    /// exists, keeps whichever of `existing` and `candidate` is larger
    /// (when both parse as `i64`). This is the single-statement guarantee
    /// behind the "cursors never roll backwards" invariant: concurrent
    /// callers can race without losing each other's progress.
    pub fn set_i64_max(&self, key: &str, candidate: i64) -> anyhow::Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = \
               CASE WHEN CAST(excluded.value AS INTEGER) > CAST(value AS INTEGER) \
                    THEN excluded.value \
                    ELSE value \
               END",
            rusqlite::params![key, candidate.to_string()],
        )
        .context("AppSettingsRepository::set_i64_max: execute failed")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_db() -> Database {
        Database::open(":memory:").expect("in-memory db should open")
    }

    #[test]
    fn get_returns_none_when_absent() {
        let repo = AppSettingsRepository::new(open_db());
        assert!(repo.get("missing").unwrap().is_none());
    }

    #[test]
    fn set_then_get_roundtrips_string() {
        let repo = AppSettingsRepository::new(open_db());
        repo.set("k", "hello").unwrap();
        assert_eq!(repo.get("k").unwrap().as_deref(), Some("hello"));
    }

    #[test]
    fn get_i64_distinguishes_absent_from_zero() {
        let repo = AppSettingsRepository::new(open_db());
        assert!(repo.get_i64("k").unwrap().is_none());
        repo.set_i64("k", 0).unwrap();
        assert_eq!(repo.get_i64("k").unwrap(), Some(0));
    }

    #[test]
    fn set_i64_max_keeps_larger_value() {
        let repo = AppSettingsRepository::new(open_db());
        repo.set_i64_max("k", 10).unwrap();
        assert_eq!(repo.get_i64("k").unwrap(), Some(10));
        // Smaller candidate must NOT overwrite.
        repo.set_i64_max("k", 3).unwrap();
        assert_eq!(repo.get_i64("k").unwrap(), Some(10));
        // Larger candidate must overwrite.
        repo.set_i64_max("k", 42).unwrap();
        assert_eq!(repo.get_i64("k").unwrap(), Some(42));
    }

    #[test]
    fn set_i64_max_inserts_when_absent() {
        let repo = AppSettingsRepository::new(open_db());
        repo.set_i64_max("k", 7).unwrap();
        assert_eq!(repo.get_i64("k").unwrap(), Some(7));
    }
}
