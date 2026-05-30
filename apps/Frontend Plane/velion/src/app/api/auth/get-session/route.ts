import { NextRequest, NextResponse } from 'next/server'
import { authErrorResponse, getCurrentSession } from '../../_lib/control-plane-auth'

/**
 * G2 consolidated: thin wrapper over the shared `getCurrentSession` helper.
 * Returns `{ user, session, authenticated }` on success, `{ authenticated: false }`
 * when no valid session, and a structured error on auth-service outages.
 *
 * Kept alongside `/api/auth/session/route.ts` (sibling) for backwards-compat
 * with existing client code; both routes now share the same implementation.
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
