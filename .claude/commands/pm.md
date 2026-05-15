---
description: "Tallytape PM — list / create / update tasks on GH Project v2 (musingfox/projects/2)"
argument-hint: "[natural language request]"
allowed-tools: ["Agent"]
---

# /pm — Tallytape GitHub Project Management

Delegate to the `gh-pm` agent so `gh` CLI output, JSON dumps, and full issue listings stay out of the main context.

Invoke `Agent` with:
- `subagent_type`: `gh-pm`
- `description`: `PM operation on tallytape GH Project`
- `prompt`: `args=$ARGUMENTS`
- `model`: omit (Haiku default). For multi-step task planning or anything requiring the user-asked higher quality, override with `sonnet`.

Relay the agent's summary verbatim. Do not embellish.

## Usage

```
/pm                                  # no args → status summary
/pm show me p4-5
/pm list todo tasks
/pm high priority open items
/pm add a task fix-list-scroll, frontend, high priority
/pm p4-4a is in progress
/pm bump #15 priority to high
/pm add label perf to p4-5
/pm p4-4b is done, archive it
/pm find tasks tagged ipc
```

The agent classifies the intent (list / get / create / status / priority / label / archive / search / summary) and confirms destructive actions before executing.

## Boundaries

- **In scope**: tasks on https://github.com/users/musingfox/projects/2 + issues at `musingfox/tallytape`.
- **Out of scope**: ADRs and dashboards still live in Obsidian — use `/obw:pm` for those. Git/PR operations are out of scope for this agent.
