import "server-only"
import type { RequestActor } from "@/lib/integrations/request-actor"

/**
 * Server-side billing-core client. Mirrors user-core.ts: internal-key auth,
 * RequestActor-scoped headers, request timeout. Used by the Control Plane
 * context composer and server-side entitlement enforcement.
 */
export class BillingCoreError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message)
  }
}

function getBillingCoreUrl() {
  return (
    process.env.BILLING_SERVICE_URL ??
    process.env.BILLING_CORE_URL ??
    "http://localhost:3014"
  ).replace(/\/+$/, "")
}

function getInternalApiKey() {
  return process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET
}

function buildHeaders(actor: RequestActor) {
  const internalApiKey = getInternalApiKey()
  if (!internalApiKey) {
    throw new BillingCoreError(
      503,
      "billing_core_key_not_configured",
      "INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET is required for billing-core.",
    )
  }

  const headers = new Headers({
    "X-Internal-Api-Key": internalApiKey,
    "X-User-Id": actor.userId,
    "Content-Type": "application/json",
  })
  if (actor.email) headers.set("X-User-Email", actor.email)
  if (actor.cookieHeader) headers.set("Cookie", actor.cookieHeader)
  return headers
}

export type BillingAccount = {
  orgId: string
  plan: string
  subscriptionStatus: string
  entitlements: Record<string, boolean>
  quotaLimits: Record<string, number>
  credits: number
}

type RawBillingAccount = {
  org_id: string
  plan?: string
  subscription_state?: string
  entitlements?: Record<string, boolean>
  quota_limits?: Record<string, number>
  credits?: number
}

export async function fetchBillingAccount(
  actor: RequestActor,
  orgId: string,
): Promise<BillingAccount | null> {
  const response = await fetch(
    `${getBillingCoreUrl()}/api/v1/billing/orgs/${orgId}/account`,
    {
      headers: buildHeaders(actor),
      cache: "no-store",
      signal: AbortSignal.timeout(5000),
    },
  ).catch((error: unknown) => {
    throw new BillingCoreError(
      502,
      "billing_core_unreachable",
      error instanceof Error ? error.message : "billing-core request failed",
    )
  })

  if (response.status === 404) return null
  if (!response.ok) {
    throw new BillingCoreError(
      response.status,
      "billing_core_error",
      `billing-core returned ${response.status}`,
    )
  }

  const raw = (await response.json()) as RawBillingAccount
  return {
    orgId: raw.org_id,
    plan: raw.plan || "free",
    subscriptionStatus: raw.subscription_state || "active",
    entitlements: raw.entitlements ?? {},
    quotaLimits: raw.quota_limits ?? {},
    credits: typeof raw.credits === "number" ? raw.credits : 0,
  }
}

/**
 * Server-side single-feature entitlement check. billing-core returns HTTP 402
 * when the feature is not allowed for the org's plan. Fails closed (false).
 */
export async function checkEntitlement(
  actor: RequestActor,
  orgId: string,
  feature: string,
): Promise<boolean> {
  const response = await fetch(
    `${getBillingCoreUrl()}/api/v1/billing/orgs/${orgId}/entitlements/${feature}`,
    {
      headers: buildHeaders(actor),
      cache: "no-store",
      signal: AbortSignal.timeout(5000),
    },
  ).catch(() => null)

  if (!response || response.status === 402 || !response.ok) return false

  const data = (await response.json().catch(() => null)) as
    | { allowed?: boolean }
    | null
  return Boolean(data?.allowed)
}
