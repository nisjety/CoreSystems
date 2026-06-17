import { NextRequest, NextResponse } from 'next/server'
import {
  authErrorResponse,
  buildControlPlaneHeaders,
  getUserServiceUrl,
  readJsonOrNull,
  requireSession,
} from '../../../_lib/control-plane-auth'

// G3 + G16: server-side onboarding state.
//
// GET  /api/user/me/onboarding-state  → { step, state } from user-core
// PUT  /api/user/me/onboarding-state  body { step, state? } → user-core
//
// Without this dedicated file, `/api/user/[...path]` catch-all would forward
// `me/onboarding-state` to `${USER_SERVICE_URL}/api/v1/me/onboarding-state`
// (no `users/` prefix). user-core registers the route under `/users/me/...`,
// not `/me/...`. The catch-all's `me/session-context` ↔ user-core `/me/...`
// mapping works because user-core registers BOTH paths for that one route;
// onboarding-state lives only under `/users/me/onboarding-state`. Hence
// this dedicated forwarder.

async function forward(
  request: NextRequest,
  method: 'GET' | 'PUT',
  body?: unknown,
): Promise<NextResponse> {
  try {
    const session = await requireSession(request)
    const response = await fetch(
      `${getUserServiceUrl()}/api/v1/users/me/onboarding-state`,
      {
        method,
        headers: buildControlPlaneHeaders(request, session),
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
        cache: 'no-store',
      },
    )
    const payload = await readJsonOrNull(response)
    return NextResponse.json(payload ?? {}, { status: response.status })
  } catch (error) {
    return authErrorResponse(error)
  }
}

export async function GET(request: NextRequest): Promise<NextResponse> {
  return forward(request, 'GET')
}

export async function PUT(request: NextRequest): Promise<NextResponse> {
  let body: unknown = null
  try {
    body = await request.json()
  } catch {
    return NextResponse.json({ error: 'invalid json body' }, { status: 400 })
  }
  return forward(request, 'PUT', body)
}
