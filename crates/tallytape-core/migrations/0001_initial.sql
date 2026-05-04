-- Migration: 0001_initial
-- Task:      p1-1-sqlite-schema
--
-- WAL mode (PRAGMA journal_mode=WAL) and foreign key enforcement
-- (PRAGMA foreign_keys=ON) are connection-level concerns set by the
-- writer crate at open time — they are NOT applied here.
--
-- Schema source of truth:
--   pm/tallytape/docs/tallytape-sqlite-schema-v1-dbml.md (v3)

-- ============================================================
-- TABLE: sessions
-- No foreign keys; must be created first.
-- ============================================================
CREATE TABLE sessions (
    id          INTEGER PRIMARY KEY, -- rowid alias; no AUTOINCREMENT needed
    source      TEXT    NOT NULL,    -- Provider name: 'claude_code', 'codex', 'opencode', ...
    external_id TEXT    NOT NULL,    -- Upstream session id (Claude Code: sessionId UUID)
    cwd         TEXT,                -- Working directory at session start; nullable for non-cwd-scoped sources
    started_at  INTEGER NOT NULL,    -- UNIX epoch seconds; first observed activity
    ended_at    INTEGER,             -- UNIX epoch seconds; last observed activity (null while open)
    metadata    TEXT                 -- Provider-specific JSON: gitBranch, version, entrypoint, userType, etc.
);

-- ============================================================
-- TABLE: receipts
-- FK → sessions (ON DELETE RESTRICT)
-- ============================================================
CREATE TABLE receipts (
    id         INTEGER PRIMARY KEY,                  -- rowid alias
    session_id INTEGER NOT NULL UNIQUE,              -- FK → sessions.id; UNIQUE = 1:1 today
    created_at INTEGER NOT NULL DEFAULT (unixepoch()), -- UNIX epoch seconds; set at insert
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()), -- UNIX epoch seconds; updated on write
    FOREIGN KEY (session_id) REFERENCES sessions (id) ON DELETE RESTRICT
);

-- ============================================================
-- TABLE: items
-- FK → receipts (ON DELETE CASCADE), sessions (ON DELETE RESTRICT)
-- ============================================================
CREATE TABLE items (
    id                    INTEGER PRIMARY KEY,                   -- rowid alias
    receipt_id            INTEGER NOT NULL,                      -- FK → receipts.id ON DELETE CASCADE
    session_id            INTEGER NOT NULL,                      -- FK → sessions.id; redundant today, kept for re-bucket flexibility
    source                TEXT    NOT NULL,                      -- Denormalized from session for the (source, request_id) unique key
    request_id            TEXT    NOT NULL,                      -- Anthropic 'req_xxx' / equivalent. Idempotency key for re-import.
    message_id            TEXT,                                  -- Anthropic message.id — alternative billing key
    parent_uuid           TEXT,                                  -- Parent message uuid — lets us reconstruct turn trees in queries
    is_sidechain          INTEGER NOT NULL DEFAULT 0,            -- 1 if Task / sub-agent call (Claude Code: isSidechain)
    occurred_at           INTEGER NOT NULL,                      -- UNIX epoch seconds (from message timestamp)
    model                 TEXT    NOT NULL,                      -- One model per item by definition (one API request = one model)
    service_tier          TEXT,                                  -- 'standard' / 'priority' / 'batch' — drives pricing lookup
    input_tokens          INTEGER NOT NULL DEFAULT 0,            -- Billable input token count
    output_tokens         INTEGER NOT NULL DEFAULT 0,            -- Billable output token count
    cache_read_tokens     INTEGER,                               -- Anthropic: cache_read_input_tokens. Nullable for providers without caching.
    cache_creation_tokens INTEGER,                               -- Anthropic: cache_creation_input_tokens. Sum of TTL buckets — breakdown lives in metadata.
    cost                  REAL    NOT NULL DEFAULT 0,            -- USD; computed by writer at insert via pricing lookup
    metadata              TEXT,                                  -- Provider-specific JSON: stop_reason, inference_geo, cache TTL breakdown, etc.
    FOREIGN KEY (receipt_id) REFERENCES receipts (id) ON DELETE CASCADE,
    FOREIGN KEY (session_id) REFERENCES sessions (id) ON DELETE RESTRICT
);

-- ============================================================
-- TABLE: pricing
-- No foreign keys.
-- ============================================================
CREATE TABLE pricing (
    id                      INTEGER PRIMARY KEY,                   -- rowid alias
    source                  TEXT    NOT NULL,                      -- Provider name matching sessions.source
    model                   TEXT    NOT NULL,                      -- Model identifier string
    service_tier            TEXT    NOT NULL DEFAULT 'standard',   -- 'standard' / 'priority' / 'batch'
    effective_from          INTEGER NOT NULL,                      -- UNIX epoch seconds; inclusive
    effective_to            INTEGER,                               -- UNIX epoch seconds; exclusive. NULL = currently in effect.
    input_per_mtok          REAL    NOT NULL,                      -- USD per 1,000,000 input tokens
    output_per_mtok         REAL    NOT NULL,                      -- USD per 1,000,000 output tokens
    cache_read_per_mtok     REAL,                                  -- NULL when provider has no caching
    cache_creation_per_mtok REAL,                                  -- NULL when provider has no caching
    currency                TEXT    NOT NULL DEFAULT 'USD',        -- ISO 4217 currency code
    created_at              INTEGER NOT NULL DEFAULT (unixepoch()) -- UNIX epoch seconds; set at insert
);

-- ============================================================
-- INDEXES: sessions
-- ============================================================
CREATE UNIQUE INDEX ux_sessions_source_external_id ON sessions (source, external_id);
CREATE INDEX ix_sessions_cwd        ON sessions (cwd);
CREATE INDEX ix_sessions_started_at ON sessions (started_at);

-- ============================================================
-- INDEXES: items
-- ============================================================
CREATE UNIQUE INDEX ux_items_source_request_id ON items (source, request_id);
CREATE INDEX ix_items_receipt_id    ON items (receipt_id);
CREATE INDEX ix_items_session_id    ON items (session_id);
CREATE INDEX ix_items_session_time  ON items (session_id, occurred_at);
CREATE INDEX ix_items_occurred_at   ON items (occurred_at);
CREATE INDEX ix_items_parent_uuid   ON items (parent_uuid);

-- ============================================================
-- INDEXES: pricing
-- ============================================================
CREATE UNIQUE INDEX ux_pricing_lookup_key  ON pricing (source, model, service_tier, effective_from);
CREATE INDEX ix_pricing_active_lookup ON pricing (source, model, service_tier);

-- ============================================================
-- VIEW: v_receipt_totals
-- Single source of truth for per-receipt aggregated token/cost totals.
-- ============================================================
CREATE VIEW v_receipt_totals AS
SELECT
    r.id                                        AS receipt_id,
    r.session_id,
    s.source,
    s.cwd,
    date(s.started_at, 'unixepoch')             AS started_date,
    COUNT(i.id)                                 AS item_count,
    COALESCE(SUM(i.input_tokens), 0)            AS total_input_tokens,
    COALESCE(SUM(i.output_tokens), 0)           AS total_output_tokens,
    COALESCE(SUM(i.cache_read_tokens), 0)       AS total_cache_read_tokens,
    COALESCE(SUM(i.cache_creation_tokens), 0)   AS total_cache_creation_tokens,
    COALESCE(SUM(i.cost), 0)                    AS total_cost
FROM receipts r
JOIN sessions s ON s.id = r.session_id
LEFT JOIN items i ON i.receipt_id = r.id
GROUP BY r.id;
