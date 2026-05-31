import "server-only"
import type { RequestActor } from "@/lib/integrations/request-actor"

/**
 * session-core Control Session client (ADR 0002 / G10). When
 * CONTROL_SESSION_AUTHORITY_ENABLED=true, session-core aggregates identity +
 * org + entitlements + billing in a single cached call, replacing the separate
 * user-core + billing-core fetches in the context composer.
 *
 * GET  /api/v1/sessions/current  -> aggregated snapshot (Redis read-through, 30s TTL)
 * POST /api/v1/sessions/refresh  -> bust cache + re-aggregate (call after plan/org changes)
 * Auth: X-Internal-Api-Key + X-User-Id.
 */
export class SessionCoreError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message)
  }
}

export function isControlSessionAuthorityEnabled(): boolean {
  return (process.env.CONTROL_SESSION_AUTHORITY_ENABLED || "").toLowerCase() === "true"
}

function getSessionCoreUrl() {
  return (
    process.env.SESSION_CORE_URL ??
    process.env.SESSION_SERVICE_URL ??
    "http://localhost:3017"
  ).replace(/\/+$/, "")
}

function getInternalApiKey() {
  return process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET
}

function buildHeaders(actor: RequestActor) {
  const internalApiKey = getInternalApiKey()
  if (!internalApiKey) {
    throw new SessionCoreError(
      503,
      "session_core_key_not_configured",
      "INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET is required for session-core.",
    )
  }
  const headers = new Headers({
    "X-Internal-Api-Key": internalApiKey,
    "X-User-Id": actor.userId,
    "Content-Type": "application/json",
  })
  if (actor.email) headers.set("X-User-Email", actor.email)
  if (actor.name) headers.set("X-User-Name", actor.name)
  return headers
}

export type ControlSession = {
  user: {
    id: string
    email?: string
    name?: string
    image?: string
    onboardingComplete?: boolean
  }
  organization?: {
    id: string
    name?: string
    plan?: string
    tenantId?: string
    role?: string
  } | null
  entitlements?: unknown[]
  billing?: {
    orgId?: string
    plan?: string
    status?: string
    subscriptionId?: string
    currentPeriodEnd?: string
  } | null
  onboardingStatus?: string
  fetchedAt?: string
}

async function call(
  actor: RequestActor,
  path: string,
  method: "GET" | "POST",
): Promise<ControlSession | null> {
  const response = await fetch(`${getSessionCoreUrl()}${path}`, {
    method,
    headers: buildHeaders(actor),
    cache: "no-store",
    signal: AbortSignal.timeout(5000),
  }).catch((error: unknown) => {
    throw new SessionCoreError(
      502,
      "session_core_unreachable",
      error instanceof Error ? error.message : "session-core request failed",
    )
  })

  if (response.status === 401 || response.status === 403 || response.status === 404) {
    return null
  }
  if (!response.ok) {
    throw new SessionCoreError(
      response.status,
      "session_core_error",
      `session-core returned ${response.status}`,
    )
  }
  return (await response.json()) as ControlSession
}

/** Aggregated control session snapshot for the current user. */
export function fetchControlSession(actor: RequestActor): Promise<ControlSession | null> {
  return call(actor, "/api/v1/sessions/current", "GET")
}

/** Force a re-aggregation (bust cache). Call after plan/org/billing changes. */
export function refreshControlSession(actor: RequestActor): Promise<ControlSession | null> {
  return call(actor, "/api/v1/sessions/refresh", "POST")
}

/** Map a session-core entitlements array to a {feature: enabled} record. */
export function entitlementsToFeatures(list: unknown): Record<string, boolean> {
  if (!Array.isArray(list)) return {}
  const out: Record<string, boolean> = {}
  for (const item of list) {
    if (typeof item === "string") {
      out[item] = true
    } else if (item && typeof item === "object") {
      const o = item as Record<string, unknown>
      const key =
        (typeof o.key === "string" && o.key) ||
        (typeof o.name === "string" && o.name) ||
        (typeof o.feature === "string" && o.feature) ||
        undefined
      if (key) out[key] = o.enabled !== false && o.value !== false
    }
  }
  return out
}
