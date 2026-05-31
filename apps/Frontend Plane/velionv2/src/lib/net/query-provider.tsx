"use client";

/**
 * App-wide TanStack Query provider. Powers the polling + mutation surfaces
 * (onboarding graph preview / connect status, navbar, inbox) so we stop
 * hand-rolling in-flight refs and manual fetch state.
 */

import { useState } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

export function QueryProvider({ children }: { children: React.ReactNode }) {
  const [client] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: {
            // The control plane is per-user and changes often; favor fresh
            // reads but dedupe bursts. Individual queries override as needed.
            staleTime: 15_000,
            gcTime: 5 * 60_000,
            retry: 2,
            refetchOnWindowFocus: false,
          },
          mutations: {
            retry: 0,
          },
        },
      }),
  );

  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}
