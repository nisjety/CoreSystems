"use client";

import { useCallback, useMemo } from "react";

import { authClient } from "@/lib/auth/auth-client";

type ConvexTokenResponse = {
  token?: string;
  access_token?: string;
  jwt?: string;
};

async function requestConvexAccessToken(forceRefreshToken: boolean) {
  const response = await fetch("/api/convex-auth/token", {
    cache: "no-store",
    credentials: "include",
    headers: forceRefreshToken ? { "x-convex-force-refresh": "true" } : undefined,
  });

  if (!response.ok) {
    return null;
  }

  const body = (await response.json().catch(() => null)) as ConvexTokenResponse | null;
  return body?.token ?? body?.access_token ?? body?.jwt ?? null;
}

export function useConvexAuth() {
  const session = authClient.useSession();

  const fetchAccessToken = useCallback(
    async ({ forceRefreshToken }: { forceRefreshToken: boolean }) =>
      requestConvexAccessToken(forceRefreshToken),
    [],
  );

  return useMemo(
    () => ({
      isLoading: session.isPending,
      isAuthenticated: Boolean(session.data?.session && session.data?.user),
      fetchAccessToken,
    }),
    [fetchAccessToken, session.data, session.isPending],
  );
}
