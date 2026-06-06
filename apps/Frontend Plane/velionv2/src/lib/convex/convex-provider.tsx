"use client";

import { ConvexProviderWithAuth, ConvexReactClient } from "convex/react";
import type { ReactNode } from "react";

import { useConvexAuth } from "@/lib/convex/convex-auth";
import { getPublicConvexUrl } from "@/lib/convex/convex-env";

const CONVEX_URL = getPublicConvexUrl();

const convex = CONVEX_URL
  ? new ConvexReactClient(CONVEX_URL, {
      unsavedChangesWarning: false,
    })
  : null;

export function ConvexClientProvider({ children }: { children: ReactNode }) {
  if (!convex) {
    return <>{children}</>;
  }

  return (
    <ConvexProviderWithAuth client={convex} useAuth={useConvexAuth}>
      {children}
    </ConvexProviderWithAuth>
  );
}
