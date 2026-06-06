import type { Metadata } from "next";
import { VelionAuthPage } from "@/features/auth/components/VelionAuthPage";
import type { AuthMode } from "@/features/auth/lib/auth-schema";
import { redirectAuthenticatedUserFromAuth } from "@/lib/auth/onboarding-access";

export const metadata: Metadata = {
  title: "Sign in | Velion v2",
  description: "Velion secure authentication.",
  robots: { index: false, follow: false },
};

const VALID_MODES: ReadonlySet<AuthMode> = new Set<AuthMode>(["signin", "signup", "forgot", "reset", "sso"]);

function first(value: string | string[] | undefined): string | undefined {
  return Array.isArray(value) ? value[0] : value;
}

export default async function LoginPage({
  searchParams,
}: {
  searchParams: Promise<{ callbackUrl?: string | string[]; mode?: string | string[]; token?: string | string[] }>;
}) {
  const { callbackUrl, mode, token } = await searchParams;
  const resetToken = first(token);
  const requestedMode = first(mode);
  const initialMode: AuthMode = resetToken
    ? "reset"
    : requestedMode && VALID_MODES.has(requestedMode as AuthMode)
      ? (requestedMode as AuthMode)
      : "signin";

  // Password-reset links arrive with a token; don't bounce the (possibly
  // signed-in) visitor away before they can set a new password.
  if (initialMode !== "reset") {
    await redirectAuthenticatedUserFromAuth();
  }

  return (
    <VelionAuthPage
      callbackUrl={first(callbackUrl)}
      initialMode={initialMode}
      resetToken={resetToken}
    />
  );
}
