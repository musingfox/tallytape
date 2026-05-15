import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { createMemoryHistory, createRouter, RouterProvider } from "@tanstack/react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useReceiptStore } from "../../receipts/store";
import type { Receipt } from "../../receipts/types";
import { routeTree } from "../../routeTree.gen";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("../../receipts/useReceiptEvents", () => ({
  useReceiptEvents: vi.fn(),
}));

function receipt(overrides: Partial<Receipt> & Pick<Receipt, "id" | "cwd" | "date">): Receipt {
  return {
    sessionId: null,
    createdAt: 1,
    updatedAt: 2,
    ...overrides,
  };
}

function seedReceipts(receipts: Receipt[]) {
  useReceiptStore.setState({
    receipts: new Map(receipts.map((item) => [item.id, item])),
    pendingArrivals: [],
    selectedId: null,
  });
}

function renderIndex() {
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });

  render(<RouterProvider router={router} />);

  return router;
}

beforeEach(() => {
  window.scrollTo = vi.fn();
  seedReceipts([]);
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockResolvedValue([]);
});

describe("ReceiptTable", () => {
  it("renders a four-column table with one row per receipt", async () => {
    seedReceipts([
      receipt({ id: 1, cwd: "/a", date: "2026-05-15" }),
      receipt({ id: 2, cwd: "/b", date: "2026-05-14" }),
    ]);

    renderIndex();

    await waitFor(() => expect(screen.getAllByRole("row")).toHaveLength(3));
    expect(screen.getByText("Date")).toBeInTheDocument();
    expect(screen.getByText("CWD")).toBeInTheDocument();
    expect(screen.getByText("Total")).toBeInTheDocument();
    expect(screen.getByText("Items")).toBeInTheDocument();
    expect(screen.getByText("/a")).toBeInTheDocument();
    expect(screen.getByText("2026-05-15")).toBeInTheDocument();
  });

  it("renders an empty state and no table when there are no receipts", async () => {
    renderIndex();

    expect(await screen.findByText("No receipts captured yet.")).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
  });

  it("navigates to the receipt detail route when a receipt row is clicked", async () => {
    seedReceipts([receipt({ id: 7, cwd: "/nav-click", date: "2026-05-15" })]);
    const router = renderIndex();

    const cwd = await screen.findByText("/nav-click");
    fireEvent.click(cwd.closest("tr") ?? cwd);

    await waitFor(() => expect(router.state.location.pathname).toBe("/receipts/7"));
  });

  it("navigates to the receipt detail route when Enter is pressed on a focused row", async () => {
    seedReceipts([receipt({ id: 7, cwd: "/nav-enter", date: "2026-05-15" })]);
    const router = renderIndex();
    const row = (await screen.findByText("/nav-enter")).closest("tr");

    expect(row).not.toBeNull();
    row?.focus();
    fireEvent.keyDown(row as HTMLTableRowElement, { key: "Enter" });

    await waitFor(() => expect(router.state.location.pathname).toBe("/receipts/7"));
  });

  it("sorts rows by date descending and id descending for equal dates", async () => {
    seedReceipts([
      receipt({ id: 1, cwd: "id-1-cwd", date: "2026-05-10" }),
      receipt({ id: 2, cwd: "id-2-cwd", date: "2026-05-15" }),
      receipt({ id: 3, cwd: "id-3-cwd", date: "2026-05-15" }),
    ]);

    renderIndex();

    await waitFor(() => expect(screen.getAllByRole("row")).toHaveLength(4));
    const rows = screen.getAllByRole("row").slice(1);
    expect(rows[0].textContent).toContain("id-3-cwd");
    expect(rows[1].textContent).toContain("id-2-cwd");
    expect(rows[2].textContent).toContain("id-1-cwd");
  });

  it("fetches item summaries once per receipt and renders totals and item counts", async () => {
    seedReceipts([
      receipt({ id: 1, cwd: "summary-1", date: "2026-05-15" }),
      receipt({ id: 2, cwd: "summary-2", date: "2026-05-14" }),
      receipt({ id: 3, cwd: "summary-3", date: "2026-05-13" }),
    ]);
    vi.mocked(invoke).mockImplementation(async (cmd, args) => {
      if (cmd === "list_items_by_receipt") {
        const id = (args as { receiptId: number }).receiptId;
        if (id === 1) return [{ cost: 0.1 }, { cost: 0.25 }];
        if (id === 2) return [{ cost: 1 }];
        return [];
      }
      return [];
    });

    renderIndex();

    await waitFor(() => expect(screen.getByText("0.35")).toBeInTheDocument());
    expect(vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "list_items_by_receipt")).toHaveLength(3);

    const row1Cells = within(screen.getByText("summary-1").closest("tr") as HTMLTableRowElement).getAllByRole("cell");
    expect(row1Cells[2]).toHaveTextContent("0.35");
    expect(row1Cells[3]).toHaveTextContent("2");

    const row2Cells = within(screen.getByText("summary-2").closest("tr") as HTMLTableRowElement).getAllByRole("cell");
    expect(row2Cells[2]).toHaveTextContent("1");
    expect(row2Cells[3]).toHaveTextContent("1");

    const row3Cells = within(screen.getByText("summary-3").closest("tr") as HTMLTableRowElement).getAllByRole("cell");
    expect(row3Cells[2]).toHaveTextContent("0");
    expect(row3Cells[3]).toHaveTextContent("0");
  });

  it("keeps a rejected receipt summary as dashes while other rows resolve", async () => {
    seedReceipts([
      receipt({ id: 1, cwd: "summary-ok", date: "2026-05-15" }),
      receipt({ id: 2, cwd: "summary-fail", date: "2026-05-14" }),
    ]);
    vi.mocked(invoke).mockImplementation(async (cmd, args) => {
      if (cmd === "list_items_by_receipt") {
        const id = (args as { receiptId: number }).receiptId;
        if (id === 2) throw new Error("failed");
        return [{ cost: 4 }];
      }
      return [];
    });

    renderIndex();

    await waitFor(() => expect(screen.getByText("4")).toBeInTheDocument());

    const okCells = within(screen.getByText("summary-ok").closest("tr") as HTMLTableRowElement).getAllByRole("cell");
    expect(okCells[2]).toHaveTextContent("4");
    expect(okCells[3]).toHaveTextContent("1");

    const failCells = within(screen.getByText("summary-fail").closest("tr") as HTMLTableRowElement).getAllByRole("cell");
    expect(failCells[2]).toHaveTextContent("—");
    expect(failCells[3]).toHaveTextContent("—");
  });

  it("renders 100 receipt rows without throwing", async () => {
    seedReceipts(
      Array.from({ length: 100 }, (_, index) =>
        receipt({ id: index + 1, cwd: `cwd-${index + 1}`, date: "2026-05-15" }),
      ),
    );

    renderIndex();

    await waitFor(() => expect(screen.getAllByRole("row")).toHaveLength(101));
  });
});
