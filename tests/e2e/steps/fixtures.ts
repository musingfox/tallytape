import { Database } from "bun:sqlite";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test as base, createBdd } from "playwright-bdd";

const MIGRATION_FILES = [
  "0001_initial.sql",
  "0002_receipts_updated_at_trigger.sql",
  "0003_receipts_cwd_date.sql",
] as const;

function openFakeDb(): { db: Database; dir: string } {
  const dir = mkdtempSync(join(tmpdir(), "tt-e2e-"));
  const db = new Database(join(dir, "tallytape.sqlite"));
  db.exec("PRAGMA journal_mode = WAL;");
  db.exec("PRAGMA foreign_keys = ON;");
  for (const file of MIGRATION_FILES) {
    db.exec(readFileSync(join("crates/tallytape-core/migrations", file), "utf8"));
  }
  return { db, dir };
}

interface FakeDb {
  insertReceipt(cwd: string, date: string): number;
  readReceipt(id: number): {
    id: number;
    sessionId: number | null;
    cwd: string;
    date: string;
    createdAt: number;
    updatedAt: number;
  };
}

interface ArrivalRef {
  cwd: string | null;
}

interface Bag {
  fakeDb: FakeDb;
  arrival: ArrivalRef;
  _raw: { db: Database; dir: string };
}

export const test = base.extend<Bag>({
  _raw: async ({}, use) => {
    const opened = openFakeDb();
    await use(opened);
    opened.db.close();
    rmSync(opened.dir, { recursive: true, force: true });
  },
  fakeDb: async ({ _raw, page }, use) => {
    const { db } = _raw;

    const insertReceipt = (cwd: string, date: string): number => {
      const result = db
        .prepare("INSERT INTO receipts (cwd, date) VALUES (?, ?)")
        .run(cwd, date);
      return Number(result.lastInsertRowid);
    };

    const readReceipt = (id: number) => {
      const row = db
        .prepare(
          "SELECT id, session_id, cwd, date, created_at, updated_at FROM receipts WHERE id = ?",
        )
        .get(id) as {
        id: number;
        session_id: number | null;
        cwd: string;
        date: string;
        created_at: number;
        updated_at: number;
      };
      return {
        id: row.id,
        sessionId: row.session_id,
        cwd: row.cwd,
        date: row.date,
        createdAt: row.created_at,
        updatedAt: row.updated_at,
      };
    };

    // Bridge from the page's mocked `__TAURI_INTERNALS__.invoke` to the real
    // SQLite query in Node-land. This is the "fake-db, real React" surface.
    await page.exposeBinding(
      "__fakeIpc",
      async (_source, cmd: string, args: unknown) => {
        switch (cmd) {
          case "list_receipts": {
            const rows = db
              .prepare(
                "SELECT id, session_id, cwd, date, created_at, updated_at " +
                  "FROM receipts ORDER BY id DESC",
              )
              .all() as Array<{
              id: number;
              session_id: number | null;
              cwd: string;
              date: string;
              created_at: number;
              updated_at: number;
            }>;
            return rows.map((r) => ({
              id: r.id,
              sessionId: r.session_id,
              cwd: r.cwd,
              date: r.date,
              createdAt: r.created_at,
              updatedAt: r.updated_at,
            }));
          }
          case "list_receipt_summaries": {
            const rows = db
              .prepare(
                "SELECT receipt_id, item_count, total_cost FROM v_receipt_totals",
              )
              .all() as Array<{
              receipt_id: number;
              item_count: number;
              total_cost: number;
            }>;
            return rows.map((r) => ({
              receiptId: r.receipt_id,
              itemCount: r.item_count,
              totalCost: r.total_cost,
            }));
          }
          case "get_summary": {
            const row = db
              .prepare(
                "SELECT " +
                  "COALESCE(SUM(total_cost), 0) AS total_cost, " +
                  "COALESCE(SUM(total_input_tokens + total_output_tokens), 0) AS total_tokens, " +
                  "COUNT(DISTINCT session_id) AS session_count, " +
                  "COUNT(*) AS receipt_count " +
                  "FROM v_receipt_totals",
              )
              .get() as {
              total_cost: number;
              total_tokens: number;
              session_count: number;
              receipt_count: number;
            };
            return {
              totalCost: row.total_cost,
              totalTokens: row.total_tokens,
              sessionCount: row.session_count,
              receiptCount: row.receipt_count,
            };
          }
          // Event channels are handled page-side in __TAURI_INTERNALS__.invoke
          // (see addInitScript below) — we should never reach __fakeIpc for them.
          default: {
            const argsRepr = JSON.stringify(args ?? null);
            throw new Error(
              `[fakeIpc] unhandled IPC command: ${cmd} (args=${argsRepr})`,
            );
          }
        }
      },
    );

    // Install the Tauri shim before any app code runs. Mirrors how
    // `@tauri-apps/api/core`'s `invoke` resolves through window globals.
    await page.addInitScript(() => {
      type Cb = (arg: unknown) => void;
      const w = window as unknown as {
        __TAURI_INTERNALS__: {
          invoke: (cmd: string, args?: unknown) => Promise<unknown>;
          transformCallback: (fn: Cb, once?: boolean) => number;
        };
        __fakeIpc: (cmd: string, args: unknown) => Promise<unknown>;
        __fireEvent: (event: string, payload: unknown) => number;
      };
      const callbacks: Record<number, Cb> = {};
      const listeners: Record<string, Set<number>> = {};
      let nextCbId = 0;
      let nextListenId = 1;
      let nextEventId = 1;

      w.__TAURI_INTERNALS__ = {
        invoke: async (cmd, args) => {
          if (cmd === "plugin:event|listen") {
            const a = args as { event: string; handler: number };
            (listeners[a.event] ??= new Set()).add(a.handler);
            return nextListenId++;
          }
          if (cmd === "plugin:event|unlisten") {
            // Caller doesn't tell us *which* listener; tests don't depend on
            // surgical unlisten so we accept the call as a no-op.
            return null;
          }
          return await w.__fakeIpc(cmd, args ?? null);
        },
        transformCallback: (fn, once = false) => {
          const id = ++nextCbId;
          callbacks[id] = (arg) => {
            if (once) delete callbacks[id];
            fn(arg);
          };
          return id;
        },
      };

      w.__fireEvent = (event, payload) => {
        const id = nextEventId++;
        for (const cbId of listeners[event] ?? new Set<number>()) {
          callbacks[cbId]?.({ event, payload, id });
        }
        return id;
      };
    });

    await use({ insertReceipt, readReceipt });
  },
  arrival: async ({}, use) => {
    const ref: ArrivalRef = { cwd: null };
    await use(ref);
  },
});

export const { Given, When, Then } = createBdd(test);
