//! Usage aggregation repository.
//!
//! Aggregations join `receipts` to `items` with `INNER JOIN` semantics, so
//! receipts that have no items are intentionally excluded from every result.
//! Token totals are usage tokens only: `input_tokens + output_tokens`. Cache
//! read/creation tokens are not included in aggregate totals or per-model
//! breakdowns.

use std::collections::HashSet;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::Database;

/// Per-model contribution within an aggregation bucket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelBreakdown {
    pub model: String,
    pub count: i64,
    pub cost: f64,
    pub tokens: i64,
}

/// Usage rollup for one date/week/month bucket.
///
/// `total_tokens` is `SUM(input_tokens + output_tokens)` only; cache read and
/// cache creation tokens are excluded. Buckets are produced from an `INNER JOIN`
/// between `receipts` and `items`, so receipts with zero items are excluded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregationBucket {
    pub bucket: String,
    pub receipt_count: i64,
    pub total_cost: f64,
    pub total_tokens: i64,
    pub model_breakdown: Vec<ModelBreakdown>,
}

/// Repository for usage rollups over receipts and items.
pub struct AggregationRepository {
    db: Database,
}

impl AggregationRepository {
    /// Create a new repository sharing the provided database handle.
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Return per-day usage buckets such as `2026-05-17`.
    ///
    /// Date bounds are inclusive and compared directly against `receipts.date`.
    /// Token totals are `input_tokens + output_tokens` only; cache tokens are
    /// excluded. Receipts with zero items are excluded by the `INNER JOIN`.
    pub fn aggregate_daily(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> anyhow::Result<Vec<AggregationBucket>> {
        self.aggregate("%Y-%m-%d", start_date, end_date, "aggregate_daily")
    }

    /// Return per-ISO-week usage buckets such as `2026-W20`.
    ///
    /// Date bounds are inclusive and compared directly against `receipts.date`.
    /// Token totals are `input_tokens + output_tokens` only; cache tokens are
    /// excluded. Receipts with zero items are excluded by the `INNER JOIN`.
    pub fn aggregate_weekly(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> anyhow::Result<Vec<AggregationBucket>> {
        self.aggregate("%G-W%V", start_date, end_date, "aggregate_weekly")
    }

    /// Return per-month usage buckets such as `2026-05`.
    ///
    /// Date bounds are inclusive and compared directly against `receipts.date`.
    /// Token totals are `input_tokens + output_tokens` only; cache tokens are
    /// excluded. Receipts with zero items are excluded by the `INNER JOIN`.
    pub fn aggregate_monthly(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> anyhow::Result<Vec<AggregationBucket>> {
        self.aggregate("%Y-%m", start_date, end_date, "aggregate_monthly")
    }

    fn aggregate(
        &self,
        fmt: &str,
        start_date: &str,
        end_date: &str,
        ctx_label: &str,
    ) -> anyhow::Result<Vec<AggregationBucket>> {
        let conn = self.db.lock();
        let sql = format!(
            "SELECT strftime('{fmt}', r.date) AS bucket, \
                    i.model, \
                    COUNT(DISTINCT r.id) AS receipt_count, \
                    SUM(i.cost) AS cost, \
                    SUM(i.input_tokens + i.output_tokens) AS tokens, \
                    GROUP_CONCAT(DISTINCT r.id) AS receipt_ids \
             FROM receipts r \
             INNER JOIN items i ON i.receipt_id = r.id \
             WHERE r.date BETWEEN ?1 AND ?2 \
             GROUP BY bucket, i.model \
             ORDER BY bucket ASC, i.model ASC"
        );

        let mut stmt = conn
            .prepare(&sql)
            .with_context(|| format!("{ctx_label}: prepare failed"))?;
        let rows = stmt
            .query_map(rusqlite::params![start_date, end_date], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    ModelBreakdown {
                        model: row.get(1)?,
                        count: row.get(2)?,
                        cost: row.get(3)?,
                        tokens: row.get(4)?,
                    },
                    row.get::<_, String>(5)?,
                ))
            })
            .with_context(|| format!("{ctx_label}: query failed"))?;

        let mut buckets: Vec<AggregationBucket> = Vec::new();
        let mut current_bucket: Option<String> = None;
        let mut receipt_ids = HashSet::new();

        for row in rows {
            let (bucket, breakdown, receipt_ids_csv) =
                row.with_context(|| format!("{ctx_label}: row failed"))?;

            if current_bucket.as_deref() != Some(bucket.as_str()) {
                if let Some(last) = buckets.last_mut() {
                    last.receipt_count = receipt_ids.len() as i64;
                }
                receipt_ids.clear();
                current_bucket = Some(bucket.clone());
                buckets.push(AggregationBucket {
                    bucket,
                    receipt_count: 0,
                    total_cost: 0.0,
                    total_tokens: 0,
                    model_breakdown: Vec::new(),
                });
            }

            for receipt_id in receipt_ids_csv.split(',') {
                let parsed = receipt_id
                    .parse::<i64>()
                    .with_context(|| format!("{ctx_label}: parse receipt id failed"))?;
                receipt_ids.insert(parsed);
            }

            let bucket = buckets
                .last_mut()
                .expect("a bucket is always present after bucket transition");
            bucket.total_cost += breakdown.cost;
            bucket.total_tokens += breakdown.tokens;
            bucket.model_breakdown.push(breakdown);
        }

        if let Some(last) = buckets.last_mut() {
            last.receipt_count = receipt_ids.len() as i64;
        }

        Ok(buckets)
    }
}
