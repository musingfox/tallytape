DROP VIEW IF EXISTS v_receipt_totals;
DROP TRIGGER IF EXISTS trg_receipts_updated_at;

CREATE TABLE receipts_new (
    id         INTEGER PRIMARY KEY,
    session_id INTEGER REFERENCES sessions(id) ON DELETE SET NULL,
    cwd        TEXT    NOT NULL,
    date       TEXT    NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(cwd, date)
);

INSERT INTO receipts_new (id, session_id, cwd, date, created_at, updated_at)
SELECT
    r.id,
    r.session_id,
    COALESCE(s.cwd, ''),
    date(s.started_at, 'unixepoch', 'localtime'),
    r.created_at,
    r.updated_at
FROM receipts r
JOIN sessions s ON s.id = r.session_id;

DROP TABLE receipts;
ALTER TABLE receipts_new RENAME TO receipts;

-- recreate 0002's trigger exactly
CREATE TRIGGER trg_receipts_updated_at
AFTER UPDATE ON receipts
FOR EACH ROW
WHEN NEW.updated_at = OLD.updated_at
BEGIN
    UPDATE receipts SET updated_at = unixepoch() WHERE id = NEW.id;
END;

CREATE VIEW v_receipt_totals AS
SELECT
    r.id                                        AS receipt_id,
    r.session_id,
    r.cwd,
    r.date                                      AS receipt_date,
    s.source                                    AS source,
    COUNT(i.id)                                 AS item_count,
    COALESCE(SUM(i.input_tokens), 0)            AS total_input_tokens,
    COALESCE(SUM(i.output_tokens), 0)           AS total_output_tokens,
    COALESCE(SUM(i.cache_read_tokens), 0)       AS total_cache_read_tokens,
    COALESCE(SUM(i.cache_creation_tokens), 0)   AS total_cache_creation_tokens,
    COALESCE(SUM(i.cost), 0)                    AS total_cost
FROM receipts r
LEFT JOIN sessions s ON s.id = r.session_id
LEFT JOIN items    i ON i.receipt_id = r.id
GROUP BY r.id;
