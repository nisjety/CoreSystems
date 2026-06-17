import { NextRequest, NextResponse } from 'next/server'
import { z } from 'zod'

const QUARRY_URL = process.env.QUARRY_API_URL || 'http://quarry-control:8081'

const commitSchema = z.object({
  urls: z.array(z.string().url()).min(1).max(500),
})

interface RouteContext {
  params: Promise<{ jobId: string }>
}

/**
 * Wave 11 §3 — commit a curated subset from a discover job. The
 * operator picked which pages to ingest; we forward that list as a new
 * `crawl_commit` job that references the parent discover job by id.
 *
 * Quarry handles fetch + parse; documents-service handles persistence
 * and embedding (via NATS bridge). No data ever loops back through
 * this route — we just kick the job.
 */
export async function POST(request: NextRequest, context: RouteContext) {
  try {
    const { jobId } = await context.params
    const body = await request.json()
    const parsed = commitSchema.parse(body)

    const response = await fetch(`${QUARRY_URL}/v1/jobs/`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Accept: 'application/json',
      },
      body: JSON.stringify({
        kind: 'crawl_commit',
        params: {
          parent_job_id: jobId,
          urls: parsed.urls,
        },
      }),
      signal: AbortSignal.timeout(60_000),
    })

    if (!response.ok) {
      const payload = (await response.json().catch(() => null)) as
        | { error?: { message?: string } }
        | null
      const message = payload?.error?.message || `Quarry returned ${response.status}`
      return NextResponse.json({ error: message }, { status: response.status })
    }

    const envelope = (await response.json()) as {
      data?: { id?: string; status?: string }
    }

    return NextResponse.json(
      {
        commitJobId: envelope.data?.id ?? null,
        status: envelope.data?.status ?? 'queued',
        pageCount: parsed.urls.length,
      },
      { status: 202 },
    )
  } catch (err) {
    if (err && typeof err === 'object' && 'issues' in err) {
      const issues = (err as { issues: Array<{ message?: string }> }).issues
      return NextResponse.json(
        { error: issues[0]?.message ?? 'Invalid commit payload' },
        { status: 400 },
      )
    }
    return NextResponse.json(
      { error: err instanceof Error ? err.message : 'Commit failed' },
      { status: 503 },
    )
  }
}
