/**
 * Phase A · A3 — /api/audit/log proxy → audit-core (Control Plane).
 *
 * Reads the verified Better Auth session, scopes the query to the
 * caller's active org_id (resolved via the existing control-plane-auth
 * helper), and forwards to `audit-core` (`/v1/audit`) on the
 * inter-plane-bus.
 *
 * Once Wave 3 enforce mode lands the upstream call will switch to an
 * audit-core-scoped JWT minted via `mintPlaneToken({ audience:
 * 'control-plane', request })`; for now it forwards the
 * `INTERNAL_API_KEY` header that audit-core's `internalAuth` middleware
 * accepts.
 */

import { NextRequest, NextResponse } from 'next/server'
import {
  authErrorResponse,
  buildControlPlaneHeaders,
  getUserServiceUrl,
  requireSession,
} from '../../_lib/control-plane-auth'

const AUDIT_CORE_URL = (
  process.env.AUDIT_CORE_URL || 'http://audit-core-service:8187'
).replace(/\/+$/, '')

const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY ||
  process.env.INTERNAL_SERVICE_SECRET ||
  ''

export async function GET(request: NextRequest): Promise<NextResponse> {
  try {
    const session = await requireSession(request)
    const orgId = await resolveOrgId(request, session)
    if (!orgId) {
      return NextResponse.json(
        { error: 'no active organisation on session' },
        { status: 400 },
      )
    }

    const url = new URL(`${AUDIT_CORE_URL}/v1/audit`)
    url.searchParams.set('org_id', orgId)
    for (const key of ['since', 'until', 'event', 'user_id', 'limit'] as const) {
      const v = request.nextUrl.searchParams.get(key)
      if (v) url.searchParams.set(key, v)
    }

    const response = await fetch(url, {
      method: 'GET',
      headers: {
        accept: 'application/json',
        ...(INTERNAL_API_KEY
          ? { 'x-internal-api-key': INTERNAL_API_KEY }
          : {}),
      },
      cache: 'no-store',
    })

    const payload = await response.json().catch(() => null)
    return NextResponse.json(payload ?? {}, { status: response.status })
  } catch (error) {
    return authErrorResponse(error)
  }
}

/**
 * Resolve the caller's active org by asking user-core for the session
 * context. Cached for the request via `buildControlPlaneHeaders`
 * (which forwards the existing cookie + correlation id). Once Wave 3
 * enforce mode lands, the org_id will come from the verified
 * audit-core-scoped JWT instead of a second user-core round-trip.
 */
async function resolveOrgId(
  request: NextRequest,
  session: Awaited<ReturnType<typeof requireSession>>,
): Promise<string | null> {
  const res = await fetch(
    `${getUserServiceUrl()}/api/v1/me/session-context`,
    {
      method: 'GET',
      headers: buildControlPlaneHeaders(request, session),
      cache: 'no-store',
    },
  )
  if (!res.ok) return null
  const body = (await res.json().catch(() => null)) as
    | { orgId?: string; activeOrgId?: string }
    | null
  return body?.orgId ?? body?.activeOrgId ?? null
}
