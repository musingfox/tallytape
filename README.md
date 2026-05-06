# tallytape

Records Claude Code session costs locally by consuming `SessionEnd` hook payloads.

## Build

```sh
cargo build --release -p tallytape-writer
```

## Hook Installation

Run the following command after building:

```sh
tallytape-writer install-hook
```

Copy the JSON output and paste it into `~/.claude/settings.json`.

## Merge Guidance

If `~/.claude/settings.json` already contains a `"hooks"` key or existing `SessionEnd` entries, you must merge the printed snippet into the existing structure rather than overwrite the file. Overwriting will silently drop any hooks you have already configured.

## Planned: Automated Installation (Phase 6)

Automated installation via `tallytape-writer install-hook --apply` is planned for Phase 6 and will handle the merge automatically.
