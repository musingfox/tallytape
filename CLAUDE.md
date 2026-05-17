# tallytape — agent guide

Tauri 2 desktop app. Frontend `src/` (React 19 + TS + TanStack Router + Tailwind v4 + Zustand). Backend `src-tauri/` (Rust). Records Claude Code session costs via `SessionEnd` hooks.

## Tooling — non-negotiable

- **JS/TS package manager: `bun`.** Lock file is `bun.lock`. **Never run `npm install` / `yarn` / `pnpm`** — they will generate the wrong lock file and CI's `bun install --frozen-lockfile` will reject the PR.
- Rust: `cargo` (stable toolchain). Workspace at `src-tauri/Cargo.toml`.
- Tests run locally; CI only runs lint + typecheck + clippy + fmt (see `.github/workflows/ci.yml`).

## Common commands

| Task | Command |
|---|---|
| Install deps | `bun install` |
| Run dev (Tauri) | `bun run tauri dev` |
| Frontend tests | `bun run test` (vitest run) — or `npm test` if you really must (same script) |
| Watch tests | `bun run test:watch` |
| Typecheck | `bun run typecheck` |
| Lint | `bun run lint` (zero-warning policy) |
| Format | `bun run format` / `bun run format:check` |
| Rust tests | `cargo test --workspace` (run from repo root or `src-tauri/`) |
| Rust lint | `cargo clippy --all-targets --all-features -- -D warnings` |

## Architecture pointers

- **IPC**: Rust `#[tauri::command]` in `src-tauri/src/`, mirrored DTOs + wrappers in `src/ipc/`. Frontend never calls `invoke` directly outside `src/ipc/`.
- **Store**: single Zustand store at `src/receipts/store.ts` (receipts Map, errors, dateFilter, load status).
- **Routing**: TanStack Router file-based — routes in `src/routes/`. `routeTree.gen.ts` is auto-generated; do not edit.
- **Schema truth**: SQL migrations in `src-tauri/migrations/` are authoritative. DBML is a v1 design snapshot, not rolling.

## Testing conventions

- Frontend tests mock `@tauri-apps/api/core` (`invoke`) and `@tauri-apps/api/event` (`listen`). See `src/routes/__tests__/dashboard.test.tsx` for the pattern.
- `vi.useFakeTimers` causes cross-file pollution under parallel runs — prefer `vi.setSystemTime` for date-only fakes, and pair `vi.useRealTimers()` in `beforeEach` when other suites in the file used fake timers.
- Co-locate tests under `__tests__/` next to the module they cover.

## Things not to "fix"

- The `package-lock.json` was generated in error by previous agents; this repo is bun-only. If you see one in a diff, delete it.
- Never suppress lint/clippy warnings or skip git hooks (`--no-verify`). Fix the root cause.
