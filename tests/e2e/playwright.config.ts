import { defineConfig, devices } from "@playwright/test";
import { defineBddConfig } from "playwright-bdd";

const testDir = defineBddConfig({
  features: "../features/*.feature",
  steps: "steps/*.ts",
  featuresRoot: "..",
  // The UI side binds both @dual (shared with the Rust API runner) and
  // @ui-only (presentation/animation flows that only make sense in the DOM).
  tags: "@dual or @ui-only",
});

export default defineConfig({
  testDir,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: 1,
  reporter: process.env.CI ? "list" : "list",
  use: {
    baseURL: "http://localhost:1420",
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: "bun run dev",
    port: 1420,
    reuseExistingServer: !process.env.CI,
    stdout: "pipe",
    stderr: "pipe",
  },
});
