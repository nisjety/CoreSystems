import "server-only"
import type { RequestActor } from "@/lib/integrations/request-actor"

/**
 * Server-side audit-core client. audit-core exposes a read API at
 * `/v1/audit`, `/v1/usage`, `/v1/usage/summary`, org-scoped by `org_id` query
 * param, authenticated by the internal service key + X-User-Id. Mirrors the
 * user-core / billing-core client conventions.
 */
export class AuditCoreError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message)
  }
}

function getAuditCoreUrl() {
  return (
    process.env.AUDIT_SERVICE_URL ??
    process.env.AUDIT_CORE_URL ??
    "http://localhost:8187"
  ).replace(/\/+$/, "")
}

function getInternalApiKey() {
  return process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET
}

function buildHeaders(actor: RequestActor) {
  const internalApiKey = getInternalApiKey()
  if (!internalApiKey) {
    throw new AuditCoreError(
      503,
      "audit_core_key_not_configured",
      "INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET is required for audit-core.",
    )
  }
  const headers = new Headers({
    "X-Internal-Api-Key": internalApiKey,
    "X-User-Id": actor.userId,
  })
  if (actor.email) headers.set("X-User-Email", actor.email)
  if (actor.cookieHeader) headers.set("Cookie", actor.cookieHeader)
  return headers
}

export type AuditEvent = {
  id?: string
  action?: string
  actor?: string
  outcome?: string
  resource?: string
  ipAddress?: string
  requestId?: string
  createdAt?: string
}

type RawAuditEvent = {
  id?: string
  action?: string
  event?: string
  actor?: string
  actor_id?: string
  user_id?: string
  outcome?: string
  status?: string
  resource?: string
  target?: string
  ip_address?: string
  request_id?: string
  created_at?: string
  timestamp?: string
}

function normalizeEvent(raw: RawAuditEvent): AuditEvent {
  return {
    id: raw.id,
    action: raw.action ?? raw.event,
    actor: raw.actor ?? raw.actor_id ?? raw.user_id,
    outcome: raw.outcome ?? raw.status,
    resource: raw.resource ?? raw.target,
    ipAddress: raw.ip_address,
    requestId: raw.request_id,
    createdAt: raw.created_at ?? raw.timestamp,
  }
}

/** Fetch recent audit events for an org. Returns [] on a tolerable outage. */
export async function fetchAuditEvents(
  actor: RequestActor,
  orgId: string,
  opts?: { limit?: number },
): Promise<AuditEvent[]> {
  const limit = opts?.limit ?? 25
  const params = new URLSearchParams({ org_id: orgId, limit: String(limit) })

  const response = await fetch(`${getAuditCoreUrl()}/v1/audit?${params}`, {
    headers: buildHeaders(actor),
    cache: "no-store",
    signal: AbortSignal.timeout(5000),
  }).catch((error: unknown) => {
    throw new AuditCoreError(
      502,
      "audit_core_unreachable",
      error instanceof Error ? error.message : "audit-core request failed",
    )
  })

  if (response.status === 404) return []
  if (!response.ok) {
    throw new AuditCoreError(
      response.status,
      "audit_core_error",
      `audit-core returned ${response.status}`,
    )
  }

  const data = (await response.json().catch(() => null)) as
    | { events?: RawAuditEvent[] }
    | RawAuditEvent[]
    | null
  const rawEvents = Array.isArray(data) ? data : (data?.events ?? [])
  return rawEvents.map(normalizeEvent)
}

export type AuditEmit = {
  orgId: string
  event: string
  plane?: string
  subject?: string
  resourceId?: string
  outcome?: "ok" | "denied" | "error"
  details?: Record<string, unknown>
}

/**
 * Fire-and-forget audit write to audit-core's HTTP ingest (POST /v1/audit).
 * NEVER throws — auditing must not break the calling request. No-op without an
 * orgId or internal key.
 */
export async function emitAuditEvent(
  actor: RequestActor,
  ev: AuditEmit,
): Promise<void> {
  if (!ev.orgId) return
  try {
    const headers = buildHeaders(actor)
    headers.set("Content-Type", "application/json")
    await fetch(`${getAuditCoreUrl()}/v1/audit`, {
      method: "POST",
      headers,
      cache: "no-store",
      signal: AbortSignal.timeout(3000),
      body: JSON.stringify({
        org_id: ev.orgId,
        user_id: actor.userId,
        plane: ev.plane ?? "control",
        event: ev.event,
        subject: ev.subject,
        resource_id: ev.resourceId,
        outcome: ev.outcome ?? "ok",
        details: ev.details,
      }),
    })
  } catch {
    // best-effort — swallow
  }
}
