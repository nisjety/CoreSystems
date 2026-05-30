import type { Route } from "next";
import { headers } from "next/headers";
import { redirect } from "next/navigation";
import {
  getControlPlaneCurrentUser,
  isControlPlaneAuthConfigured,
} from "@/lib/auth/control-plane";
import { getAuthDatabasePool, hasAuthDatabaseConfig } from "@/lib/auth/database";
import type { RequestActor } from "@/lib/integrations/request-actor";
import { fetchUserCoreJson, UserCoreError } from "@/lib/integrations/user-core";

type BetterAuthUser = {
  id: string;
  email?: string;
  image?: string | null;
  name?: string;
};

type AuthenticatedUser = BetterAuthUser & {
  cookieHeader?: string;
};

export type UserCoreSessionContext = {
  userId?: string;
  orgId?: string | null;
  role?: string | null;
  onboardingStatus?: string | null;
  onboarding_complete?: boolean;
  onboardingComplete?: boolean;
};

export type AuthGateState = {
  onboardingComplete: boolean;
  onboardingStatus: string | null;
  source: "anonymous" | "local-fallback" | "unknown" | "user-core";
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

let onboardingTableReady: Promise<void> | null = null;

export function isCompletedOnboardingContext(context: UserCoreSessionContext | null | undefined) {
  return context?.onboardingStatus === "COMPLETED" ||
    context?.onboarding_complete === true ||
    context?.onboardingComplete === true;
}

function hasAuthoritativeOnboardingContext(context: UserCoreSessionContext | null | undefined) {
  return Boolean(
    context?.onboardingStatus ||
      typeof context?.onboarding_complete === "boolean" ||
      typeof context?.onboardingComplete === "boolean",
  );
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

async function ensureOnboardingCompletionTable() {
  onboardingTableReady ??= getAuthDatabasePool().query(`
    CREATE TABLE IF NOT EXISTS velion_onboarding_completions (
      user_id TEXT PRIMARY KEY,
      completed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
      updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
    )
  `).then(() => undefined);

  return onboardingTableReady;
}

async function readLocalOnboardingComplete(userId: string) {
  if (!hasAuthDatabaseConfig()) {
    return false;
  }

  await ensureOnboardingCompletionTable();
  const result = await getAuthDatabasePool().query<{ completed_at: Date }>(
    "SELECT completed_at FROM velion_onboarding_completions WHERE user_id = $1 LIMIT 1",
    [userId],
  );

  return (result.rowCount ?? 0) > 0;
}

async function markLocalOnboardingComplete(userId: string) {
  await ensureOnboardingCompletionTable();
  await getAuthDatabasePool().query(
    `
      INSERT INTO velion_onboarding_completions (user_id, completed_at, updated_at)
      VALUES ($1, NOW(), NOW())
      ON CONFLICT (user_id)
      DO UPDATE SET updated_at = NOW()
    `,
    [userId],
  );
}

export async function getCurrentAuthUser(): Promise<AuthenticatedUser | null> {
  const headerList = await headers();

  if (isControlPlaneAuthConfigured()) {
    try {
      const user = await getControlPlaneCurrentUser(headerList);
      if (!user) {
        return null;
      }

      return {
        id: user.id,
        email: user.email,
        image: user.image,
        name: user.name,
        cookieHeader: headerList.get("cookie") ?? undefined,
      };
    } catch {
      return null;
    }
  }

  if (!hasAuthDatabaseConfig()) {
    return null;
  }

  try {
    const { auth } = await import("@/lib/auth/auth");
    const session = await auth.api.getSession({ headers: headerList });
    const user = session?.user as BetterAuthUser | undefined;

    if (!user?.id) {
      return null;
    }

    return {
      id: user.id,
      email: user.email,
      image: user.image,
      name: user.name,
      cookieHeader: headerList.get("cookie") ?? undefined,
    };
  } catch {
    return null;
  }
}

async function readUserCoreOnboardingContext(user: AuthenticatedUser) {
  return fetchUserCoreJson<UserCoreSessionContext>(
    toRequestActor(user),
    "/api/v1/me/session-context",
  );
}

export async function getAuthGateState(): Promise<AuthGateState> {
  const user = await getCurrentAuthUser();

  if (!user) {
    return {
      user: null,
      onboardingComplete: false,
      onboardingStatus: null,
      source: "anonymous",
    };
  }

  try {
    const context = await readUserCoreOnboardingContext(user);

    if (hasAuthoritativeOnboardingContext(context)) {
      const localComplete = isCompletedOnboardingContext(context)
        ? false
        : await readLocalOnboardingComplete(user.id);

      return {
        user,
        onboardingComplete: isCompletedOnboardingContext(context) || localComplete,
        onboardingStatus: isCompletedOnboardingContext(context) || localComplete
          ? "COMPLETED"
          : context.onboardingStatus ?? null,
        source: localComplete ? "local-fallback" : "user-core",
      };
    }
  } catch (error) {
    if (!(error instanceof UserCoreError) || ![401, 403, 502, 503].includes(error.status)) {
      throw error;
    }
  }

  const onboardingComplete = await readLocalOnboardingComplete(user.id);

  return {
    user,
    onboardingComplete,
    onboardingStatus: onboardingComplete ? "COMPLETED" : null,
    source: onboardingComplete ? "local-fallback" : "unknown",
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

export async function markCurrentUserOnboardingComplete() {
  const user = await getCurrentAuthUser();

  if (!user) {
    throw new AuthGateError(401, "authentication_required", "Sign in is required.");
  }

  try {
    await fetchUserCoreJson(toRequestActor(user), "/api/v1/users/onboarding/complete", {
      method: "POST",
      body: JSON.stringify({}),
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
