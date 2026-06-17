'use client';

import { useEffect } from 'react';
import { useRouter } from 'next/navigation';

const WARM_ROUTES = [
  '/dashboard',
  '/chat',
  '/search',
  '/knowledge',
  '/planner',
  '/settings/integrations',
  '/notifications',
] as const;

type IdleWindow = Window & {
  requestIdleCallback?: (callback: () => void, options?: { timeout: number }) => number;
  cancelIdleCallback?: (handle: number) => void;
};

export function DashboardRouteWarmup() {
  const router = useRouter();

  useEffect(() => {
    const win = window as IdleWindow;
    const warmRoutes = () => {
      WARM_ROUTES.forEach((route) => {
        router.prefetch(route);
      });
    };

    if (typeof win.requestIdleCallback === 'function') {
      const handle = win.requestIdleCallback(warmRoutes, { timeout: 1500 });
      return () => {
        if (typeof win.cancelIdleCallback === 'function') {
          win.cancelIdleCallback(handle);
        }
      };
    }

    const timeout = window.setTimeout(warmRoutes, 400);
    return () => {
      window.clearTimeout(timeout);
    };
  }, [router]);

  return null;
}
