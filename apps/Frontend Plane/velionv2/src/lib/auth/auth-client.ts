"use client";

import { createAuthClient } from "better-auth/react";
import { twoFactorClient } from "better-auth/plugins/two-factor";
import { passkeyClient } from "@better-auth/passkey/client";

// In control-plane mode the browser talks to the same-origin /api/auth/*
// proxy, so the client base URL must be the app's own origin. Fall back to
// NEXT_PUBLIC_APP_URL, then to undefined (Better Auth uses window.origin).
const authClientBaseURL =
  process.env.NEXT_PUBLIC_AUTH_BASE_URL ||
  process.env.NEXT_PUBLIC_APP_URL ||
  undefined;

export const authClient = createAuthClient({
  baseURL: authClientBaseURL,
  plugins: [
    twoFactorClient({
      twoFactorPage: "/login",
    }),
    passkeyClient(),
  ],
});
