---
status: accepted
date: 2026-05-18
decision-makers: ""
---

# p6 Polish Phase Architecture: Notifications, Background Delivery, Tray, Hook Install

## Context and Problem Statement

Phase 6 ("polish") of tallytape introduces four feature areas that share architectural surface area but were filed as independent issues (#22–#28):

- **p6-4** Integrate `tauri-plugin-notification` — where does the `notify` helper live, and where is the "permission asked?" flag stored?
- **p6-5** Deliver a system notification when a Claude Code session finishes while the UI is closed — should the writer emit it, should a long-lived watcher emit it, or should an OS scheduler emit it?
- **p6-6** Tray icon — does closing the window quit the process or hide it to tray? What is the cross-platform scope?
- **p6-7** Promote `install-hook` from print-only stub to an idempotent patcher of `~/.claude/settings.json` — what dedup key identifies "our" entry, what is the backup strategy, and do we accept JSONC?

Without a single ADR, each issue would re-litigate the same questions and risk diverging answers (for example, two different "is the app running?" detection paths). We need one document that pins the cross-issue architectural contracts before any of these tickets ships.

## Decision Drivers

- All four features must keep the writer ↔ SQLite ↔ Tauri boundary from [ADR-0001](0001-foundational-architecture.md) intact — capture must keep working when the UI is closed
- Notifications cross the OS permission layer and must request consent exactly once
- `~/.claude/settings.json` is the user's file; any in-place modification needs a backup and a deterministic dedup key so reinstalls / multi-version installs do not stack hook entries
- macOS is the v1 platform; Windows and Linux are best-effort (CLAUDE.md does not commit to a cross-platform matrix yet)
- Settings that need to survive UI rebuilds (e.g., "notification permission already requested") live in SQLite, not in `localStorage` — the WebView's storage is rebuilt on every dev rebuild and is not addressable from the writer

## Considered Options

Discussed inline under each decision below.

## Decision Outcome

### D1 — p6-4 helper location + permission storage

- The Rust-side helper is `src-tauri/src/notifications.rs`, exposing a `notify(app: &AppHandle, title: &str, body: &str) -> Result<()>` function. It is callable from any `#[tauri::command]` handler and from the file-watcher emitter added in p6-5.
- A thin TypeScript wrapper lives at `src/ipc/notifications.ts` and is the only frontend entry point — never call `@tauri-apps/api` plugin imports from React components (same boundary rule that ESLint already enforces for `invoke`).
- Permission is requested at most once. The "asked?" flag is stored in a new `app_settings` row (`key TEXT PRIMARY KEY, value TEXT NOT NULL`) added via a new migration under `crates/tallytape-core/migrations/`. We do **not** use `localStorage` — dev rebuilds wipe it, and the writer cannot read it. We do **not** use the OS keychain — this is not a secret.
- The first-run permission prompt fires from `App.tsx` on mount, gated by a `get_app_setting('notification_permission_asked')` IPC. After a successful prompt (regardless of allow/deny), set the flag.

**Why not `tauri-plugin-store`?** It duplicates a persistence layer we already own (SQLite). Single source of truth, one migration history.

### D2 — p6-5 delivery path: keep Tauri app alive in tray, not writer-driven

Three options were considered:

- **A. Writer process emits the notification directly** (via `notify-rust` or an OS-specific API). Survives even when the Tauri app was never launched today, but: (1) the writer would need its own OS permission grant separate from the Tauri app's — confusing UX; (2) Tauri's capability system wraps the notification permission on macOS, and bypassing it puts us in a state where the Info.plist of `tallytape-writer` would need its own usage description string; (3) duplicates the helper from D1 in a separate crate.

- **B. Tauri app stays running in tray (close hides window); a `notify` crate watcher inside the Tauri process listens for SQLite row inserts and emits notifications via the D1 helper.** This is what we already build for live updates (the `notify` watcher exists — `src-tauri/src/lib.rs` emits `receipt-added` / `receipt-updated` events). Adding "if no window is focused, also call `notify(...)`" is a single conditional. The close-to-tray semantics required for this path are decided in D3 below — the two decisions are architecturally coupled.

- **C. macOS LaunchAgent that wraps a notifier daemon**, separate from both writer and UI. Avoids the "must be running" constraint but introduces a third binary to ship, a launchd plist to install, and a Linux/Windows equivalent we have not designed.

**Chosen: B.** It reuses the existing watcher, keeps the permission grant tied to the user-facing Tauri app, and avoids a third binary. The accepted cost is "background delivery only works if the user has launched the app at least once this session and has not quit via tray." This matches the user expectation set by every other macOS menubar app (Slack, 1Password, Raycast).

**2-second SLA from #26's AC**: the existing `notify` crate watcher already runs with 100ms debounce; the bottleneck is `tauri-plugin-notification`'s OS round-trip, which is < 500ms in practice. We do not need additional instrumentation to meet 2s — but we add a single `tracing` span around the helper call so a regression is grep-able from the log.

**"Click reopens app"**: handled by `tauri-plugin-notification`'s built-in action callback — the callback brings the main window forward via `WebviewWindow::set_focus()`. This is **the same code path** as tray "Open" (D3), so the implementation is one shared function.

**Catch-up on reopen (the "user fully quit" case)**: D2 option B has an explicit gap — receipts ingested by the writer while the Tauri app was fully quit produce no notification AND no UX signal on the next open. The catch-up mechanism closes both halves of that gap: replay missed receipts as pending on boot, and send one summary system notification.

**Two persistent cursors** in the `app_settings` table (from D1):
- `last_seen_receipt_id` — the highest `receipts.id` already presented to the user. Drives the "Added since" replay.
- `last_seen_max_updated_at` — the highest `receipts.updated_at` already presented. Drives the "Updated since" replay (catches the writer reingest path, which is rare but real — see `tallytape-writer/tests/`).

Cursors update on three triggers, always via `MAX(current_value_in_db, new_value)` so the watcher thread and the focus thread cannot roll either cursor backwards if they race:

1. After each watcher batch emits — write the batch's max `id` and max `updated_at`.
2. On window focus (`tauri::WindowEvent::Focused`) — write the current top-of-table values, so an OS-level kill (Activity Monitor, power loss) mid-session loses at most receipts the user has not yet looked at.
3. On graceful exit (`RunEvent::ExitRequested`) — final write on the only true-exit path defined in D3.

**Boot replay query**, executed immediately after `AppBackend::open` and the in-memory snapshot refresh, in a single read transaction:

```sql
SELECT * FROM receipts
WHERE id > :last_seen_id
   OR (id <= :last_seen_id AND updated_at > :last_seen_updated_at)
ORDER BY id DESC
LIMIT :cap + 1
```

- Rows with `id > :last_seen_id` are surfaced as pending Added.
- Rows matching only the `updated_at` clause are surfaced as pending Updated.
- The `LIMIT :cap + 1` lets us detect overflow without a separate `COUNT(*)`.

**Overflow handling** (cap = 50):
- If the result is ≤ cap rows, all are marked pending; animation plays per row; both cursors advance to the dispatched max.
- If the result is `cap + 1` rows, only the top `cap` (newest by id) are marked pending. The remainder are still loaded into the store via `listReceipts(null)` (data is never hidden), but without `data-pending="true"`. The drawer renders a single overflow row at the bottom of the pending section: "+M more arrived while you were away (open the receipts list to view all)". Cursors advance past **all** loaded rows so overflow never replays on the next boot.
- The cap exists because framer-motion `layout` animation cost grows non-linearly above ~50 simultaneous rows on the system WebView.

**Summary system notification**: if the catch-up batch is non-empty AND notification permission has been granted (D1), emit exactly one summary via the D1 helper:
- Title: "tallytape"
- Body: pluralised — "+1 receipt while you were away" / "+N receipts while you were away". `N` is the true batch size, not the overflow-capped count.
- Action callback: shared with the live-notification callback — `WebviewWindow::set_focus()` plus a window-event that scrolls the drawer to the top.
- Fires once per boot, regardless of receipt count.

**First-ever launch** (no rows in `app_settings` for either cursor): seed both to current `MAX(receipts.id)` and `MAX(receipts.updated_at)` (or 0 if the receipts table is empty). The user has just opened tallytape today; historical receipts ingested by an earlier writer install do not count as "missed". Distinguish "row absent" (uninitialised) from "row present with value 0" (table was empty at seed time, but baseline is still valid).

**Two snapshots, two purposes**: the existing in-memory `AppBackend.receipt_snapshot` (process-lifetime, `HashMap<i64, i64>` keyed by id → updated_at) is the watcher's diff baseline for emitting `Added` / `Updated` events. The new persistent cursors are the user-presentation baseline. They have different lifetimes (process vs SQLite) and different consumers (watcher vs UI), and must not be merged. Suggested naming to keep them apart in code: `last_emitted_snapshot` (in-memory) vs `last_seen_*` (persisted).

We use the integer PK as the primary cursor because (1) `receipts.id` is monotonic and unique by construction, (2) timestamp comparison is fuzzy across hooks that fire within the same second, and (3) the catch-up SQL stays a trivial indexed range scan. The secondary `updated_at` cursor only needs to catch the much smaller Updated set, where the same-second concern does not apply (reingests are spaced minutes or hours apart in practice).

### D3 — p6-6 tray + close-to-tray semantics

- Use Tauri 2's built-in tray API (`TrayIconBuilder`). Do not add a plugin.
- **Tray menu** (v1): `Open` (show + focus main window), `Quit tallytape` (truly exit). No other items.
- **Quit is reachable only from the tray menu.** All other "close-like" gestures hide the window without ending the process:
  - The window close button (red traffic light on macOS, `×` on Windows/Linux) hides the window.
  - `Cmd+Q` / `Alt+F4` is intercepted (Tauri `RunEvent::ExitRequested` → `prevent_exit()`) and hides the window instead of quitting.
  - The macOS application menu's "Quit" item is removed (or rebound to the same hide behaviour); the only labelled exit lives in the tray.
- The "Quit tallytape" tray item is labelled explicitly (not just "Quit") so it does not read as "minimise". It is the only path that truly terminates the process — which makes "is the app running?" a deterministic question (yes, unless the user clicked tray → Quit tallytape).
- This is the behaviour required by D2 — without close-to-tray-everywhere, p6-5 background delivery has too many silent exit paths to be reliable.
- **Cross-platform scope for v1: macOS only.** Tauri's tray API works on Windows and Linux, but we don't test there and we don't ship there yet. Document this in the issue acceptance criteria when picking up #27; do not block the ticket on Linux compatibility.
- The tray icon asset is a 22×22 template-style PNG under `src-tauri/icons/tray.png`. Until a designer produces one, ship a placeholder (literal text "T" rendered to PNG) and file a follow-up issue.

### D4 — p6-7 install-hook: dedup key, backup, JSON dialect

- `~/.claude/settings.json` is **strict JSON** (Claude Code's own parser does not accept comments). Use `serde_json`, not `json5` / a JSONC tolerator. If the user has manually added comments (we have seen this in shared dotfiles), parsing fails — write a backup, refuse to modify, print a clear "your settings.json has comments which Claude Code does not support; please remove them and re-run" message, exit non-zero.
- **Dedup key**: a hook entry in `hooks.SessionEnd[*].hooks[]` is "ours" if its `command` field, after expanding `~` and canonicalising via `std::fs::canonicalize`, points at a binary whose file name is `tallytape-writer` (or `tallytape-writer.exe` on Windows). We match on the **canonicalised file name**, not the absolute path — the install location can change across Homebrew vs cargo-install vs a downloaded release, and we do not want to stack entries on every upgrade.
- **Backup**: before any write, copy `settings.json` to `settings.json.bak.YYYYMMDDHHMMSS`. Keep the most recent 3 backups; older ones are deleted by the installer. If the user wants a manual backup, that is on them.
- **Idempotency**: install scans, removes all matching entries, then inserts the canonical one. Running install twice is identical to running it once.
- **Uninstall**: same dedup rule; remove only matching entries. If the `hooks[]` array becomes empty, drop the matcher object. If the `SessionEnd[]` array becomes empty, drop the `SessionEnd` key. If `hooks` becomes empty, drop the key. Never touch unrelated keys.
- The current `install-hook` (p2-6) is a print-only stub that emits a JSON snippet to stdout. Keep that stub reachable as `install-hook --print` for users who want to merge the entry into a config-management tool themselves. The new default (no flag) does the patching.

### Consequences

- Good, because the permission flag and notification helper have a single home each, removing the "which layer owns this?" question
- Good, because the dedup key is a property of the binary, not its install path — works across Homebrew, cargo-install, and downloaded releases without manual cleanup
- Good, because keeping background delivery inside the running Tauri app reuses the existing watcher and avoids a second OS permission grant
- Neutral, because "close hides, doesn't quit" is a behaviour change users may not expect on first launch — mitigated by tray menu being visible
- Neutral, because tray icon asset is a placeholder until design lands; documented as follow-up
- Good, because the two-cursor catch-up means even a fully-quit period does not silently lose user-visible signal — Added and Updated receipts from the gap surface as pending on next open, plus one summary system notification
- Good, because the overflow cap keeps the boot-time animation cost bounded regardless of how long the user was away
- Bad, because background notification delivery does not work if the user has fully quit the app via tray menu "Quit tallytape". This is the explicit cost of D2 option B over option A. Mitigated by the catch-up mechanism above and documented in onboarding when notification setting UI lands.
- Bad, because the `app_settings` migration adds another table that needs care during schema migrations — but the table is the right home for any future preference (notification cadence, default date range, theme) and for the catch-up cursor

### Confirmation

This ADR is confirmed when:

- A migration under `crates/tallytape-core/migrations/` creates the `app_settings` table and an `AppSettingsRepository` exposes `get` / `set` to the Tauri app
- `src-tauri/src/notifications.rs` exists and is the only call site of `tauri-plugin-notification`'s emit API
- `src/ipc/notifications.ts` wraps the helper for the frontend; ESLint `no-restricted-imports` (or a follow-up rule) blocks direct plugin imports outside that file
- The Tauri app stays running after the window is closed, after `Cmd+Q`, and after the macOS app-menu Quit; tray menu "Open" restores the window; tray menu "Quit tallytape" is the **only** path that ends the process
- After a tray "Quit tallytape" → relaunch cycle, every receipt the writer ingested while the app was down (up to the overflow cap) appears in the drawer with `data-pending="true"` and the fly-in animation; batches above the cap render an overflow row; both cursors advance past all loaded rows; a single summary system notification is emitted if permission is granted
- The Updated-cursor branch is exercised by a test that simulates writer reingest while the app is down and verifies the affected receipt appears pending Updated on next boot
- `tallytape-writer install-hook` modifies `~/.claude/settings.json` in place, creates a `.bak.<timestamp>` first, and a second run is a no-op (`shasum` of the file is identical on the third run as the second)
- `tallytape-writer uninstall-hook` exists and leaves no tallytape entries in the file
- A new integration test in `crates/tallytape-writer/tests/` covers: install on missing file, install on existing file with our entry already present (idempotent), install on existing file with **someone else's** entries (untouched), uninstall removes only ours

## Pros and Cons of the Options

### D2 alternative A — writer emits notifications directly

- Good, because works even if the Tauri app was never launched today
- Bad, because writer needs its own notification permission grant separate from the Tauri app
- Bad, because requires bundling `notify-rust` (or equivalent) into the writer, growing its binary size and platform-specific code
- Bad, because duplicates the helper that the Tauri app already needs for foreground notifications

### D2 alternative C — separate notifier daemon via LaunchAgent

- Good, because completely decouples notification delivery from any other process lifecycle
- Bad, because adds a third binary to build, ship, and version
- Bad, because LaunchAgent plist install/uninstall is its own subsystem (similar surface area to p6-7's `install-hook`)
- Bad, because no obvious Linux/Windows equivalent — would split the architecture by platform

### D4 alternative — store dedup key as absolute path

- Good, because trivial to implement
- Bad, because stacks entries on every install-path change (Homebrew → cargo-install upgrade leaves both pointing at different paths)
- Bad, because absolute path comparison is sensitive to symlinks (`/usr/local/bin` vs `/opt/homebrew/bin`)

### D4 alternative — JSONC tolerant parser

- Good, because users with commented settings would get a less disruptive experience
- Bad, because we would have to re-serialize back to strict JSON (Claude Code does not accept JSONC), silently dropping the user's comments — worse than refusing

## More Information

- [ADR-0001: Foundational Architecture](0001-foundational-architecture.md) — defines the writer ↔ SQLite ↔ Tauri boundary that this ADR builds on
- [tauri-plugin-notification docs](https://v2.tauri.app/plugin/notification/)
- [Tauri 2 tray icon docs](https://v2.tauri.app/learn/system-tray/)
- Claude Code SessionEnd hook schema lives in `~/.claude/settings.json`; the strict-JSON rule is verified by Claude Code's own parser
- Follow-up issues to file when this ADR is picked up: notification cadence settings UI, app icon / branding, p6-5 SLA measurement skeleton, tray icon design asset
