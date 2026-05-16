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
    loadStatus: 'ready',
    loadError: null,
    errors: [],
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

    expect(await screen.findByText("No receipts yet — start a session to print your first tape.")).toBeInTheDocument();
    expect(screen.queryByRole("table")).toBeNull();
  });

  it("renders five skeleton rows while receipts are loading", async () => {
    useReceiptStore.setState({ loadStatus: "loading", receipts: new Map() });

    renderIndex();

    await waitFor(() => expect(screen.getAllByTestId("skeleton-row")).toHaveLength(5));
    expect(screen.queryByText(/No receipts yet/)).toBeNull();
    expect(screen.getByRole("table").querySelector("thead")).not.toBeNull();
  });

  it("renders five skeleton rows while receipts are idle", async () => {
    useReceiptStore.setState({ loadStatus: "idle" });

    renderIndex();

    await waitFor(() => expect(screen.getAllByTestId("skeleton-row")).toHaveLength(5));
    expect(screen.queryByText(/No receipts yet/)).toBeNull();
  });

  it("renders ready empty copy without skeleton rows", async () => {
    useReceiptStore.setState({ loadStatus: "ready", receipts: new Map() });

    renderIndex();

    expect(await screen.findByText("No receipts yet — start a session to print your first tape.")).toBeInTheDocument();
    expect(screen.queryAllByTestId("skeleton-row")).toHaveLength(0);
  });

  it("renders load error banner and toast", async () => {
    useReceiptStore.setState({
      loadStatus: "error",
      loadError: "db locked",
      errors: [{ id: "t1", message: "db locked", createdAt: 1 }],
    });

    renderIndex();

    expect(await screen.findByText("Couldn't load receipts.")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("db locked");
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

  it("fetches summaries via a single aggregated IPC and renders totals and item counts", async () => {
    seedReceipts([
      receipt({ id: 1, cwd: "summary-1", date: "2026-05-15" }),
      receipt({ id: 2, cwd: "summary-2", date: "2026-05-14" }),
      receipt({ id: 3, cwd: "summary-3", date: "2026-05-13" }),
    ]);
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === "list_receipt_summaries") {
        return [
          { receiptId: 1, totalCost: 0.35, itemCount: 2 },
          { receiptId: 2, totalCost: 1, itemCount: 1 },
          { receiptId: 3, totalCost: 0, itemCount: 0 },
        ];
      }
      return [];
    });

    renderIndex();

    await waitFor(() => expect(screen.getByText("0.35")).toBeInTheDocument());
    expect(vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "list_receipt_summaries")).toHaveLength(1);
    expect(vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "list_items_by_receipt")).toHaveLength(0);

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

  it("drops stale summary resolutions when receipt dependencies change", async () => {
    seedReceipts([
      receipt({ id: 1, cwd: "stale-a", date: "2026-05-15" }),
      receipt({ id: 2, cwd: "stale-b", date: "2026-05-14" }),
    ]);
    let resolveFirst: (value: unknown) => void = () => {};
    let resolveSecond: (value: unknown) => void = () => {};
    vi.mocked(invoke)
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveFirst = resolve;
          }),
      )
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveSecond = resolve;
          }),
      );

    renderIndex();
    await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledTimes(1));

    seedReceipts([
      receipt({ id: 1, cwd: "stale-a", date: "2026-05-15" }),
      receipt({ id: 2, cwd: "stale-b", date: "2026-05-14" }),
      receipt({ id: 3, cwd: "stale-c", date: "2026-05-13" }),
    ]);
    await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledTimes(2));

    resolveSecond([{ receiptId: 3, totalCost: 9, itemCount: 9 }]);
    await waitFor(() => expect(screen.getByLabelText("Total cost $9.00")).toBeInTheDocument());
    resolveFirst([
      { receiptId: 1, totalCost: 1, itemCount: 1 },
      { receiptId: 2, totalCost: 2, itemCount: 2 },
    ]);

    await waitFor(() => {
      const row = screen.getByText("stale-c").closest("tr") as HTMLTableRowElement;
      const cells = within(row).getAllByRole("cell");
      expect(cells[2]).toHaveTextContent("9");
      expect(cells[3]).toHaveTextContent("9");
    });
    expect(screen.queryByText("1")).toBeNull();
    expect(screen.queryByText("2")).toBeNull();
  });

  it("keeps a missing receipt summary as dashes while other rows resolve", async () => {
    seedReceipts([
      receipt({ id: 1, cwd: "summary-ok", date: "2026-05-15" }),
      receipt({ id: 2, cwd: "summary-fail", date: "2026-05-14" }),
    ]);
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === "list_receipt_summaries") {
        return [{ receiptId: 1, totalCost: 4, itemCount: 1 }];
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

  it("activates a row with Space and prevents default scroll", async () => {
    seedReceipts([receipt({ id: 9, cwd: "/space", date: "2026-05-15" })]);
    const router = renderIndex();
    const row = (await screen.findByText("/space")).closest("tr") as HTMLTableRowElement;
    row.focus();
    const event = new KeyboardEvent("keydown", { key: " ", bubbles: true, cancelable: true });
    row.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    await waitFor(() => expect(router.state.location.pathname).toBe("/receipts/9"));
  });

  it("does not navigate on unrelated keys", async () => {
    seedReceipts([receipt({ id: 9, cwd: "/x", date: "2026-05-15" })]);
    const router = renderIndex();
    const row = (await screen.findByText("/x")).closest("tr") as HTMLTableRowElement;
    fireEvent.keyDown(row, { key: "a" });
    expect(router.state.location.pathname).toBe("/");
  });

  it("exposes exactly one initial tab stop and rotates tabIndex on ArrowDown", async () => {
    seedReceipts([
      receipt({ id: 1, cwd: "r1", date: "2026-05-15" }),
      receipt({ id: 2, cwd: "r2", date: "2026-05-14" }),
      receipt({ id: 3, cwd: "r3", date: "2026-05-13" }),
    ]);
    renderIndex();
    await waitFor(() => expect(screen.getAllByRole("row")).toHaveLength(4));
    const bodyRows = screen.getAllByRole("row").slice(1) as HTMLTableRowElement[];
    expect(bodyRows.map((r) => r.tabIndex)).toEqual([0, -1, -1]);
    bodyRows[0].focus();
    fireEvent.keyDown(bodyRows[0], { key: "ArrowDown" });
    expect(document.activeElement).toBe(bodyRows[1]);
    expect(bodyRows.map((r) => r.tabIndex)).toEqual([-1, 0, -1]);
  });

  it("does not wrap at the boundaries and supports Home/End", async () => {
    seedReceipts([
      receipt({ id: 1, cwd: "r1", date: "2026-05-15" }),
      receipt({ id: 2, cwd: "r2", date: "2026-05-14" }),
      receipt({ id: 3, cwd: "r3", date: "2026-05-13" }),
    ]);
    renderIndex();
    const bodyRows = (await screen.findAllByRole("row")).slice(1) as HTMLTableRowElement[];
    bodyRows[0].focus();
    fireEvent.keyDown(bodyRows[0], { key: "ArrowUp" });
    expect(document.activeElement).toBe(bodyRows[0]);
    fireEvent.keyDown(bodyRows[0], { key: "End" });
    expect(document.activeElement).toBe(bodyRows[2]);
    fireEvent.keyDown(bodyRows[2], { key: "ArrowDown" });
    expect(document.activeElement).toBe(bodyRows[2]);
    fireEvent.keyDown(bodyRows[2], { key: "Home" });
    expect(document.activeElement).toBe(bodyRows[0]);
  });

  it("attaches the focus-visible ring utility classes to each interactive row", async () => {
    seedReceipts([receipt({ id: 1, cwd: "/fr", date: "2026-05-15" })]);
    renderIndex();
    const row = (await screen.findByText("/fr")).closest("tr") as HTMLTableRowElement;
    expect(row.className).toMatch(/focus-visible:ring-2/);
    expect(row.className).toMatch(/focus-visible:ring-inset/);
    expect(row.className).not.toMatch(/(^|\s)focus:ring/);
  });

  it("labels each interactive row with an SR-friendly action phrase", async () => {
    seedReceipts([receipt({ id: 1, cwd: "/foo", date: "2026-05-15" })]);
    renderIndex();
    const row = (await screen.findByText("/foo")).closest("tr") as HTMLTableRowElement;
    expect(row.getAttribute("aria-label")).toBe("View receipt for /foo on 2026-05-15");
  });

  it("does not attach interactive aria-label to skeleton rows", async () => {
    useReceiptStore.setState({ loadStatus: "loading", receipts: new Map() });
    renderIndex();
    const skeletons = await screen.findAllByTestId("skeleton-row");
    for (const skeleton of skeletons) {
      expect(skeleton.hasAttribute("aria-label")).toBe(false);
      expect(skeleton.getAttribute("aria-busy")).toBe("true");
    }
  });

  it("marks the Date header with aria-sort=descending and leaves others unset", async () => {
    seedReceipts([receipt({ id: 1, cwd: "/x", date: "2026-05-15" })]);
    renderIndex();
    const dateTh = (await screen.findByText("Date")).closest("th") as HTMLTableCellElement;
    expect(dateTh.getAttribute("aria-sort")).toBe("descending");
    expect(screen.getByText("Total").closest("th")?.hasAttribute("aria-sort")).toBe(false);
    expect(screen.getByText("CWD").closest("th")?.hasAttribute("aria-sort")).toBe(false);
    expect(screen.getByText("Items").closest("th")?.hasAttribute("aria-sort")).toBe(false);
  });

  it("labels Total and Items cells with units and currency", async () => {
    seedReceipts([
      receipt({ id: 1, cwd: "/two", date: "2026-05-15" }),
      receipt({ id: 2, cwd: "/one", date: "2026-05-14" }),
    ]);
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === "list_receipt_summaries") {
        return [
          { receiptId: 1, totalCost: 0.35, itemCount: 2 },
          { receiptId: 2, totalCost: 1, itemCount: 1 },
        ];
      }
      return [];
    });
    renderIndex();
    await waitFor(() => expect(screen.getByText("0.35")).toBeInTheDocument());
    const twoCells = within(screen.getByText("/two").closest("tr") as HTMLTableRowElement).getAllByRole("cell");
    expect(twoCells[2].getAttribute("aria-label")).toBe("Total cost $0.35");
    expect(twoCells[3].getAttribute("aria-label")).toBe("2 items");
    const oneCells = within(screen.getByText("/one").closest("tr") as HTMLTableRowElement).getAllByRole("cell");
    expect(oneCells[3].getAttribute("aria-label")).toBe("1 item");
  });

  it("labels pending Total/Items cells as Pending", async () => {
    seedReceipts([receipt({ id: 1, cwd: "/p", date: "2026-05-15" })]);
    vi.mocked(invoke).mockRejectedValue(new Error("nope"));
    renderIndex();
    const row = (await screen.findByText("/p")).closest("tr") as HTMLTableRowElement;
    await waitFor(() => {
      const cells = within(row).getAllByRole("cell");
      expect(cells[2].getAttribute("aria-label")).toBe("Pending");
      expect(cells[3].getAttribute("aria-label")).toBe("Pending");
    });
  });

  it("assigns aria-rowindex starting at 2 for body rows", async () => {
    seedReceipts([
      receipt({ id: 1, cwd: "r1", date: "2026-05-15" }),
      receipt({ id: 2, cwd: "r2", date: "2026-05-14" }),
      receipt({ id: 3, cwd: "r3", date: "2026-05-13" }),
    ]);
    renderIndex();
    const rows = await screen.findAllByRole("row");
    expect(rows[1].getAttribute("aria-rowindex")).toBe("2");
    expect(rows[2].getAttribute("aria-rowindex")).toBe("3");
    expect(rows[3].getAttribute("aria-rowindex")).toBe("4");
  });
});
