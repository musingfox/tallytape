//! Range summary repository.
//!
//! Summaries join `receipts` to `items` with `INNER JOIN` semantics, matching
//! aggregation behavior: receipts that have no items are excluded from every
//! metric. Token totals are usage tokens only: `input_tokens + output_tokens`.

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::Database;

/// Usage summary for an inclusive receipt date range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeSummary {
    pub total_cost: f64,
    pub total_tokens: i64,
    pub session_count: i64,
    pub receipt_count: i64,
}

/// Repository for range-scoped usage summaries over receipts and items.
pub struct SummaryRepository {
    db: Database,
}

impl SummaryRepository {
    /// Create a new repository sharing the provided database handle.
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Return usage totals for receipts dated inclusively between `start_date` and `end_date`.
    ///
    /// Counts use `COUNT(DISTINCT ...)` to remain stable when a receipt has multiple items.
    /// `COUNT(DISTINCT r.session_id)` excludes NULL session ids by SQLite semantics.
    pub fn summarize(&self, start_date: &str, end_date: &str) -> anyhow::Result<RangeSummary> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare(
                "SELECT COALESCE(SUM(i.cost), 0.0) AS total_cost, \
                        COALESCE(SUM(i.input_tokens + i.output_tokens), 0) AS total_tokens, \
                        COUNT(DISTINCT r.session_id) AS session_count, \
                        COUNT(DISTINCT r.id) AS receipt_count \
                 FROM receipts r \
                 INNER JOIN items i ON i.receipt_id = r.id \
                 WHERE r.date BETWEEN ?1 AND ?2",
            )
            .context("summarize: prepare failed")?;

        stmt.query_row(rusqlite::params![start_date, end_date], |row| {
            Ok(RangeSummary {
                total_cost: row.get(0)?,
                total_tokens: row.get(1)?,
                session_count: row.get(2)?,
                receipt_count: row.get(3)?,
            })
        })
        .context("summarize: query failed")
    }
}
