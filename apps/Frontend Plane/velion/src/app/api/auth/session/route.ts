import { NextRequest, NextResponse } from 'next/server'
import { authErrorResponse, getCurrentSession } from '../../_lib/control-plane-auth'

/**
 * G2 consolidated: thin wrapper over the shared `getCurrentSession` helper.
 * Mirrors `/api/auth/get-session/route.ts` for backwards-compat with code
 * that calls either path. Both consult the same WeakMap-cached session helper
 * and emit the canonical shape: `{ user, session, authenticated }` on success,
 * `{ authenticated: false }` on no session, and a structured error on outage.
 */
export async function GET(request: NextRequest) {
  try {
    const session = await getCurrentSession(request)
    if (!session) {
      return NextResponse.json({ authenticated: false }, { status: 200 })
    }
    return NextResponse.json(session, { status: 200 })
  } catch (error) {
    return authErrorResponse(error)
  }
}

export const POST = GET
export const OPTIONS = GET
