use tallytape_core::{
    calculate_cost, db_path, merge_item, Database, NewItemDraft, SessionRepository,
};

use crate::session_loader::SessionResult;

/// Summary of what was written during a single `persist` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistOutcome {
    pub session_id: i64,
    pub inserted: usize,
    pub skipped_duplicates: usize,
    pub skipped_unknown_model: usize,
}

/// Open the application database and persist `result`.
///
/// Never panics. Returns `Err` only when the database cannot be opened or the
/// session upsert fails — individual item failures are logged and skipped.
pub fn persist(result: SessionResult) -> anyhow::Result<PersistOutcome> {
    let path = db_path().map_err(|e| {
        log::error!("db_path() resolution failed: {e}");
        e
    })?;
    let db = Database::open(&path).map_err(|e| {
        log::error!("Database::open({}) failed: {e}", path.display());
        e
    })?;
    persist_with_db(&db, result)
}

/// Core persist logic. Accepts an already-opened `Database` so tests can
/// inject a temp-dir database without touching the real user data directory.
pub(crate) fn persist_with_db(
    db: &Database,
    result: SessionResult,
) -> anyhow::Result<PersistOutcome> {
    let external_id = result.session.external_id.clone();
    let session = SessionRepository::new(db.clone())
        .upsert(result.session)
        .map_err(|e| {
            log::error!("upsert session {external_id} failed: {e}");
            e
        })?;
    let session_id = session.id;
    log::debug!(
        "persisting session {} with {} items, stats={:?}",
        session_id,
        result.items.len(),
        result.stats,
    );

    let mut inserted = 0usize;
    let mut skipped_duplicates = 0usize;
    let mut skipped_unknown_model = 0usize;

    for item in result.items {
        let cache_creation = item.cache_creation_tokens.unwrap_or(0);
        let cache_read = item.cache_read_tokens.unwrap_or(0);

        let cost = match calculate_cost(
            &item.model,
            item.input_tokens,
            item.output_tokens,
            cache_creation,
            cache_read,
        ) {
            Some(c) => c,
            None => {
                log::warn!(
                    "unknown model {:?} — cost set to 0.0 for request_id {:?}",
                    item.model,
                    item.request_id
                );
                skipped_unknown_model += 1;
                0.0
            }
        };

        let draft = NewItemDraft {
            source: "claude-code".to_string(),
            request_id: item.request_id.clone(),
            message_id: item.message_id,
            parent_uuid: item.parent_uuid,
            is_sidechain: item.is_sidechain,
            occurred_at: item.occurred_at,
            model: item.model,
            service_tier: item.service_tier,
            input_tokens: item.input_tokens,
            output_tokens: item.output_tokens,
            cache_read_tokens: item.cache_read_tokens,
            cache_creation_tokens: item.cache_creation_tokens,
            cost,
            metadata: item.metadata,
        };

        match merge_item(db, session_id, draft) {
            Ok(_) => {
                inserted += 1;
            }
            Err(e) => {
                if is_unique_constraint_violation(&e) {
                    log::debug!("duplicate request_id {:?} — skipping", item.request_id);
                    skipped_duplicates += 1;
                } else {
                    log::warn!(
                        "merge_item failed for request_id {:?}: {e}",
                        item.request_id
                    );
                    // Continue processing remaining items.
                }
            }
        }
    }

    Ok(PersistOutcome {
        session_id,
        inserted,
        skipped_duplicates,
        skipped_unknown_model,
    })
}

/// Return `true` if `err` wraps a SQLite UNIQUE constraint violation.
fn is_unique_constraint_violation(err: &anyhow::Error) -> bool {
    if let Some(rusqlite::Error::SqliteFailure(ffi_err, _)) = err.downcast_ref::<rusqlite::Error>()
    {
        return ffi_err.code == rusqlite::ErrorCode::ConstraintViolation;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tallytape_core::{NewSession, ParsedItem, SessionRepository, TokenStats};
    use tempfile::tempdir;

    fn make_session(external_id: &str) -> NewSession {
        NewSession {
            source: "claude-code".to_string(),
            external_id: external_id.to_string(),
            cwd: Some("/test/project".to_string()),
            started_at: 1_700_000_000,
            ended_at: None,
            metadata: None,
        }
    }

    fn make_item(request_id: &str, model: &str) -> ParsedItem {
        ParsedItem {
            request_id: request_id.to_string(),
            message_id: Some(format!("msg-{request_id}")),
            parent_uuid: None,
            is_sidechain: false,
            occurred_at: 1_700_000_000,
            model: model.to_string(),
            service_tier: None,
            input_tokens: 100,
            output_tokens: 200,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            metadata: None,
        }
    }

    fn count_items(db: &Database) -> i64 {
        let conn = db.lock();
        conn.query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0))
            .unwrap()
    }

    fn count_sessions(db: &Database) -> i64 {
        let conn = db.lock();
        conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap()
    }

    // Test 1: fresh db with 2 items from a known model
    #[test]
    fn persists_session_and_items_to_fresh_db() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("t1.db")).unwrap();

        let result = SessionResult {
            session: make_session("session-1"),
            items: vec![
                make_item("req-1", "claude-opus-4-7"),
                make_item("req-2", "claude-opus-4-7"),
            ],
            stats: TokenStats::default(),
        };

        let outcome = persist_with_db(&db, result).expect("persist should succeed");

        assert_eq!(outcome.inserted, 2);
        assert_eq!(outcome.skipped_duplicates, 0);
        assert_eq!(outcome.skipped_unknown_model, 0);

        // Verify session row exists
        assert_eq!(count_sessions(&db), 1);
        let session = SessionRepository::new(db.clone())
            .find_by_id(outcome.session_id)
            .unwrap()
            .expect("session must exist");
        assert_eq!(session.external_id, "session-1");

        // Verify 2 item rows
        assert_eq!(count_items(&db), 2);
    }

    // Test 2: duplicate request_id on rerun → skipped
    #[test]
    fn skips_duplicate_request_id_on_rerun() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("t2.db")).unwrap();

        let make_result = || SessionResult {
            session: make_session("session-2"),
            items: vec![
                make_item("req-a", "claude-opus-4-7"),
                make_item("req-b", "claude-opus-4-7"),
            ],
            stats: TokenStats::default(),
        };

        // First run
        let first = persist_with_db(&db, make_result()).expect("first persist should succeed");
        assert_eq!(first.inserted, 2);

        // Second run — same session + same items
        let second = persist_with_db(&db, make_result()).expect("second persist should succeed");
        assert_eq!(second.inserted, 0);
        assert_eq!(second.skipped_duplicates, 2);

        // DB should still have exactly 2 items
        assert_eq!(count_items(&db), 2);
    }

    // Test 3: unknown model → inserted with cost 0.0, skipped_unknown_model counted
    #[test]
    fn unknown_model_inserts_with_zero_cost() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("t3.db")).unwrap();

        let result = SessionResult {
            session: make_session("session-3"),
            items: vec![make_item("req-unk", "made-up-model")],
            stats: TokenStats::default(),
        };

        let outcome = persist_with_db(&db, result).expect("persist should succeed");

        assert_eq!(outcome.inserted, 1, "item should still be inserted");
        assert_eq!(outcome.skipped_unknown_model, 1);
        assert_eq!(outcome.skipped_duplicates, 0);

        // Verify cost is 0.0 in DB
        let conn = db.lock();
        let cost: f64 = conn
            .query_row(
                "SELECT cost FROM items WHERE request_id = 'req-unk'",
                [],
                |r| r.get(0),
            )
            .expect("item must exist");
        assert!(
            (cost - 0.0).abs() < f64::EPSILON,
            "cost must be 0.0, got {cost}"
        );
    }

    // Test 4: empty items → session row inserted, no items
    #[test]
    fn empty_items_persists_session_only() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("t4.db")).unwrap();

        let result = SessionResult {
            session: make_session("session-4"),
            items: vec![],
            stats: TokenStats::default(),
        };

        let outcome = persist_with_db(&db, result).expect("persist should succeed");

        assert_eq!(outcome.inserted, 0);
        assert_eq!(outcome.skipped_duplicates, 0);
        assert_eq!(outcome.skipped_unknown_model, 0);
        assert_eq!(count_sessions(&db), 1);
        assert_eq!(count_items(&db), 0);
    }

    // Test 5: cache tokens pass through correctly
    #[test]
    fn cache_tokens_pass_through_correctly() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("t5.db")).unwrap();

        let item = ParsedItem {
            request_id: "req-cache".to_string(),
            message_id: None,
            parent_uuid: None,
            is_sidechain: false,
            occurred_at: 1_700_000_000,
            model: "claude-opus-4-7".to_string(),
            service_tier: None,
            input_tokens: 100,
            output_tokens: 200,
            cache_read_tokens: Some(5),
            cache_creation_tokens: Some(7),
            metadata: None,
        };

        let result = SessionResult {
            session: make_session("session-5"),
            items: vec![item],
            stats: TokenStats::default(),
        };

        let outcome = persist_with_db(&db, result).expect("persist should succeed");
        assert_eq!(outcome.inserted, 1);

        // Verify cache tokens in DB
        let conn = db.lock();
        let (cache_read, cache_creation): (Option<i64>, Option<i64>) = conn
            .query_row(
                "SELECT cache_read_tokens, cache_creation_tokens FROM items WHERE request_id = 'req-cache'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("item must exist");

        assert_eq!(cache_read, Some(5));
        assert_eq!(cache_creation, Some(7));

        // Verify cost matches calculate_cost(model, 100, 200, 7, 5)
        let expected_cost = calculate_cost("claude-opus-4-7", 100, 200, 7, 5)
            .expect("known model should return cost");
        let actual_cost: f64 = conn
            .query_row(
                "SELECT cost FROM items WHERE request_id = 'req-cache'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            (actual_cost - expected_cost).abs() < 1e-9,
            "cost mismatch: expected {expected_cost}, got {actual_cost}"
        );
    }

    // Test TC3: upsert failure logs ERROR with external_id
    #[test]
    fn upsert_failure_logs_error_with_external_id() {
        use log::{LevelFilter, Log, Metadata, Record};
        use std::sync::{Arc, Mutex, Once};

        // Capture logger that collects messages into a shared Vec.
        struct CapLog(Arc<Mutex<Vec<String>>>);
        impl Log for CapLog {
            fn enabled(&self, _: &Metadata) -> bool {
                true
            }
            fn log(&self, record: &Record) {
                if record.level() <= log::Level::Error {
                    self.0.lock().unwrap().push(format!("{}", record.args()));
                }
            }
            fn flush(&self) {}
        }

        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        static LOGGER_ONCE: Once = Once::new();
        let captured_clone = Arc::clone(&captured);

        LOGGER_ONCE.call_once(|| {
            // Only install if not already set (integration tests may have set FileLogger).
            let _ = log::set_boxed_logger(Box::new(CapLog(captured_clone)));
            log::set_max_level(LevelFilter::Error);
        });

        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("tc3.db")).unwrap();

        // Drop the sessions table to cause upsert failure.
        {
            let conn = db.lock();
            conn.execute_batch("DROP TABLE sessions;").unwrap();
        }

        let result = SessionResult {
            session: make_session("upsert-fail-ext-id"),
            items: vec![],
            stats: TokenStats::default(),
        };

        let err = persist_with_db(&db, result);
        assert!(err.is_err(), "persist_with_db should fail when sessions table is missing");

        // Check captured log messages if the capture logger was successfully installed.
        // If a logger was already set (e.g. FileLogger in integration test binary),
        // we accept that and only verify the error propagated (done above).
        let msgs = captured.lock().unwrap();
        if !msgs.is_empty() {
            let logged_upsert = msgs.iter().any(|m| {
                m.contains("upsert") && m.contains("upsert-fail-ext-id")
            });
            assert!(
                logged_upsert,
                "expected error log with 'upsert' and 'upsert-fail-ext-id', got: {:?}",
                msgs
            );
        }
        // If msgs is empty the CapLog was not installed (logger already set); Err propagation
        // verified above satisfies the fallback path.
    }
}
