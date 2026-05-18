import { render, waitFor } from "@testing-library/react";
import { createMemoryHistory, createRouter, RouterProvider } from "@tanstack/react-router";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useReceiptStore } from "../../receipts/store";
import { routeTree } from "../../routeTree.gen";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: vi.fn(),
  requestPermission: vi.fn(),
  sendNotification: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import { requestPermission } from "@tauri-apps/plugin-notification";

function makeRouter() {
  return createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
}

beforeEach(() => {
  vi.useRealTimers();
  useReceiptStore.setState({ receipts: new Map() });
  vi.clearAllMocks();
  // Default: invoke list_receipts etc. return empty arrays; get_summary returns zeros
  vi.mocked(invoke).mockImplementation(async (cmd: string) => {
    if (cmd === "get_summary") {
      return { totalCost: 0, totalTokens: 0, sessionCount: 0, receiptCount: 0 };
    }
    return [];
  });
  vi.mocked(requestPermission).mockResolvedValue("granted" as NotificationPermission);
});

describe("First-run notification permission prompt (C6)", () => {
  it("requests permission and sets flag when notification_permission_asked is absent", async () => {
    // getAppSetting('notification_permission_asked') → null
    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      const a = args as Record<string, string> | undefined;
      if (cmd === "get_app_setting" && a?.key === "notification_permission_asked") return null;
      if (cmd === "set_app_setting") return undefined;
      if (cmd === "get_summary") return { totalCost: 0, totalTokens: 0, sessionCount: 0, receiptCount: 0 };
      return [];
    });

    render(<RouterProvider router={makeRouter()} />);

    await waitFor(() => {
      expect(vi.mocked(requestPermission)).toHaveBeenCalledOnce();
    });
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("set_app_setting", {
        key: "notification_permission_asked",
        value: "true",
      });
    });
  });

  it("does NOT request permission when notification_permission_asked is already set", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      const a = args as Record<string, string> | undefined;
      if (cmd === "get_app_setting" && a?.key === "notification_permission_asked") return "true";
      if (cmd === "get_summary") return { totalCost: 0, totalTokens: 0, sessionCount: 0, receiptCount: 0 };
      return [];
    });

    render(<RouterProvider router={makeRouter()} />);

    // Give the effect time to run
    await waitFor(() => {
      // setAppSetting should NOT have been called
      const setCalls = vi.mocked(invoke).mock.calls.filter(
        ([cmd]) => cmd === "set_app_setting",
      );
      expect(setCalls).toHaveLength(0);
    });
    expect(vi.mocked(requestPermission)).not.toHaveBeenCalled();
  });

  it("sets the flag even when requestPermission rejects (finally block)", async () => {
    vi.mocked(requestPermission).mockRejectedValue(new Error("permission API unavailable"));

    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      const a = args as Record<string, string> | undefined;
      if (cmd === "get_app_setting" && a?.key === "notification_permission_asked") return null;
      if (cmd === "set_app_setting") return undefined;
      if (cmd === "get_summary") return { totalCost: 0, totalTokens: 0, sessionCount: 0, receiptCount: 0 };
      return [];
    });

    // Root should still render without throwing
    const { container } = render(<RouterProvider router={makeRouter()} />);
    expect(container).toBeTruthy();

    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("set_app_setting", {
        key: "notification_permission_asked",
        value: "true",
      });
    });
  });

  it("second render with flag already set does NOT trigger another prompt", async () => {
    // First render: flag absent → sets it
    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      const a = args as Record<string, string> | undefined;
      if (cmd === "get_app_setting" && a?.key === "notification_permission_asked") return "true";
      if (cmd === "get_summary") return { totalCost: 0, totalTokens: 0, sessionCount: 0, receiptCount: 0 };
      return [];
    });

    render(<RouterProvider router={makeRouter()} />);

    // After settle, requestPermission must not have been called
    await new Promise((r) => setTimeout(r, 100));
    expect(vi.mocked(requestPermission)).not.toHaveBeenCalled();
  });
});
