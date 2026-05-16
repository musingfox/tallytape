import { useEffect } from "react";
import { useReceiptStore, type ReceiptError } from "../receipts/store";

function Toast({ error }: { error: ReceiptError }) {
  const dismissError = useReceiptStore((state) => state.dismissError);

  useEffect(() => {
    const timeout = window.setTimeout(() => dismissError(error.id), 5000);
    return () => window.clearTimeout(timeout);
  }, [dismissError, error.id]);

  return (
    <div
      role="status"
      aria-live="polite"
      className="max-w-[360px] rounded border border-red-300 bg-[#fff0f0] px-3 py-2 font-mono text-sm text-red-900 shadow"
    >
      <div className="flex items-start gap-3">
        <span className="min-w-0 flex-1">{error.message}</span>
        <button
          type="button"
          aria-label="Dismiss"
          className="font-bold text-red-900"
          onClick={() => dismissError(error.id)}
        >
          ×
        </button>
      </div>
    </div>
  );
}

export function Toaster() {
  const errors = useReceiptStore((state) => state.errors);

  if (errors.length === 0) {
    return null;
  }

  return (
    <div className="fixed right-4 bottom-4 z-50 flex flex-col gap-2">
      {errors.map((error) => (
        <Toast key={error.id} error={error} />
      ))}
    </div>
  );
}
