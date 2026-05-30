import "server-only"
import { cache } from "react"
import { getCurrentAuthUser } from "@/lib/auth/onboarding-access"
import { fetchBillingAccount } from "@/lib/integrations/billing-core"
import {
  entitlementsToFeatures,
  fetchControlSession,
  isControlSessionAuthorityEnabled,
} from "@/lib/integrations/session-core"
import { fetchUserCoreJson, UserCoreError } from "@/lib/integrations/user-core"
import type { RequestActor } from "@/lib/integrations/request-actor"
import {
  ANONYMOUS_CONTROL_PLANE_CONTEXT,
  type ControlPlaneContextValue,
  type ControlPlaneEntitlements,
} from "@/lib/control-plane/context-types"

type SessionContext = {
  userId?: string
  orgId?: string | null
  role?: string | null
  onboardingStatus?: string | null
  onboarding_complete?: boolean
  onboardingComplete?: boolean
}

function isComplete(ctx: SessionContext | null): boolean {
  return (
    ctx?.onboardingStatus === "COMPLETED" ||
    ctx?.onboarding_complete === true ||
    ctx?.onboardingComplete === true
  )
}

/**
 * Composes the full Control Plane context for the current request:
 *   auth-core (identity) + user-core /me/session-context (org/role/onboarding)
 *   + billing-core account (plan/entitlements/quotas).
 *
 * Wrapped in React `cache()` so all components in one render share a single
 * fetch. Never throws on a billing-core outage — entitlements degrade to null.
 */
export const getControlPlaneContext = cache(
  async (): Promise<ControlPlaneContextValue> => {
    const user = await getCurrentAuthUser()
    if (!user) return ANONYMOUS_CONTROL_PLANE_CONTEXT

    const actor: RequestActor = {
      userId: user.id,
      email: user.email,
      name: user.name,
      avatar: user.image ?? undefined,
      cookieHeader: user.cookieHeader,
    }

    // Single-call aggregator path (ADR 0002 / G10): session-core composes
    // identity + org + entitlements + billing in one cached call. Falls through
    // to the user-core composition below on any failure.
    if (isControlSessionAuthorityEnabled()) {
      try {
        const snap = await fetchControlSession(actor)
        if (snap) {
          const onboardingComplete =
            snap.user.onboardingComplete === true ||
            snap.onboardingStatus === "COMPLETED"
          const hasBilling =
            snap.billing != null ||
            (Array.isArray(snap.entitlements) && snap.entitlements.length > 0)
          return {
            user: {
              id: user.id,
              email: snap.user.email ?? user.email,
              name: snap.user.name ?? user.name,
              image: snap.user.image ?? user.image ?? null,
            },
            orgId: snap.organization?.id ?? null,
            role: snap.organization?.role ?? null,
            onboardingStatus:
              snap.onboardingStatus ?? (onboardingComplete ? "COMPLETED" : null),
            onboardingComplete,
            entitlements: hasBilling
              ? {
                  plan: snap.billing?.plan ?? snap.organization?.plan ?? "free",
                  subscriptionStatus: snap.billing?.status ?? "active",
                  features: entitlementsToFeatures(snap.entitlements),
                  quotas: {},
                  credits: 0,
                }
              : null,
          }
        }
      } catch {
        // session-core unavailable — fall back to the user-core composition.
      }
    }

    let session: SessionContext | null = null
    try {
      session = await fetchUserCoreJson<SessionContext>(
        actor,
        "/api/v1/me/session-context",
      )
    } catch (error) {
      // Tolerate user-core unavailability; do not blow up the page render.
      if (!(error instanceof UserCoreError)) throw error
    }

    const orgId = session?.orgId ?? null
    const onboardingComplete = isComplete(session)

    let entitlements: ControlPlaneEntitlements | null = null
    if (orgId) {
      try {
        const account = await fetchBillingAccount(actor, orgId)
        if (account) {
          entitlements = {
            plan: account.plan,
            subscriptionStatus: account.subscriptionStatus,
            features: account.entitlements,
            quotas: account.quotaLimits,
            credits: account.credits,
          }
        }
      } catch {
        // Billing is eventually-consistent (NATS-provisioned). Never block.
        entitlements = null
      }
    }

    return {
      user: {
        id: user.id,
        email: user.email,
        name: user.name,
        image: user.image ?? null,
      },
      orgId,
      role: session?.role ?? null,
      onboardingStatus:
        session?.onboardingStatus ?? (onboardingComplete ? "COMPLETED" : null),
      onboardingComplete,
      entitlements,
    }
  },
)
