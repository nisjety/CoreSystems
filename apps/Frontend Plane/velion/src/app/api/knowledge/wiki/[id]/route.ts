import { NextRequest, NextResponse } from 'next/server'
import { z } from 'zod'

import { resolveChatActor, ChatStoreError } from '@/app/api/chat/_lib/session-store'
import { ensureWikiEnabled } from '@/lib/knowledge/feature-flags'
import {
  getWikiPage,
  getCurrentVersion,
  getBacklinks,
  listSourceLogs,
  updateVersion,
} from '@/lib/knowledge/wiki-client'

const patchSchema = z.object({
  content: z.string().max(64_000),
  edit_reason: z.string().trim().min(1).max(120).default('manual_edit'),
})

interface RouteContext {
  params: Promise<{ id: string }>
}

/**
 * Wave 11.C-a — single wiki page detail.
 *
 * Composes 4 wiki-store-go calls in parallel:
 *   - page          → GET /v1/wiki/pages/{id}
 *   - version       → same endpoint (returns page+version together)
 *   - backlinks     → GET /v1/wiki/pages/{id}/backlinks
 *   - source-logs   → GET /v1/wiki/pages/{id}/source-logs (latest)
 *
 * Empty backlinks / source_log are valid — the editor renders them
 * as empty sections rather than failing the load.
 */
export async function GET(_request: NextRequest, context: RouteContext) {
  const gate = ensureWikiEnabled()
  if (gate !== true) {
    return NextResponse.json({ error: gate.error }, { status: gate.status })
  }
  try {
    const { id } = await context.params
    const actor = await resolveChatActor()
    const base = { orgId: actor.orgId, userId: actor.userId }

    const [page, version, backlinks, sourceLog] = await Promise.all([
      getWikiPage(id, base),
      getCurrentVersion(id, base),
      getBacklinks(id, base),
      listSourceLogs(id, base),
    ])

    if (!page) {
      return NextResponse.json({ error: 'Page not found' }, { status: 404 })
    }

    return NextResponse.json(
      {
        page,
        version,
        backlinks_resolved: backlinks,
        source_log: sourceLog,
      },
      { headers: { 'Cache-Control': 'no-store' } },
    )
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }
    return NextResponse.json(
      { error: error instanceof Error ? error.message : 'Wiki detail load failed' },
      { status: 500 },
    )
  }
}

export async function PATCH(request: NextRequest, context: RouteContext) {
  const gate = ensureWikiEnabled()
  if (gate !== true) {
    return NextResponse.json({ error: gate.error }, { status: gate.status })
  }
  try {
    const { id } = await context.params
    const actor = await resolveChatActor()
    const body = await request.json()
    const parsed = patchSchema.parse(body)

    const result = await updateVersion(
      id,
      { content: parsed.content, edit_reason: parsed.edit_reason },
      { orgId: actor.orgId, userId: actor.userId },
    )
    if (!result.ok) {
      return NextResponse.json({ error: result.error }, { status: result.status })
    }
    return NextResponse.json(result.data, { status: 200 })
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
