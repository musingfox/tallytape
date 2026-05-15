import { invoke } from "@tauri-apps/api/core";
import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ItemDto } from "../../ipc/types";
import { useReceiptStore } from "../../receipts/store";
import type { Receipt } from "../../receipts/types";
import { ReceiptDetail } from "../receipts/$id";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

function makeItem(overrides: Partial<ItemDto> = {}): ItemDto {
  return {
    id: 1,
    receiptId: 42,
    sessionId: 1,
    source: "claude",
    requestId: "request-1",
    messageId: null,
    parentUuid: null,
    isSidechain: false,
    occurredAt: 100,
    model: "claude-sonnet-4",
    serviceTier: null,
    inputTokens: 0,
    outputTokens: 0,
    cacheReadTokens: null,
    cacheCreationTokens: null,
    cost: 0,
    metadata: null,
    ...overrides,
  };
}

function seedReceipt(overrides: Partial<Receipt> = {}) {
  const id = overrides.id ?? 42;
  useReceiptStore.setState({
    receipts: new Map([
      [
        id,
        {
          id,
          sessionId: null,
          cwd: "/tmp/detail-found",
          date: "2026-05-15",
          createdAt: 1000,
          updatedAt: 2000,
          ...overrides,
        },
      ],
    ]),
  });
}

beforeEach(() => {
  useReceiptStore.setState({ receipts: new Map() });
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockResolvedValue([]);
});

describe("ReceiptDetail", () => {
  it("renders the matching receipt from the store", () => {
    useReceiptStore.setState({
      receipts: new Map([
        [
          42,
          {
            id: 42,
            sessionId: null,
            cwd: "/tmp/detail-found",
            date: "2026-05-15",
            createdAt: 1000,
            updatedAt: 2000,
          },
        ],
      ]),
    });

    render(<ReceiptDetail id={42} />);

    expect(screen.getByText("/tmp/detail-found")).toBeInTheDocument();
    expect(screen.getByText("2026-05-15")).toBeInTheDocument();
  });

  it("renders a not found fallback for a missing receipt", () => {
    render(<ReceiptDetail id={999} />);

    expect(screen.getByText(/Not Found/i)).toBeInTheDocument();
    expect(document.body.textContent).toContain("999");
  });

  describe("receipt header", () => {
    it("renders location, session slug, and date before items load", () => {
      vi.mocked(invoke).mockReturnValue(new Promise(() => {}));
      seedReceipt({ id: 42, cwd: "/tmp/detail-found", date: "2026-05-15", sessionId: null, createdAt: 1, updatedAt: 2 });

      render(<ReceiptDetail id={42} />);

      expect(screen.getByText("/tmp/detail-found")).toBeInTheDocument();
      expect(screen.getByText("2026-05-15")).toBeInTheDocument();
      expect(screen.getByText("detail-found")).toBeInTheDocument();
      expect(screen.getByText("Location")).toBeInTheDocument();
      expect(screen.getByText("Session")).toBeInTheDocument();
      expect(screen.getByText("Date")).toBeInTheDocument();
    });
  });

  describe("items", () => {
    it("shows a loading indicator while items are pending", () => {
      vi.mocked(invoke).mockReturnValue(new Promise(() => {}));
      seedReceipt();

      render(<ReceiptDetail id={42} />);

      expect(screen.getByText("Loading items…")).toBeInTheDocument();
    });

    it("does not update state after unmount when the item fetch resolves", async () => {
      let resolveItems: ((items: ItemDto[]) => void) | undefined;
      const promise = new Promise<ItemDto[]>((resolve) => {
        resolveItems = resolve;
      });
      vi.mocked(invoke).mockReturnValue(promise);
      const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);
      seedReceipt();

      const { unmount } = render(<ReceiptDetail id={42} />);
      unmount();
      resolveItems?.([]);
      await Promise.resolve();

      expect(
        consoleSpy.mock.calls.some((call) => call.some((part) => String(part).includes("unmounted"))),
      ).toBe(false);
      consoleSpy.mockRestore();
    });

    it("groups one model and omits zero cache rows", async () => {
      seedReceipt();
      vi.mocked(invoke).mockResolvedValue([
        makeItem({ id: 1, inputTokens: 10, outputTokens: 1, cacheCreationTokens: null, cacheReadTokens: null, cost: 0.1 }),
        makeItem({ id: 2, inputTokens: 20, outputTokens: 2, cacheCreationTokens: 0, cacheReadTokens: 0, cost: 0.2 }),
        makeItem({ id: 3, inputTokens: 30, outputTokens: 3, cacheCreationTokens: 0, cacheReadTokens: 0, cost: 0.3 }),
      ]);

      render(<ReceiptDetail id={42} />);

      expect(await screen.findByText("claude-sonnet-4")).toBeInTheDocument();
      expect(screen.getAllByText("$0.60")).toHaveLength(2);
      expect(screen.getByText("Input tokens")).toBeInTheDocument();
      expect(screen.getByText("60")).toBeInTheDocument();
      expect(screen.getByText("Output tokens")).toBeInTheDocument();
      expect(screen.getByText("6")).toBeInTheDocument();
      expect(screen.queryByText("Cache write")).toBeNull();
      expect(screen.queryByText("Cache read")).toBeNull();
    });

    it("orders model groups by subtotal cost descending", async () => {
      seedReceipt();
      vi.mocked(invoke).mockResolvedValue([
        makeItem({ id: 1, model: "A", cost: 0.4 }),
        makeItem({ id: 2, model: "A", cost: 0.6 }),
        makeItem({ id: 3, model: "B", cost: 5 }),
      ]);

      render(<ReceiptDetail id={42} />);
      await screen.findByText("B");

      const text = screen.getByTestId("receipt-paper").textContent ?? "";
      expect(text.indexOf("B")).toBeLessThan(text.indexOf("A"));
    });

    it("renders cache write rows only when cache write tokens are non-zero", async () => {
      seedReceipt();
      vi.mocked(invoke).mockResolvedValue([
        makeItem({ cacheCreationTokens: 1000, cacheReadTokens: 0 }),
      ]);

      render(<ReceiptDetail id={42} />);

      expect(await screen.findByText("Cache write")).toBeInTheDocument();
      expect(screen.queryByText("Cache read")).toBeNull();
    });

    it("renders the rounded total cost", async () => {
      seedReceipt();
      vi.mocked(invoke).mockResolvedValue([
        makeItem({ id: 1, model: "A", cost: 1.234 }),
        makeItem({ id: 2, model: "B", cost: 2.1 }),
        makeItem({ id: 3, model: "C", cost: 0.005 }),
      ]);

      render(<ReceiptDetail id={42} />);

      expect(await screen.findByText("TOTAL")).toBeInTheDocument();
      expect(screen.getByText("$3.34")).toBeInTheDocument();
    });

    it("renders zero total for empty items", async () => {
      seedReceipt();
      vi.mocked(invoke).mockResolvedValue([]);

      render(<ReceiptDetail id={42} />);

      await waitFor(() => expect(screen.queryByText("Loading items…")).toBeNull());
      expect(screen.getByText("TOTAL")).toBeInTheDocument();
      expect(screen.getByText("$0.00")).toBeInTheDocument();
    });

    it("renders cashier as the highest-subtotal model", async () => {
      seedReceipt();
      vi.mocked(invoke).mockResolvedValue([
        makeItem({ id: 1, model: "A", cost: 1 }),
        makeItem({ id: 2, model: "B", cost: 5 }),
      ]);

      render(<ReceiptDetail id={42} />);

      const cashier = await screen.findByText(/CASHIER:/);
      expect(cashier.textContent).toContain("B");
      expect(screen.getByText("Thank you for building!")).toBeInTheDocument();
    });

    it("omits cashier but keeps thank-you for empty items", async () => {
      seedReceipt();
      vi.mocked(invoke).mockResolvedValue([]);

      render(<ReceiptDetail id={42} />);

      await waitFor(() => expect(screen.queryByText("Loading items…")).toBeNull());
      expect(screen.queryByText(/CASHIER:/)).toBeNull();
      expect(screen.getByText("Thank you for building!")).toBeInTheDocument();
    });

    it("uses receipt styling classes", async () => {
      seedReceipt();
      vi.mocked(invoke).mockResolvedValue([makeItem({ cost: 1 })]);

      render(<ReceiptDetail id={42} />);
      await screen.findByText("claude-sonnet-4");

      const paper = screen.getByTestId("receipt-paper");
      expect(paper.className).toMatch(/font-mono|Courier/);
      expect(paper.className).toContain("bg-[#f8f8f8]");
      expect(paper.querySelector('[class*="border-t-2"]')).not.toBeNull();
      expect(paper.querySelector('[class*="border-dashed"]')).not.toBeNull();
    });

    it("keeps the header visible when item loading fails", async () => {
      seedReceipt();
      vi.mocked(invoke).mockRejectedValue(new Error("boom"));

      render(<ReceiptDetail id={42} />);

      expect(await screen.findByText("Failed to load items")).toBeInTheDocument();
      expect(screen.getByText("/tmp/detail-found")).toBeInTheDocument();
    });

    it("renders many items across all model groups with the correct total", async () => {
      seedReceipt();
      const items = Array.from({ length: 60 }, (_, index) => {
        const modelIndex = index % 3;
        const model = ["m1", "m2", "m3"][modelIndex] ?? "m1";
        return makeItem({
          id: index + 1,
          model,
          occurredAt: 100 + index,
          inputTokens: index + 1,
          outputTokens: index % 7,
          cacheCreationTokens: index % 5 === 0 ? 100 : null,
          cacheReadTokens: index % 4 === 0 ? 50 : 0,
          cost: [0.01, 0.02, 0.03][modelIndex] ?? 0.01,
        });
      });
      vi.mocked(invoke).mockResolvedValue(items);

      render(<ReceiptDetail id={42} />);

      expect(await screen.findByText("$1.20")).toBeInTheDocument();
      expect(screen.getByText("m1")).toBeInTheDocument();
      expect(screen.getByText("m2")).toBeInTheDocument();
      expect(screen.getByText("m3")).toBeInTheDocument();
    });
  });
});
