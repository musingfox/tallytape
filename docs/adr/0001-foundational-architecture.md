---
status: accepted
date: 2026-05-04
decision-makers: ""
---

# Foundational Architecture: Detached Writer + SQLite + Tauri + Claude-Code-Session Merge Rule

## Context and Problem Statement

TallyTape needs to capture and track activity (such as Claude Code sessions) into a queryable local store, while also providing a desktop UI for browsing and analyzing that data. The capture path may run from multiple entry points (UI, CLI invocations, automation hooks) and must remain reliable even if the UI process crashes. The data model needs to merge raw entries into meaningful units of work without forcing the user to manually delimit "sessions."

What architecture should TallyTape adopt for the data write path, the storage engine, the UI shell, and the entry-merging logic?

## Decision Drivers

* Capture must continue working when the UI is not running or crashes
* The same store must be writable from a CLI binary as well as from the UI
* Multiple processes must be able to write concurrently without corrupting data
* Storage should be local-first with no required network or server
* The store needs richer query capability than flat files for time-series and aggregation
* The merge rule for grouping raw entries into "units" should reflect a natural boundary the user already understands, not an arbitrary timer
* The Rust ecosystem is preferred (existing `tallytape-core` and `tallytape-writer` crates already established the workspace)

## Considered Options

* **Write path**: detached writer binary (separate process) vs in-process writer inside the UI
* **Storage**: SQLite (embedded, ACID, WAL) vs JSONL flat files vs PostgreSQL/server DB
* **UI shell**: Tauri 2.x (Rust + WebView) vs Electron (JS-only backend)
* **Merge rule**: align with Claude Code's own session boundary vs custom time-window heuristic vs manual user-defined sessions

## Decision Outcome

Chosen architecture:

1. **Detached writer binary** (`tallytape-writer`) — a standalone process invoked by the UI, by CLI, and by automation hooks; not embedded in the Tauri app.
2. **SQLite** (WAL mode) as the single source of truth for all captured data.
3. **Tauri 2.x** (`tallytape-app`) as the desktop UI shell, communicating with the writer indirectly via SQLite (and directly only for live state the writer doesn't own).
4. **Merge rule**: entries belonging to the same Claude Code session are merged into one logical record. The Claude Code session ID (provided by Claude Code itself) is the merge key — TallyTape does not invent its own session boundary. The "same-cwd" framing in earlier planning notes is a consequence of how Claude Code scopes its sessions, not a separate rule.

Rationale: this combination keeps capture reliable under UI crashes, allows headless / CLI / scripted writes, gives every client (UI, CLI, future automations) the same query surface, and avoids designing a session-boundary heuristic when an authoritative one (Claude Code's own session) already exists.

### Consequences

* Good, because the writer keeps recording even when the UI is closed or crashes
* Good, because the writer is reachable from CLI, shell hooks, schedulers, and the UI uniformly
* Good, because SQLite WAL mode permits concurrent writers without file-lock contention
* Good, because SQL gives time-series and aggregation queries that JSONL cannot serve directly
* Good, because aligning the merge boundary with Claude Code's session ID removes a category of edge cases (idle gaps, parallel terminals, resumed sessions) that a TallyTape-defined timer would handle inconsistently
* Good, because the local-first SQLite file is trivially backed up, synced, or migrated later
* Neutral, because two Rust binaries (UI + writer) must be built and distributed together
* Neutral, because the schema is portable to PostgreSQL later if multi-device server-side aggregation becomes a goal
* Bad, because the writer ↔ UI boundary requires defining an IPC or shared-DB contract, which is more design surface than an in-process writer
* Bad, because tying the merge rule to Claude Code couples TallyTape to Claude Code's session-ID semantics — if Claude Code changes how it scopes sessions, the merge behavior shifts
* Bad, because supporting non-Claude-Code data sources later will require a separate merge rule per source

### Confirmation

This decision is confirmed when:

* `tallytape-writer` runs as a standalone binary that can be invoked without the UI present
* `tallytape-app` (Tauri) reads from SQLite and does not host the write path in-process
* The SQLite file is opened in WAL mode by both writer and reader
* The schema includes a `claude_session_id` (or equivalent) column, and merging logic groups rows by that column rather than by a time window or cwd heuristic alone
* No code path in the UI writes user-captured data to storage directly — all writes go through the writer binary

## Pros and Cons of the Options

### Detached writer binary (chosen)

A standalone executable invoked per capture event; communicates with SQLite directly.

* Good, because UI crashes do not block capture
* Good, because the same binary is callable from CLI, hooks, and the UI
* Good, because process isolation contains writer bugs
* Bad, because requires bundling and updating two binaries
* Bad, because writer ↔ UI contract must be designed (IPC, file watching, or DB polling)

### In-process writer inside UI

Writer logic compiled into the Tauri app; no separate binary.

* Good, because one binary to ship and update
* Good, because no IPC contract to design
* Bad, because UI crashes lose pending writes
* Bad, because no headless / CLI / hook capture path
* Bad, because heavy IO can block or jitter the UI process

### SQLite (chosen)

Embedded, single-file, ACID, WAL mode.

* Good, because zero-ops local-first storage
* Good, because WAL supports multi-writer concurrency
* Good, because SQL handles time-series and aggregation queries
* Good, because portable to PostgreSQL later via schema mapping
* Bad, because not designed for cross-machine multi-writer scenarios (would need sync layer)

### JSONL / flat files

Append-only newline-delimited JSON per record.

* Good, because trivially append-safe and human-readable
* Good, because zero schema-migration burden up-front
* Bad, because no query engine — every read is a full scan
* Bad, because aggregation requires loading everything into memory or a separate index
* Bad, because concurrent appends from multiple processes need OS-level locking discipline

### PostgreSQL / other server DB

Client-server relational database.

* Good, because mature, scales horizontally, supports multi-device aggregation
* Bad, because requires a server process — breaks local-first / zero-ops goal
* Bad, because installation friction for end users
* Bad, because overkill for single-user single-machine usage

### Tauri 2.x (chosen)

Rust backend + system WebView frontend.

* Good, because shares the Rust workspace with `tallytape-core` / `tallytape-writer`
* Good, because produces small binaries (no bundled Chromium)
* Good, because uses system WebKit on macOS — no extra runtime install
* Bad, because ecosystem younger than Electron; some plugins less mature

### Electron

Chromium + Node.js backend.

* Good, because most mature desktop-web ecosystem
* Good, because cross-platform behavior most uniform
* Bad, because JS-only backend disconnects from the existing Rust workspace
* Bad, because large binary size (bundled Chromium)
* Bad, because would require a separate Rust ↔ JS bridge to reach the writer

### Merge rule: Claude Code session boundary (chosen)

Use Claude Code's own session ID as the merge key for grouping captured entries.

* Good, because the boundary is authoritative — no heuristic to tune
* Good, because handles idle gaps, resumed sessions, and parallel terminals correctly by construction
* Good, because the user already reasons in terms of Claude Code sessions
* Bad, because couples TallyTape to Claude Code's session semantics
* Bad, because non-Claude-Code data sources need a different rule

### Merge rule: time-window heuristic

Group entries with the same cwd if they fall within N minutes of each other.

* Good, because source-agnostic — works for any timestamped event stream
* Bad, because N is arbitrary and user-tunable, generating support questions
* Bad, because misclassifies long thinking pauses as session breaks
* Bad, because parallel terminals in the same cwd merge incorrectly

### Merge rule: manual user-defined sessions

User explicitly starts and stops sessions via UI/CLI.

* Good, because boundaries are exact
* Bad, because user must remember to delimit — the highest-friction option
* Bad, because automation-driven captures (hooks) have no natural place to call start/stop

## More Information

* [ADR-0000: Use MADR 4.0 for ADRs](0000-use-madr.md)
* [Tauri 2.0 docs](https://v2.tauri.app/)
* [SQLite WAL mode](https://www.sqlite.org/wal.html)
* Future ADRs to write: SQLite schema design (P1), writer ↔ UI IPC contract (P1), non-Claude-Code data source merge rules (P2+)
