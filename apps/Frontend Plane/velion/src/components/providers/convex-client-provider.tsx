"use client";

import { ConvexReactClient } from "convex/react";
import { ConvexProviderWithAuth } from "convex/react";
import { ReactNode, useCallback, useMemo, useRef } from "react";
import { useAuth } from "../auth/hooks/use-auth";

// Expecting environment variable to be set
const CONVEX_URL =
    process.env.NEXT_PUBLIC_CONVEX_URL ||
    process.env.NEXT_PUBLIC_CONVEX_HTTP ||
    "http://127.0.0.1:3210";

const convex = new ConvexReactClient(CONVEX_URL, {
    unsavedChangesWarning: false,
});

type ConvexTokenResponse = {
    token: string;
    expiresAt: string;
};

/**
 * Hoisted hook so its identity is stable across `ConvexClientProvider`
 * renders. Passing an inline arrow to `useAuth` worked in practice but
 * confused some users / linters; this is the canonical shape from the
 * Convex docs.
 *
 * The hook receives nothing — it pulls auth state via the velion
 * `useAuth` hook and exposes the contract Convex expects. The returned
 * object is memoized so its identity only changes when the underlying
 * auth values change. Without this memo, every parent render of
 * ConvexClientProvider produced a fresh `{ isLoading, isAuthenticated,
 * fetchAccessToken }` literal — ConvexProviderWithAuth interpreted that
 * as "auth changed", re-ran its connect/auth flow, and fired the
 * /api/convex-auth/token route ~12s apart. That cycle was the source
 * of the chat + sidebar flicker.
 */
function useConvexAuthBridge() {
    const { user, isLoading } = useAuth();
    const isAuthenticated = user !== null;

    // Keep a stable reference to the current user id so fetchAccessToken
    // can read it without participating in the callback's dep array
    // (which would re-create the callback on every auth tick).
    const userIdRef = useRef<string | null>(null);
    userIdRef.current = user?.id ?? null;

    const fetchAccessToken = useCallback(
        async ({ forceRefreshToken }: { forceRefreshToken: boolean }) => {
            try {
                if (!userIdRef.current) return null;

                const response = await fetch("/api/convex-auth/token", {
                    method: "GET",
                    cache: "no-store",
                    credentials: "include",
                    headers: forceRefreshToken
                        ? {
                              "Cache-Control": "no-cache",
                              Pragma: "no-cache",
                          }
                        : undefined,
                });

                if (!response.ok) {
                    console.error(
                        "Failed to fetch Convex auth token:",
                        response.status,
                    );
                    return null;
                }

                const data = (await response.json()) as Partial<ConvexTokenResponse>;
                return typeof data.token === "string" && data.token.length > 0
                    ? data.token
                    : null;
            } catch (err) {
                console.error("Failed to fetch access token for Convex:", err);
                return null;
            }
        },
        // Empty deps: the callback closes over a ref, so its identity is
        // stable for the component's lifetime. This is what stops the
        // reconnect loop — Convex sees the same callback reference and
        // does not invalidate its auth on every render.
        [],
    );

    return useMemo(
        () => ({ isLoading, isAuthenticated, fetchAccessToken }),
        [isLoading, isAuthenticated, fetchAccessToken],
    );
}

export function ConvexClientProvider({ children }: { children: ReactNode }) {
    return (
        <ConvexProviderWithAuth client={convex} useAuth={useConvexAuthBridge}>
            {children}
        </ConvexProviderWithAuth>
    );
}
