import { Link, createRootRoute, Outlet } from "@tanstack/react-router";
import { lazy, Suspense } from "react";
import { Toaster } from "../components/Toaster";

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
        <Link to="/dashboard">Dashboard</Link>
      </header>
      <main>
        <Outlet />
      </main>
      <Toaster />
      <Suspense>
        <TanStackRouterDevtools />
      </Suspense>
    </>
  ),
});
