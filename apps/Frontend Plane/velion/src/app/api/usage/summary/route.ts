/**
 * Phase A · A3 — /api/usage/summary proxy → audit-core
 * (`GET /v1/usage/summary?org_id=...&since=...&until=...`).
 *
 * Powers the velion `/settings/usage` dashboard. Returns one row per
 * (plane, op) for the supplied window with totals across the window.
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

    const url = new URL(`${AUDIT_CORE_URL}/v1/usage/summary`)
    url.searchParams.set('org_id', orgId)
    for (const key of ['since', 'until'] as const) {
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
