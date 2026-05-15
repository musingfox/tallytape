import { createMemoryHistory, createRouter } from "@tanstack/react-router";
import { describe, expect, it } from "vitest";
import { routeTree } from "../../routeTree.gen";

describe("routeTree", () => {
  it("registers the index and receipt detail paths", () => {
    const router = createRouter({
      routeTree,
      history: createMemoryHistory({ initialEntries: ["/"] }),
    });

    const fullPaths = new Set(Object.values(router.routesById).map((route) => route.fullPath));

    expect(fullPaths).toContain("/");
    expect(fullPaths).toContain("/receipts/$id");
  });
});
