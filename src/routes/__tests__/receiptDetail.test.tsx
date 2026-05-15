import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { useReceiptStore } from "../../receipts/store";
import { ReceiptDetail } from "../receipts/$id";

beforeEach(() => {
  useReceiptStore.setState({ receipts: new Map() });
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
});
