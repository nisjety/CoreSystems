import { NextRequest } from 'next/server'
import { getModelPlaneTokenFromSession } from '@/lib/model-plane/auth-token'

// U2-15 follow-up (verevon ui-ux-verevon-gap.md §10): Realtime voice config.
//
// The chat composer's "voice mode" surface opens a WebSocket directly to the
// model-gateway at `GET /v1/ai/realtime`. This route does NOT proxy the
// upgrade itself (Next.js App Router can't currently proxy WebSocket); it
// hands the browser a connection descriptor so the client can connect to the
// gateway with the appropriate subprotocol-encoded bearer token.
//
// Why subprotocol auth? Browsers cannot set custom headers on the standard
// WebSocket constructor. The accepted workaround is to encode the bearer in
// the `Sec-WebSocket-Protocol` list as `bearer.<token>`. The model-gateway's
// `auth::require_auth` middleware reads it from there as a fallback when
// the `Authorization` header is missing. The handler echoes back the
// `realtime.v1` subprotocol so the browser handshake completes.
//
// Response shape:
//   {
//     "url":         "ws://localhost:18080/v1/ai/realtime",
//     "protocols":   ["realtime.v1", "bearer.<token>"],
//     "expires_in":  3600
//   }
//
// In dev (MODEL_GATEWAY_AUTH_DEV_BYPASS=1) the bearer can be any non-empty
// string. In prod this should be a short-lived JWT minted via auth-core.

const MODEL_GATEWAY_WS_URL =
  process.env.NEXT_PUBLIC_MODEL_GATEWAY_WS_URL ??
  process.env.MODEL_GATEWAY_WS_URL ??
  'ws://localhost:18080'

interface RealtimeConfig {
  url: string
  protocols: string[]
  expires_in: number
}

export async function GET(request: NextRequest): Promise<Response> {
  // U2-5: mint a real Model Plane JWT from the active session. The token
  // travels back to the browser via `Sec-WebSocket-Protocol: bearer.<jwt>`
  // because the standard WebSocket constructor can't set custom headers.
  // The gateway's `auth::require_auth` middleware reads it from there.
  //
  // Important: the token surfaces to the browser. That's acceptable
  // because (a) it's short-lived (default 15 min via
  // MODEL_PLANE_AUTH_TOKEN_TTL_SECONDS), (b) it's scoped to this user +
  // org via the embedded `user_id` + `org_id` claims, and (c) the
  // Model Plane gateway is the only audience that accepts it.
  let token: string
  try {
    token = await getModelPlaneTokenFromSession(request)
  } catch (error: unknown) {
    return Response.json(
      {
        error: 'Realtime session unavailable',
        message:
          error instanceof Error ? error.message : 'token mint failed',
      },
      { status: 401 },
    )
  }

  const config: RealtimeConfig = {
    url: `${MODEL_GATEWAY_WS_URL.replace(/\/$/, '')}/v1/ai/realtime`,
    // Order matters: `realtime.v1` first so the gateway echoes the wire
    // protocol; `bearer.<token>` is consumed by auth and not echoed.
    protocols: ['realtime.v1', `bearer.${token}`],
    expires_in: 900,
  }
  return Response.json(config, {
    headers: { 'Cache-Control': 'no-store, max-age=0' },
  })
}
