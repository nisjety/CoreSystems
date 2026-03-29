import { createAuthClient } from "better-auth/react";
import { genericOAuthClient, twoFactorClient, phoneNumberClient, passkeyClient, adminClient } from "better-auth/client/plugins";

export const authClient = createAuthClient({
  // Prefer same-origin in the browser to avoid hitting the auth service directly.
  baseURL: typeof window !== 'undefined'
    ? window.location.origin
    : process.env.NEXT_PUBLIC_APP_URL || "http://localhost:3000",
  basePath: "/api/auth", // This proxies to the auth service
  plugins: [
    genericOAuthClient(), // Required for custom OAuth providers (Okta, Vipps)
    twoFactorClient({
      onTwoFactorRedirect() {
        // Handle 2FA verification redirect
        window.location.href = "/2fa-verification";
      },
    }),
    phoneNumberClient(),
    passkeyClient(), // Add passkey support
    adminClient(), // Add admin client plugin for user management
  ],
});

export type AuthClient = typeof authClient;
