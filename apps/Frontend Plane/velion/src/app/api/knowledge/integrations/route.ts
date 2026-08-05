import { NextResponse } from 'next/server'

import { ChatStoreError, getKnowledgeIntegrations } from '../_lib/knowledge-data'

/**
 * Empty payload shape returned when downstream services (integration-engine,
 * Convex mirror, user-core) are unreachable in dev. The Knowledge tab in the
 * UI keys off `providers.length` for the empty-state; an empty list is the
 * honest signal for "no integrations connected" and keeps the tab usable
 * while a backend service is offline. Auth failures still propagate as 401
 * so the UI redirects to login — only soft 5xx degrades.
 */
const EMPTY_INTEGRATIONS_PAYLOAD = {
  orgId: '',
  userId: '',
  totalConnected: 0,
  connections: [],
  providerLinks: [],
  providers: [],
} as const

export async function GET() {
  try {
    const payload = await getKnowledgeIntegrations()
    return NextResponse.json(payload, {
      headers: { 'Cache-Control': 'no-store' },
    })
  } catch (error) {
    if (error instanceof ChatStoreError) {
      // 401 (no session) → propagate so the UI can redirect to /login.
      // 403 (no org) and 5xx (Convex / user-core / integration-engine
      // unreachable) → degrade to empty payload so the Knowledge tab
      // renders its empty-state instead of a hard error. The reason
      // surfaces in `x-verevon-integration-warning` for observability.
      if (error.statusCode === 401) {
        return NextResponse.json({ error: error.message }, { status: 401 })
      }
      return NextResponse.json(EMPTY_INTEGRATIONS_PAYLOAD, {
        headers: {
          'Cache-Control': 'no-store',
          'x-verevon-integration-warning': error.message.slice(0, 200),
        },
      })
    }

    // Unknown error — same graceful degrade so the UI stays usable.
    return NextResponse.json(EMPTY_INTEGRATIONS_PAYLOAD, {
      headers: {
        'Cache-Control': 'no-store',
        'x-verevon-integration-warning':
          error instanceof Error ? error.message.slice(0, 200) : 'unknown error',
      },
    })
  }
}
