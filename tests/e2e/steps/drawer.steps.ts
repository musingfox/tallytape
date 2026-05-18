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

Given("I have not run any Claude Code sessions yet", async ({ fakeDb }) => {
  void fakeDb;
});

Given(
  /^the dashboard is showing (?:my )?(\d+)(?: most recent)? receipts$/,
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

Then("I see no receipts", async ({ page }) => {
  // The drawer renders an empty-state <p> instead of a <table> when the store
  // has zero receipts — both signals confirm the surface is empty.
  await expect(page.locator("#receipt-drawer-panel tbody tr")).toHaveCount(0);
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

// ---------------------------------------------------------------------------
// @ui-only steps — empty surface message
// ---------------------------------------------------------------------------

Then("an empty-state message invites me to start", async ({ page }) => {
  await expect(
    page.getByText("No receipts yet — start a session to print your first tape."),
  ).toBeVisible();
});

// ---------------------------------------------------------------------------
// @ui-only steps — pending marker fades after entry animation settles
// ---------------------------------------------------------------------------

When("I wait for the entry animation to settle", async ({ page }) => {
  // The framer-motion layout transition fires onAnimationComplete, which the
  // store consumes via clearPendingArrivals. We just need to wait until the
  // pending attribute has been turned off.
  await expect(
    page.locator(`#receipt-drawer-panel tbody tr[data-pending="true"]`),
  ).toHaveCount(0, { timeout: 5_000 });
});

Then("no receipt is highlighted as new", async ({ page }) => {
  await expect(
    page.locator(`#receipt-drawer-panel tbody tr[data-pending="true"]`),
  ).toHaveCount(0);
});

// ---------------------------------------------------------------------------
// @ui-only steps — drawer collapse / expand
// ---------------------------------------------------------------------------

When("I collapse the receipt drawer", async ({ page }) => {
  await page.getByRole("button", { name: "Recent Receipts" }).click();
});

When("I re-open the receipt drawer", async ({ page }) => {
  await page.getByRole("button", { name: "Recent Receipts" }).click();
});

Then("the receipt list is hidden", async ({ page }) => {
  await expect(
    page.getByRole("button", { name: "Recent Receipts" }),
  ).toHaveAttribute("aria-expanded", "false");
  await expect(page.locator("#receipt-drawer-panel table")).toHaveCount(0);
});

Then("the receipt list is visible again", async ({ page }) => {
  await expect(
    page.getByRole("button", { name: "Recent Receipts" }),
  ).toHaveAttribute("aria-expanded", "true");
  await expect(page.locator("#receipt-drawer-panel table")).toBeVisible();
});

Then(
  "the drawer header remains so I can re-open it",
  async ({ page }) => {
    await expect(
      page.getByRole("button", { name: "Recent Receipts" }),
    ).toBeVisible();
  },
);
