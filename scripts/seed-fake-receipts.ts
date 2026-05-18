#!/usr/bin/env bun
import { Database } from "bun:sqlite";
import { existsSync, mkdirSync } from "node:fs";
import { homedir, platform } from "node:os";
import { join } from "node:path";

const SOURCE = "claude_code";
const EXTERNAL_ID_PREFIX = "fake-seed-";

const MODELS = [
  "claude-opus-4-7",
  "claude-sonnet-4-6",
  "claude-haiku-4-5-20251001",
] as const;

const CWD_LABELS = [
  "alpha",
  "beta",
  "gamma",
  "long-project-path-to-check-truncation",
];

function resolveDbPath(): string {
  const home = homedir();
  switch (platform()) {
    case "darwin":
      return join(home, "Library/Application Support/tallytape/tallytape.sqlite");
    case "linux":
      return join(home, ".local/share/tallytape/tallytape.sqlite");
    case "win32":
      return join(process.env.APPDATA ?? home, "tallytape/tallytape.sqlite");
    default:
      throw new Error(`Unsupported platform: ${platform()}`);
  }
}

function openDb(path: string): Database {
  const dir = path.slice(0, path.lastIndexOf("/"));
  if (!existsSync(dir)) mkdirSync(dir, { recursive: true });
  const db = new Database(path);
  db.exec("PRAGMA journal_mode = WAL;");
  db.exec("PRAGMA foreign_keys = ON;");
  return db;
}

interface SeedRow {
  external_id: string;
  cwd: string;
  date: string;
  started_at: number;
  ended_at: number;
  model: string;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cost: number;
  request_id_a: string;
  request_id_b: string;
}

function isoLocalDate(epochSeconds: number): string {
  const d = new Date(epochSeconds * 1000);
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function buildRow(now: number, idx: number): SeedRow {
  const tag = `${now}-${idx}-${Math.random().toString(36).slice(2, 8)}`;
  const model = MODELS[idx % MODELS.length];
  // Unique cwd per insert (receipts UNIQUE on (cwd, date) — must avoid collision).
  const label = CWD_LABELS[idx % CWD_LABELS.length];
  const cwd = `/Users/dev/seed-demo/${label}-${idx}-${Math.random().toString(36).slice(2, 6)}`;
  const inputTokens = 5_000 + Math.floor(Math.random() * 25_000);
  const outputTokens = 800 + Math.floor(Math.random() * 4_000);
  const cacheRead = Math.floor(Math.random() * 30_000);
  const cost = Number((Math.random() * 2.4 + 0.05).toFixed(4));
  // Spread starts across the last ~3 days so the dashboard date filter has something to chew on.
  const startedAt = now - Math.floor(Math.random() * 3 * 86_400);
  return {
    external_id: `${EXTERNAL_ID_PREFIX}${tag}`,
    cwd,
    date: isoLocalDate(startedAt),
    started_at: startedAt,
    ended_at: startedAt + 60 + Math.floor(Math.random() * 1800),
    model,
    input_tokens: inputTokens,
    output_tokens: outputTokens,
    cache_read_tokens: cacheRead,
    cost,
    request_id_a: `req-${tag}-a`,
    request_id_b: `req-${tag}-b`,
  };
}

function insertReceipt(db: Database, row: SeedRow): number {
  const insertSession = db.prepare(
    `INSERT INTO sessions (source, external_id, cwd, started_at, ended_at, metadata)
     VALUES (?, ?, ?, ?, ?, NULL)`,
  );
  const insertReceiptStmt = db.prepare(
    `INSERT INTO receipts (session_id, cwd, date) VALUES (?, ?, ?)`,
  );
  const insertItem = db.prepare(
    `INSERT INTO items (
       receipt_id, session_id, source, request_id, message_id, parent_uuid,
       is_sidechain, occurred_at, model, service_tier,
       input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, cost, metadata
     ) VALUES (?, ?, ?, ?, ?, NULL, 0, ?, ?, 'standard', ?, ?, ?, 0, ?, NULL)`,
  );

  const tx = db.transaction(() => {
    const sessionResult = insertSession.run(
      SOURCE,
      row.external_id,
      row.cwd,
      row.started_at,
      row.ended_at,
    );
    const sessionId = Number(sessionResult.lastInsertRowid);

    const receiptResult = insertReceiptStmt.run(sessionId, row.cwd, row.date);
    const receiptId = Number(receiptResult.lastInsertRowid);

    const halfCost = Number((row.cost / 2).toFixed(4));
    insertItem.run(
      receiptId,
      sessionId,
      SOURCE,
      row.request_id_a,
      `msg-${row.external_id}-a`,
      row.started_at + 10,
      row.model,
      row.input_tokens,
      row.output_tokens,
      row.cache_read_tokens,
      halfCost,
    );
    insertItem.run(
      receiptId,
      sessionId,
      SOURCE,
      row.request_id_b,
      `msg-${row.external_id}-b`,
      row.started_at + 30,
      row.model,
      Math.floor(row.input_tokens / 2),
      Math.floor(row.output_tokens / 2),
      0,
      Number((row.cost - halfCost).toFixed(4)),
    );

    return receiptId;
  });

  return tx();
}

function cmdBulk(db: Database, n: number): void {
  const now = Math.floor(Date.now() / 1000);
  const start = performance.now();
  db.transaction(() => {
    for (let i = 0; i < n; i++) {
      insertReceipt(db, buildRow(now, i));
    }
  })();
  const ms = Math.round(performance.now() - start);
  console.log(`bulk: inserted ${n} receipts in ${ms}ms (single watcher fire expected)`);
}

async function cmdStream(db: Database, n: number, intervalMs: number): Promise<void> {
  console.log(`stream: ${n} receipts, ${intervalMs}ms apart — watch the drawer for slide-in`);
  const now = Math.floor(Date.now() / 1000);
  for (let i = 0; i < n; i++) {
    const id = insertReceipt(db, buildRow(now + i, i));
    console.log(`  [${i + 1}/${n}] inserted receipt id=${id}`);
    if (i < n - 1) await Bun.sleep(intervalMs);
  }
}

function cmdClear(db: Database): void {
  const sessionsResult = db
    .prepare(
      `SELECT id FROM sessions WHERE source = ? AND external_id LIKE ?`,
    )
    .all(SOURCE, `${EXTERNAL_ID_PREFIX}%`) as Array<{ id: number }>;

  if (sessionsResult.length === 0) {
    console.log("clear: no fake-seed rows found");
    return;
  }

  const ids = sessionsResult.map((r) => r.id);
  const placeholders = ids.map(() => "?").join(",");

  db.transaction(() => {
    db.prepare(`DELETE FROM items WHERE session_id IN (${placeholders})`).run(...ids);
    db.prepare(`DELETE FROM receipts WHERE session_id IN (${placeholders})`).run(...ids);
    db.prepare(`DELETE FROM sessions WHERE id IN (${placeholders})`).run(...ids);
  })();

  console.log(`clear: removed ${ids.length} fake-seed sessions (and their receipts/items)`);
}

function cmdStatus(db: Database): void {
  const fakeRow = db
    .prepare(
      `SELECT COUNT(*) AS n FROM sessions WHERE source = ? AND external_id LIKE ?`,
    )
    .get(SOURCE, `${EXTERNAL_ID_PREFIX}%`) as { n: number };
  const totalSessions = db.prepare(`SELECT COUNT(*) AS n FROM sessions`).get() as { n: number };
  const totalReceipts = db.prepare(`SELECT COUNT(*) AS n FROM receipts`).get() as { n: number };
  console.log(`db: ${resolveDbPath()}`);
  console.log(`  total sessions: ${totalSessions.n}`);
  console.log(`  total receipts: ${totalReceipts.n}`);
  console.log(`  fake-seed sessions: ${fakeRow.n}`);
}

function usage(): never {
  console.error(`Usage:
  bun run scripts/seed-fake-receipts.ts status
  bun run scripts/seed-fake-receipts.ts bulk <N>
  bun run scripts/seed-fake-receipts.ts stream <N> [--interval=<ms>]
  bun run scripts/seed-fake-receipts.ts clear

bulk    — insert N receipts in one transaction (single watcher event)
stream  — insert N receipts one at a time with delay (each fires receipt-added)
clear   — delete only rows tagged with external_id prefix "${EXTERNAL_ID_PREFIX}"
status  — print row counts including how many fake-seed rows exist`);
  process.exit(2);
}

const args = process.argv.slice(2);
const cmd = args[0];
if (!cmd) usage();

const dbPath = resolveDbPath();
if (!existsSync(dbPath) && cmd !== "status") {
  console.error(`db not found at ${dbPath} — run the app once to create it.`);
  process.exit(1);
}

const db = openDb(dbPath);
try {
  switch (cmd) {
    case "status":
      cmdStatus(db);
      break;
    case "clear":
      cmdClear(db);
      break;
    case "bulk": {
      const n = Number(args[1]);
      if (!Number.isFinite(n) || n <= 0) usage();
      cmdBulk(db, n);
      break;
    }
    case "stream": {
      const n = Number(args[1]);
      if (!Number.isFinite(n) || n <= 0) usage();
      const intervalArg = args.find((a) => a.startsWith("--interval="));
      const intervalMs = intervalArg ? Number(intervalArg.split("=")[1]) : 1000;
      if (!Number.isFinite(intervalMs) || intervalMs < 0) usage();
      await cmdStream(db, n, intervalMs);
      break;
    }
    default:
      usage();
  }
} finally {
  db.close();
}
