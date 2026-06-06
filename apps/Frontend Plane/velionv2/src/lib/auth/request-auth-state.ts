import "server-only";
import { cache } from "react";
import { readLocalOnboardingComplete } from "@/lib/auth/onboarding-completions";
import { getCurrentAuthUser, type AuthenticatedUser } from "@/lib/auth/current-auth-user";
import { type ControlSession, fetchControlSession, isControlSessionAuthorityEnabled } from "@/lib/integrations/session-core";
import { fetchUserCoreJson, UserCoreError } from "@/lib/integrations/user-core";

export type UserCoreSessionContext = {
  userId?: string;
  orgId?: string | null;
  role?: string | null;
  onboardingStatus?: string | null;
  onboarding_complete?: boolean;
  onboardingComplete?: boolean;
};

export type RequestAuthState = {
  controlSession: ControlSession | null;
  onboardingComplete: boolean;
  onboardingStatus: string | null;
  orgId: string | null;
  role: string | null;
  sessionContext: UserCoreSessionContext | null;
  source: "anonymous" | "control-session" | "local-fallback" | "unknown" | "user-core";
  user: AuthenticatedUser | null;
};

type RequestActorLike = {
  avatar?: string;
  cookieHeader?: string;
  email?: string;
  name?: string;
  userId: string;
};

function toRequestActor(user: AuthenticatedUser): RequestActorLike {
  return {
    userId: user.id,
    email: user.email,
    name: user.name,
    avatar: user.image ?? undefined,
    cookieHeader: user.cookieHeader,
  };
}

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

async function readUserCoreOnboardingContext(user: AuthenticatedUser) {
  return fetchUserCoreJson<UserCoreSessionContext>(
    toRequestActor(user),
    "/api/v1/me/session-context",
  );
}

export const getRequestAuthState = cache(async (): Promise<RequestAuthState> => {
  const user = await getCurrentAuthUser();

  if (!user) {
    return {
      user: null,
      controlSession: null,
      onboardingComplete: false,
      onboardingStatus: null,
      orgId: null,
      role: null,
      sessionContext: null,
      source: "anonymous",
    };
  }

  if (user.testAuth) {
    return {
      user,
      controlSession: null,
      onboardingComplete: true,
      onboardingStatus: "COMPLETED",
      orgId: user.testOrgId ?? "org_playwright",
      role: "owner",
      sessionContext: null,
      source: "local-fallback",
    };
  }

  if (isControlSessionAuthorityEnabled()) {
    try {
      const controlSession = await fetchControlSession(toRequestActor(user));
      if (controlSession) {
        const onboardingComplete =
          controlSession.user.onboardingComplete === true ||
          controlSession.onboardingStatus === "COMPLETED";

        return {
          user: {
            ...user,
            email: controlSession.user.email ?? user.email,
            image: controlSession.user.image ?? user.image,
            name: controlSession.user.name ?? user.name,
          },
          controlSession,
          onboardingComplete,
          onboardingStatus: controlSession.onboardingStatus ?? (onboardingComplete ? "COMPLETED" : null),
          orgId: controlSession.organization?.id ?? null,
          role: controlSession.organization?.role ?? null,
          sessionContext: null,
          source: "control-session",
        };
      }
    } catch {
      // Fall through to the user-core onboarding snapshot.
    }
  }

  let sessionContext: UserCoreSessionContext | null = null;
  try {
    sessionContext = await readUserCoreOnboardingContext(user);

    if (hasAuthoritativeOnboardingContext(sessionContext)) {
      const localComplete = isCompletedOnboardingContext(sessionContext)
        ? false
        : await readLocalOnboardingComplete(user.id);

      return {
        user,
        controlSession: null,
        onboardingComplete: isCompletedOnboardingContext(sessionContext) || localComplete,
        onboardingStatus: isCompletedOnboardingContext(sessionContext) || localComplete
          ? "COMPLETED"
          : sessionContext.onboardingStatus ?? null,
        orgId: sessionContext.orgId ?? null,
        role: sessionContext.role ?? null,
        sessionContext,
        source: localComplete ? "local-fallback" : "user-core",
      };
    }
  } catch (error) {
    if (!(error instanceof UserCoreError) || ![401, 403, 500, 502, 503].includes(error.status)) {
      throw error;
    }
  }

  const onboardingComplete = await readLocalOnboardingComplete(user.id);

  return {
    user,
    controlSession: null,
    onboardingComplete,
    onboardingStatus: onboardingComplete ? "COMPLETED" : sessionContext?.onboardingStatus ?? null,
    orgId: sessionContext?.orgId ?? null,
    role: sessionContext?.role ?? null,
    sessionContext,
    source: onboardingComplete ? "local-fallback" : "unknown",
  };
});
