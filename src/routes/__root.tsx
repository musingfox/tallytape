import { Link, createRootRoute, Outlet } from "@tanstack/react-router";
import { lazy, Suspense, useEffect } from "react";
import { Toaster } from "../components/Toaster";
import { getAppSetting, requestNotificationPermission, setAppSetting } from "../ipc";

const TanStackRouterDevtools = import.meta.env.PROD
  ? () => null
  : lazy(() =>
      import("@tanstack/router-devtools").then((m) => ({
        default: m.TanStackRouterDevtools,
      })),
    );

function RootComponent() {
  useEffect(() => {
    (async () => {
      const asked = await getAppSetting("notification_permission_asked");
      if (asked === null) {
        try {
          await requestNotificationPermission();
        } finally {
          await setAppSetting("notification_permission_asked", "true");
        }
      }
    })().catch(console.error);
  }, []);

  return (
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
  );
}

export const Route = createRootRoute({
  component: RootComponent,
});
