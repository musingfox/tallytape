import { createRootRoute, Outlet } from "@tanstack/react-router";
import { lazy, Suspense } from "react";

const TanStackRouterDevtools = import.meta.env.PROD
  ? () => null
  : lazy(() =>
      import("@tanstack/router-devtools").then((m) => ({
        default: m.TanStackRouterDevtools,
      })),
    );

export const Route = createRootRoute({
  component: () => (
    <>
      <header role="banner">
        <h1>TallyTape</h1>
      </header>
      <main>
        <Outlet />
      </main>
      <Suspense>
        <TanStackRouterDevtools />
      </Suspense>
    </>
  ),
});
