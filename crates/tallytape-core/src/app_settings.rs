//! Key/value application settings repository.
//!
//! Provides last-write-wins scalar persistence for arbitrary string-keyed
//! settings that survive across restarts. Backed by the `app_settings` table
//! (migration 0004).

use anyhow::Context;
use rusqlite::OptionalExtension;

use crate::Database;

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
}
