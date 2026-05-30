import { NextRequest, NextResponse } from 'next/server'
import {
  authErrorResponse,
  buildControlPlaneHeaders,
  getSessionServiceUrl,
  getUserServiceUrl,
  isControlSessionAuthorityEnabled,
  readJsonOrNull,
  requireSession,
} from '../../../_lib/control-plane-auth'

// G10 Step 6: When `CONTROL_SESSION_AUTHORITY_ENABLED=true`, this route
// forwards to the repurposed CP session-core's Control Session aggregator at
// `GET /api/v1/sessions/current` (richer payload — user + org + entitlements
// + billing). When the flag is off (default), it falls back to user-core's
// narrower `/api/v1/me/session-context` to preserve the legacy contract
// during the transition.
//
// Without this dedicated file, the `/api/user/[...path]` catch-all forwards
// everything to user-core; Next.js picks specific paths before catch-alls,
// so this override takes precedence for `/api/user/me/session-context`.

const UPSTREAM_TIMEOUT_MS = 8_000

export async function GET(request: NextRequest) {
  try {
    const session = await requireSession(request)
    const headers = buildControlPlaneHeaders(request, session)

    const upstreamUrl = isControlSessionAuthorityEnabled()
      ? `${getSessionServiceUrl()}/api/v1/sessions/current`
      : `${getUserServiceUrl()}/api/v1/me/session-context`

    let response: Response
    try {
      response = await fetch(upstreamUrl, {
        method: 'GET',
        headers,
        cache: 'no-store',
        signal: AbortSignal.timeout(UPSTREAM_TIMEOUT_MS),
      })
    } catch (err) {
      // Network error / abort / timeout. Return 503 so chat's resolver
      // falls through to the Convex mirror instead of hanging the UI.
      const reason = err instanceof Error ? err.name : 'fetch failed'
      return NextResponse.json(
        { error: 'session_context_unavailable', reason },
        { status: 503 },
      )
    }

    const payload = await readJsonOrNull(response)
    return NextResponse.json(payload ?? {}, { status: response.status })
  } catch (error) {
    return authErrorResponse(error)
  }
}
