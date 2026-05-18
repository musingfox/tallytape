import { expect } from "@playwright/test";
import { DataTable } from "playwright-bdd";
import { Given, Then, When } from "./fixtures";

// ---------------------------------------------------------------------------
// Given steps
// ---------------------------------------------------------------------------

Given("a fresh tallytape data directory", async ({ fakeDb }) => {
  // The fakeDb fixture already provisioned an empty migrations-applied DB +
  // installed the Tauri shim. Nothing to do here — the step exists so the
  // Gherkin Background reads naturally.
  void fakeDb;
});

Given(
  "my last few days of work produced these sessions:",
  async ({ fakeDb }, table: DataTable) => {
    for (const row of table.hashes()) {
      fakeDb.insertReceipt(row.cwd, row.date);
    }
  },
);

Given(
  /^the dashboard is showing my (\d+) most recent receipts$/,
  async ({ fakeDb, page }, n: number) => {
    for (let i = 0; i < n; i++) {
      fakeDb.insertReceipt(
        `/Users/dev/project-${i}`,
        `2026-05-${String(10 + i).padStart(2, "0")}`,
      );
    }
    await page.goto("/");
    await expect(page.locator("#receipt-drawer-panel tbody tr")).toHaveCount(n);
  },
);

// ---------------------------------------------------------------------------
// When steps
// ---------------------------------------------------------------------------

When("I open the dashboard", async ({ page }) => {
  await page.goto("/");
  // The drawer always renders its <section>; wait for the load to settle so
  // subsequent assertions don't race the initial list_receipts response.
  await expect(page.locator("section")).toBeVisible();
});

When("a new Claude Code session finishes", async ({ fakeDb, page, arrival }) => {
  const cwd = "/Users/dev/just-arrived";
  const date = "2026-05-20";
  const id = fakeDb.insertReceipt(cwd, date);
  arrival.cwd = cwd;
  const payload = fakeDb.readReceipt(id);
  await page.evaluate(
    ([event, p]) => {
      const w = window as unknown as {
        __fireEvent: (event: string, payload: unknown) => void;
      };
      w.__fireEvent(event as string, p);
    },
    ["receipt-added", payload] as const,
  );
});

// ---------------------------------------------------------------------------
// Then steps
// ---------------------------------------------------------------------------

Then(/^I see (\d+) receipts$/, async ({ page }, n: number) => {
  await expect(page.locator("#receipt-drawer-panel tbody tr")).toHaveCount(n);
});

Then(/^I now see (\d+) receipts in total$/, async ({ page }, n: number) => {
  await expect(page.locator("#receipt-drawer-panel tbody tr")).toHaveCount(n);
});

Then("the most recent receipt appears first", async ({ page }) => {
  const dates = await page
    .locator("#receipt-drawer-panel tbody tr td:first-child")
    .allTextContents();
  expect(dates.length).toBeGreaterThanOrEqual(2);
  const sorted = [...dates].sort().reverse();
  expect(dates).toEqual(sorted);
});

Then(
  "the new receipt appears highlighted as new",
  async ({ page, arrival }) => {
    const cwd = arrival.cwd;
    expect(cwd, "no arrival recorded").not.toBeNull();
    // The pending row carries data-pending="true"; locate the row whose CWD
    // cell matches the arrival.
    const row = page.locator(
      `#receipt-drawer-panel tbody tr[data-pending="true"]`,
      { hasText: cwd! },
    );
    await expect(row).toHaveCount(1);
  },
);
