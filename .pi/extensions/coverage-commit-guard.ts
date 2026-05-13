import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { isToolCallEventType, type ExtensionAPI } from "@mariozechner/pi-coding-agent";

const COMMIT_COMMAND = /^\s*(git|jj)\s+commit\b/;

export default function (pi: ExtensionAPI) {
  pi.on("tool_call", async (event) => {
    if (!isToolCallEventType("bash", event)) return;
    if (!COMMIT_COMMAND.test(event.input.command ?? "")) return;

    const cwd = process.cwd();
    if (
      !existsSync(join(cwd, "package.json")) ||
      !existsSync(join(cwd, "scripts", "check-rust-branch-coverage.py"))
    ) {
      return;
    }

    try {
      execFileSync("bun", ["run", "coverage:rust:branch"], {
        cwd,
        stdio: "inherit",
        env: process.env,
      });
    } catch {
      return {
        block: true,
        reason:
          "Rust branch coverage gate failed. Run `bun run coverage:rust:branch` and fix coverage before committing.",
      };
    }
  });
}
