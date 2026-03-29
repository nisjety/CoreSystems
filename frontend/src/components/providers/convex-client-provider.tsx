"use client";

import { ConvexReactClient } from "convex/react";
import { ConvexProviderWithAuth } from "convex/react";
import { ReactNode, useCallback, useMemo } from "react";
import { useAuth } from "../auth/hooks/use-auth";

// Expecting environment variable to be set
const CONVEX_URL = process.env.NEXT_PUBLIC_CONVEX_URL || "http://127.0.0.1:3000";

const convex = new ConvexReactClient(CONVEX_URL, {
    unsavedChangesWarning: false,
});

export function ConvexClientProvider({ children }: { children: ReactNode }) {
    const { user, isLoading } = useAuth();

    // Create a memoized auth state for Convex
    // Convex requires us to tell it when the user is authenticated 
    // and how to fetch an access token if it's needed for the connection.
    const isAuthenticated = useMemo(() => user !== null, [user]);

    // Better Auth sessions operate primarily through cookies, but if Convex
    // is strictly expecting a JWT for WS connections, we can try to fetch a token 
    // from a backend proxy or pass a default string for local development
    // where Convex gateway might just trust the proxy.
    const fetchAccessToken = useCallback(async ({ forceRefreshToken }: { forceRefreshToken: boolean }) => {
        try {
            if (!user) return null;

            // Note: Since Convex in this setup uses a custom auth integration 
            // via externalAuthId (in useConvexChat), we provide the session token
            // as the JWT proof for the connection.
            const { authClient } = await import("@/components/auth/lib/auth-client-enterprise");
            const sessionResult = await authClient.getSession();

            if (sessionResult && sessionResult.data?.session) {
                // Using session ID or token as the secure opaque token for Convex
                const session = sessionResult.data.session as any;
                return session.token || session.id || user.id;
            }

            return null;
        } catch (err) {
            console.error("Failed to fetch access token for Convex:", err);
            return null;
        }
    }, [user]);

    return (
        <ConvexProviderWithAuth
            client={convex}
            useAuth={() => ({
                isLoading,
                isAuthenticated,
                fetchAccessToken,
            })}
        >
            {children}
        </ConvexProviderWithAuth>
    );
}
