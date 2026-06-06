import type { Route } from "next";
import { redirect } from "next/navigation";
import { hasAuthDatabaseConfig } from "@/lib/auth/database";
import { getCurrentAuthUser, type AuthenticatedUser } from "@/lib/auth/current-auth-user";
import { markLocalOnboardingComplete } from "@/lib/auth/onboarding-completions";
import { getRequestAuthState } from "@/lib/auth/request-auth-state";
import { fetchUserCoreJson, UserCoreError } from "@/lib/integrations/user-core";

export type { UserCoreSessionContext } from "@/lib/auth/request-auth-state";
export { isCompletedOnboardingContext } from "@/lib/auth/request-auth-state";

type RequestActor = {
  userId: string;
  email?: string;
  name?: string;
  avatar?: string;
  cookieHeader?: string;
};

export type AuthGateState = {
  onboardingComplete: boolean;
  onboardingStatus: string | null;
  source: "anonymous" | "control-session" | "local-fallback" | "unknown" | "user-core";
  user: AuthenticatedUser | null;
};

export class AuthGateError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

function loginRedirect(callbackUrl: string) {
  return `/login?callbackUrl=${encodeURIComponent(callbackUrl)}` as Route;
}

function toRequestActor(user: AuthenticatedUser): RequestActor {
  return {
    userId: user.id,
    email: user.email,
    name: user.name,
    avatar: user.image ?? undefined,
    cookieHeader: user.cookieHeader,
  };
}

export async function getAuthGateState(): Promise<AuthGateState> {
  const state = await getRequestAuthState();

  return {
    user: state.user,
    onboardingComplete: state.onboardingComplete,
    onboardingStatus: state.onboardingStatus,
    source: state.source,
  };
}

export async function requireOnboardingAccess() {
  const gate = await getAuthGateState();

  if (!gate.user) {
    redirect(loginRedirect("/onboarding"));
  }

  if (gate.onboardingComplete) {
    redirect("/dashboard" as Route);
  }

  return gate;
}

export async function requireCompletedOnboarding(callbackUrl = "/dashboard") {
  const gate = await getAuthGateState();

  if (!gate.user) {
    redirect(loginRedirect(callbackUrl));
  }

  if (!gate.onboardingComplete) {
    redirect("/onboarding" as Route);
  }

  return gate;
}

export async function redirectAuthenticatedUserFromAuth() {
  const gate = await getAuthGateState();

  if (!gate.user) {
    return;
  }

  redirect((gate.onboardingComplete ? "/dashboard" : "/onboarding") as Route);
}

export async function markCurrentUserOnboardingComplete(payload?: Record<string, unknown>) {
  const user = await getCurrentAuthUser();

  if (!user) {
    throw new AuthGateError(401, "authentication_required", "Sign in is required.");
  }

  try {
    await fetchUserCoreJson(toRequestActor(user), "/api/v1/users/onboarding/complete", {
      method: "POST",
      body: JSON.stringify(payload ?? {}),
    });

    if (hasAuthDatabaseConfig()) {
      await markLocalOnboardingComplete(user.id);
    }

    return {
      onboardingComplete: true,
      source: "user-core",
    };
  } catch (error) {
    if (
      hasAuthDatabaseConfig() &&
      error instanceof UserCoreError &&
      [401, 403, 502, 503].includes(error.status)
    ) {
      await markLocalOnboardingComplete(user.id);
      return {
        onboardingComplete: true,
        source: "local-fallback",
      };
    }

    if (error instanceof UserCoreError) {
      throw new AuthGateError(error.status, error.code, error.message);
    }

    throw error;
  }
}
