import { createFileRoute } from "@tanstack/react-router";
import { useCounterStore } from "../store/counter";

export const Route = createFileRoute("/")({
  component: Index,
});

function Index() {
  const count = useCounterStore((state) => state.count);
  const increment = useCounterStore((state) => state.increment);

  return (
    <div className="p-4">
      <h1 className="text-blue-500 text-2xl font-bold">TallyTape</h1>
      <p className="mt-2">Count: {count}</p>
      <button className="mt-2 px-4 py-2 bg-blue-500 text-white rounded" onClick={increment}>
        Increment
      </button>
    </div>
  );
}
