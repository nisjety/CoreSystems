import { NextRequest, NextResponse } from 'next/server'

import { resolveChatActor } from '@/app/api/chat/_lib/session-store'

const ORCHESTRATOR_URL =
  process.env.DATA_ORCHESTRATOR_URL ||
  process.env.DATA_ORCHESTRATOR_GO_URL ||
  'http://data-orchestrator-go:3030'

const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY ||
  process.env.INTERNAL_SERVICE_SECRET ||
  ''

/**
 * Wave 11 §2 — "Retrain agents" CTA in the sticky right rail.
 *
 * Kicks `POST /v1/reindex/jobs` on data-orchestrator-go (canonical
 * owner per `apps/Data Plane/docs/gap-data.md` §9). The orchestrator
 * walks every doc with `is_dirty=true` for the org, re-chunks/embeds
 * through index-engine-rs and embedding-engine-rs, and clears the
 * `dirty` flag. The UI right-rail Convex subscription picks up the
 * cleared state without us needing to poll.
 */
export async function POST(_request: NextRequest) {
  try {
    const actor = await resolveChatActor()

    const response = await fetch(`${ORCHESTRATOR_URL}/v1/reindex/jobs`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Internal-Api-Key': INTERNAL_API_KEY,
        'X-Org-ID': actor.orgId,
        'X-User-Id': actor.userId,
      },
      body: JSON.stringify({
        org_id: actor.orgId,
        scope: { dirty_only: true },
      }),
      signal: AbortSignal.timeout(30_000),
    })

    if (!response.ok) {
      const payload = (await response.json().catch(() => null)) as
        | { error?: string; detail?: string }
        | null
      const message =
        payload?.error ??
        payload?.detail ??
        `Orchestrator returned ${response.status}`
      return NextResponse.json({ error: message }, { status: response.status })
    }

    const data = await response.json()
    return NextResponse.json({ jobId: data.id ?? data.job_id ?? null, status: 'queued' }, { status: 202 })
  } catch (err) {
    return NextResponse.json(
      { error: err instanceof Error ? err.message : 'Retrain failed' },
      { status: 503 },
    )
  }
}
