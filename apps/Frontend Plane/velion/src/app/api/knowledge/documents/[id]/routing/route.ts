import { NextRequest, NextResponse } from 'next/server'
import { z } from 'zod'

import { resolveChatActor, ChatStoreError } from '@/app/api/chat/_lib/session-store'
import { ROUTER_ROUTES } from '@/lib/knowledge/router-schema'

const overrideSchema = z.object({
  route: z.enum(ROUTER_ROUTES),
})

interface RouteContext {
  params: Promise<{ id: string }>
}

const DOCUMENTS_SERVICE_URL =
  process.env.DOCUMENTS_SERVICE_URL ||
  process.env.DOCS_SERVICE_URL ||
  'http://documents-service:8001'

const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY ||
  process.env.INTERNAL_SERVICE_SECRET ||
  ''

/**
 * Wave 11 §7 — manual routing override.
 *
 * Operator changes a doc's route from the RoutingMatrix UI dropdown.
 * Persists `routed_by='manual'` so the auto-classifier won't clobber
 * it on the next sweep (unless `force=true` is passed).
 */
export async function PATCH(request: NextRequest, context: RouteContext) {
  try {
    const { id } = await context.params
    const actor = await resolveChatActor()
    const body = await request.json()
    const parsed = overrideSchema.parse(body)

    const response = await fetch(
      `${DOCUMENTS_SERVICE_URL}/v1/documents/${encodeURIComponent(id)}/routing`,
      {
        method: 'PATCH',
        headers: {
          'Content-Type': 'application/json',
          'X-Internal-Api-Key': INTERNAL_API_KEY,
          'X-Org-ID': actor.orgId,
        },
        body: JSON.stringify({
          org_id: actor.orgId,
          route: parsed.route,
          routed_by: 'manual',
          routed_at: Date.now(),
        }),
      },
    )

    if (!response.ok) {
      const detail = await response.text().catch(() => '')
      throw new Error(detail || `Routing update failed: ${response.status}`)
    }

    return NextResponse.json({ success: true, route: parsed.route })
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }
    if (error && typeof error === 'object' && 'issues' in error) {
      const issues = (error as { issues: Array<{ message?: string }> }).issues
      return NextResponse.json(
        { error: issues[0]?.message ?? 'Validation failed' },
        { status: 400 },
      )
    }
    return NextResponse.json(
      { error: error instanceof Error ? error.message : 'Update failed' },
      { status: 500 },
    )
  }
}
