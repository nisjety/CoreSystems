import { NextRequest, NextResponse } from 'next/server'
import { z } from 'zod'

import { resolveChatActor, ChatStoreError } from '@/app/api/chat/_lib/session-store'
import { ensureWikiEnabled } from '@/lib/knowledge/feature-flags'
import {
  createPage,
  getWikiPageByPath,
} from '@/lib/knowledge/wiki-client'

const createSchema = z.object({
  title: z.string().trim().min(1).max(200),
  path: z
    .string()
    .trim()
    .min(1)
    .max(500)
    .refine((p) => p.startsWith('/'), 'path must start with "/"'),
  initial_content: z.string().max(64_000).default(''),
  workspace_id: z.string().optional(),
})

/**
 * Wave 11.C-a — wiki list / create.
 *
 * wiki-store-go does not expose a "list all pages" endpoint, so GET
 * here serves a hint payload. The page sidebar in the UI uses
 * operator-bookmarked paths from localStorage + this endpoint's
 * `lookup_by_path` capability to hydrate them.
 *
 * POST creates a new wiki page (real call to wiki-store-go).
 */
export async function GET(request: NextRequest) {
  const gate = ensureWikiEnabled()
  if (gate !== true) {
    return NextResponse.json({ error: gate.error }, { status: gate.status })
  }

  try {
    const actor = await resolveChatActor()
    const sp = request.nextUrl.searchParams
    const path = sp.get('path')

    // Path-based lookup mode: resolves a known path to its page record.
    if (path) {
      const page = await getWikiPageByPath(path, { orgId: actor.orgId, userId: actor.userId })
      if (!page) {
        return NextResponse.json({ error: 'Page not found', path }, { status: 404 })
      }
      return NextResponse.json({ page }, { headers: { 'Cache-Control': 'no-store' } })
    }

    // List mode: no list endpoint on the service yet.
    return NextResponse.json(
      {
        pages: [],
        warning:
          'wiki-store-go has no list-all endpoint yet. The sidebar uses bookmarked paths from your browser session; use the Open-by-path input to hydrate one.',
      },
      { headers: { 'Cache-Control': 'no-store' } },
    )
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }
    return NextResponse.json(
      { error: error instanceof Error ? error.message : 'Wiki list failed' },
      { status: 500 },
    )
  }
}

export async function POST(request: NextRequest) {
  const gate = ensureWikiEnabled()
  if (gate !== true) {
    return NextResponse.json({ error: gate.error }, { status: gate.status })
  }

  try {
    const actor = await resolveChatActor()
    const body = await request.json()
    const parsed = createSchema.parse(body)
    const result = await createPage(parsed, { orgId: actor.orgId, userId: actor.userId })
    if (!result.ok) {
      return NextResponse.json({ error: result.error }, { status: result.status })
    }
    return NextResponse.json(result.data, { status: 201 })
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
      { error: error instanceof Error ? error.message : 'Create failed' },
      { status: 500 },
    )
  }
}
