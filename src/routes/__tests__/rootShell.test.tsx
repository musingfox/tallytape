import { render, screen, waitFor } from "@testing-library/react";
import { createMemoryHistory, createRouter, RouterProvider } from "@tanstack/react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useReceiptStore } from "../../receipts/store";
import { routeTree } from "../../routeTree.gen";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue([]),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

beforeEach(() => {
  useReceiptStore.setState({ receipts: new Map() });
  vi.clearAllMocks();
});

describe("RootShellRendersTitleAndOutlet", () => {
  it("renders banner with TallyTape and the index page Count: heading", async () => {
    const router = createRouter({
      routeTree,
      history: createMemoryHistory({ initialEntries: ["/"] }),
    });

    render(<RouterProvider router={router} />);

    await waitFor(() => {
      expect(screen.getByRole("banner").textContent).toContain("TallyTape");
      expect(screen.getByText(/Count:/)).toBeTruthy();
    });
  });
});
