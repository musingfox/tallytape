import { createFileRoute } from "@tanstack/react-router";
import { useReceiptStore } from "../../receipts/store";

export function parseParams(raw: { id: string }): { id: number } {
  if (!/^\d+$/.test(raw.id)) {
    throw new Error(`Invalid receipt id: ${raw.id}`);
  }
  return { id: Number(raw.id) };
}

export function ReceiptDetail({ id }: { id: number }) {
  const receipt = useReceiptStore((s) => s.receipts.get(id));

  if (receipt === undefined) {
    return <p>Not Found: receipt {id}</p>;
  }

  return (
    <div>
      <p>{receipt.cwd}</p>
      <p>{receipt.date}</p>
      <p>{receipt.updatedAt}</p>
    </div>
  );
}

function ReceiptDetailRoute() {
  const { id } = Route.useParams();
  return <ReceiptDetail id={id} />;
}

export const Route = createFileRoute("/receipts/$id")({
  parseParams,
  component: ReceiptDetailRoute,
});
