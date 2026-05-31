import "server-only"
import type { Route } from "next"
import { redirect } from "next/navigation"
import { getControlPlaneContext } from "@/lib/auth/control-plane-context"
import {
  hasFeature,
  type ControlPlaneEntitlements,
} from "@/lib/control-plane/context-types"

/**
 * Server-side paywall enforcement, backed by REAL billing-core entitlements
 * (composed in getControlPlaneContext, which is request-memoized). Use these
 * in Server Components / route handlers to gate premium capabilities.
 */

export async function getEntitlements(): Promise<ControlPlaneEntitlements | null> {
  return (await getControlPlaneContext()).entitlements
}

export async function hasEntitlement(feature: string): Promise<boolean> {
  return hasFeature(await getControlPlaneContext(), feature)
}

/**
 * Guard a Server Component on a feature flag. Redirects to the billing/upgrade
 * page when the org's plan does not grant the feature.
 */
export async function requireEntitlement(
  feature: string,
  opts?: { redirectTo?: string },
): Promise<void> {
  const granted = await hasEntitlement(feature)
  if (!granted) {
    const target =
      opts?.redirectTo ??
      `/settings/billing?upgrade=${encodeURIComponent(feature)}`
    redirect(target as Route)
  }
}

/** Quota helper: remaining allowance for a metric (Infinity when unlimited). */
export async function getQuotaLimit(metric: string): Promise<number> {
  const entitlements = await getEntitlements()
  const limit = entitlements?.quotas?.[metric]
  return typeof limit === "number" ? limit : Infinity
}
