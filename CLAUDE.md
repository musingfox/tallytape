# tallytape — onboarding guide

Local-first desktop app that records Claude Code session costs. A detached writer binary ingests `SessionEnd` hook payloads into SQLite; a Tauri 2 desktop UI queries that DB and renders dashboards/receipts. Architecture rationale lives in `docs/adr/0001-foundational-architecture.md`.

## 1. Project goal

Capture every Claude Code session (cost, tokens, model breakdown, cwd) into a local SQLite store and expose it through a desktop dashboard so users can audit spend per project / period without any cloud round-trip. Capture must keep working when the UI is closed or crashes.

## 2. Tech stack

| Layer | Tech |
|---|---|
| Desktop shell | Tauri 2 (Rust + system WebView) |
| Frontend | React 19 + TypeScript 5.8 + Vite 7 |
| Routing | TanStack Router (file-based, generated tree) |
| State | Zustand (single store) |
| Styling | Tailwind CSS v4 (CSS-first config) |
| Backend | Rust (stable toolchain), SQLite via `rusqlite` + `rusqlite_migration`, WAL mode |
| File watching | `notify` crate (100ms debounce) → Tauri events |
| Hook ingest | Detached `tallytape-writer` binary spawned by Claude Code's `SessionEnd` hook |
| Tests | Vitest + RTL + jsdom (frontend); `cargo test` (Rust) |
| Lint/Format | ESLint (zero-warning), Prettier, `cargo fmt`, `clippy -D warnings` |
| JS package manager | **`bun` only** — lock file is `bun.lock` |

## 3. System architecture

```
Claude Code SessionEnd hook
        │ (JSON payload via stdin)
        ▼
tallytape-writer (detached binary, spawns __worker child)
        │  parse → load session metadata → merge by Claude Code session_id
        ▼
SQLite (WAL mode)  ◄── single source of truth
        │  rusqlite_migration applies migrations on open
        ▼
tallytape-app (Tauri)
   ├── notify watcher: emits `receipt-added` / `receipt-updated` events
   └── 6 #[tauri::command] IPC handlers
        ▼
src/ipc/*  →  Zustand store  →  React routes/components
```

**Merge key**: Claude Code's own `session_id`. tallytape never invents a session boundary — entries with the same `session_id` collapse into one logical receipt (`session_id` + `cwd` co-vary because Claude Code scopes sessions per working directory).

## 4. Workspace layout

```
tallytape/
├── src/                       React + TS frontend (Tauri WebView)
│   ├── routes/                TanStack file-based routes (__root, index, dashboard, receipts/$id)
│   ├── components/            CalendarHeatmap, DashboardCards, DateRangePicker, Toaster, ...
│   ├── receipts/              Zustand store, types, useReceiptEvents hook
│   ├── ipc/                   Typed wrappers around tauri `invoke` (commands.ts + types.ts)
│   ├── lib/                   Pure helpers (dateRange.ts, ...)
│   ├── test/setup.ts          Vitest global setup (jest-dom matchers + cleanup)
│   └── routeTree.gen.ts       AUTO-GENERATED — do not edit
├── src-tauri/                 Tauri app crate (tallytape-app)
│   ├── src/lib.rs             #[tauri::command] handlers, file watcher, event emitters
│   ├── tauri.conf.json        Window/CSP/bundle config
│   └── Cargo.toml             Wraps tallytape-core
├── crates/
│   ├── tallytape-core/        DB + repos + migrations (library, depended on by app + writer)
│   │   ├── src/               Receipt/Item/Session/Aggregation/Summary repos, schema
│   │   ├── migrations/        SQL files — authoritative schema (see §8)
│   │   └── tests/             Integration tests against real SQLite
│   └── tallytape-writer/      Detached ingest binary
│       ├── src/main.rs        CLI: install-hook | __worker subcommands
│       └── tests/             Hook parsing, latency, install-hook output, regression fixtures
├── docs/adr/                  Architecture Decision Records (MADR 3.0)
├── .github/workflows/ci.yml   Lint + typecheck only (tests run locally)
├── .githooks/                 Local pre-commit (secrets, file size, no-commit markers)
└── public/                    Static assets
```

## 5. Common commands

Run from repo root unless noted.

| Task | Command |
|---|---|
| Install JS deps | `bun install` |
| Run desktop app (dev) | `bun run tauri dev` |
| Build desktop app | `bun run tauri build` (also runs `bun run build` for frontend) |
| Frontend tests (run once) | `bun run test` |
| Frontend tests (watch) | `bun run test:watch` |
| Frontend typecheck | `bun run typecheck` |
| Frontend lint | `bun run lint` (zero warnings tolerated) |
| Frontend format | `bun run format` / `bun run format:check` |
| Rust tests | `cargo test --workspace` |
| Rust lint | `cargo clippy --all-targets --all-features -- -D warnings` |
| Rust fmt | `cargo fmt --all` |
| Rust branch coverage gate | `bun run coverage:rust:branch` (requires `cargo +nightly`; threshold ≥65%) |
| Build writer only | `cargo build --release -p tallytape-writer` |
| Install hook (after build) | `./target/release/tallytape-writer install-hook` → merge JSON into `~/.claude/settings.json` |

**Never** run `npm install`, `yarn`, or `pnpm` — they generate the wrong lock file and CI's `bun install --frozen-lockfile` will reject the PR.

## 6. IPC surface

All commands live in `src-tauri/src/lib.rs` (`#[tauri::command]`) and are wrapped by `src/ipc/commands.ts`. Errors are returned as `{ message: string }`. Frontend code must **never** call `invoke` directly outside `src/ipc/`.

| Command | Args | Returns |
|---|---|---|
| `list_receipts` | `{ dateRange: DateRange \| null }` | `Receipt[]` |
| `get_receipt` | `{ id: number }` | `Receipt` |
| `list_items_by_receipt` | `{ receiptId: number }` | `Item[]` |
| `list_receipt_summaries` | _(none)_ | `ReceiptSummary[]` (aggregated; collapses N+1) |
| `get_aggregation` | `{ granularity: 'daily'\|'weekly'\|'monthly', dateRange: DateRange }` | `AggregationBucket[]` |
| `get_summary` | `{ dateRange: DateRange }` | `{ totalCost, totalTokens, sessionCount, receiptCount }` |

Live updates flow via Tauri events: the watcher emits `receipt-added` / `receipt-updated` → `src/receipts/useReceiptEvents.ts` listens → store refreshes.

## 7. Testing standards

- Every module under `src/` has a co-located `__tests__/` directory; follow the same pattern when adding new behaviour. Vitest + Testing Library + jsdom.
- Mock the Tauri boundary: `vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))` and `vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn().mockResolvedValue(() => {}) }))`. Pattern is in `src/routes/__tests__/dashboard.test.tsx`.
- **Timers**: prefer `vi.setSystemTime(...)` for date-only fakes. `vi.useFakeTimers()` leaks across files under parallel runs — if you must use it (e.g. `Toaster.test.tsx` for auto-dismiss), pair with `afterEach(() => vi.useRealTimers())` *and* prepend `vi.useRealTimers()` to other files' `beforeEach`.
- **Rust integration tests** live in each crate's `tests/` dir. `tallytape-writer` carries the highest test density (hook parsing, latency, install-hook JSON, regression fixtures, reingest lifecycle, worker ingest). Use `tempfile` for DB scratch dirs — never touch the user's real data directory.
- **Coverage gate** on Rust: `bun run coverage:rust:branch` enforces ≥65% branch coverage on the workspace via `cargo +nightly llvm-cov`. Run before opening a PR that touches Rust.
- **Contract-first**: when adding an IPC command, write the test cases (DTO shape, edge cases) before the handler.
- Never suppress lint/clippy warnings (no `#[allow]`, no `eslint-disable`); fix the root cause. Never skip git hooks (`--no-verify`).

## 8. Migrations & schema

- Migrations live at `crates/tallytape-core/migrations/*.sql` and are **the authoritative schema** — DBML / design docs are snapshots, not rolling truth.
- Numbered + applied automatically by `rusqlite_migration` on DB open.
- To add a migration: drop a new `NNNN_<slug>.sql` (next number), include only forward SQL, write a Rust integration test in `crates/tallytape-core/tests/repo_integration.rs` exercising the new columns/indexes.
- Storage path resolved via `directories` crate (platform-standard data dir); WAL mode on, foreign keys on.

## 9. Quality gates

**Local pre-commit (`.githooks/pre-commit`)** — runs on every commit unless `CLAUDECODE=1`:
- secret detection (gitleaks)
- file size cap, line endings, EOF newline, trailing whitespace
- no-commit / FIXME / DO-NOT-COMMIT markers
- lock file consistency

**CI (`.github/workflows/ci.yml`)** — runs on push to main + every PR:
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `bun install --frozen-lockfile`
- `bun run typecheck` / `lint` / `format:check`

Note CI does **not** run tests; tests are run locally and on release. Don't merge without running `bun run test` + `cargo test --workspace` yourself.

## 10. Things not to "fix"

- `routeTree.gen.ts` — regenerate via TanStack plugin, never hand-edit.
- `bun.lock` — keep it; do not introduce a `package-lock.json` / `yarn.lock` / `pnpm-lock.yaml`. If you see one in a diff, delete it.
- ADRs under `docs/adr/` — append new ADRs; do not retroactively edit accepted ones.
- Migrations that are already merged — never edit in place; write a new migration.
- Test fixtures under `crates/tallytape-writer/tests/fixtures/` — these encode real hook payloads; replace only when fixing a captured regression.

## 11. Where decisions live

- Foundational architecture (writer ↔ SQLite ↔ Tauri, session-merge rule): `docs/adr/0001-foundational-architecture.md`
- Project management: GitHub Project v2 at `musingfox/projects/2`; issues at `musingfox/tallytape` (tasks moved off Obsidian on 2026-05-16). Use `/pm` slash command for PM ops.
- Schema truth: `crates/tallytape-core/migrations/`.
