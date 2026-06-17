import { NextRequest, NextResponse } from 'next/server'

// G32: target Quarry-v2 control service. The v1 endpoint POST /v1/crawl on
// `quarry-api:8090` no longer exists — v2 exposes POST /v1/jobs/ on
// quarry-control:8081 with a typed `kind` envelope.
const QUARRY_URL = process.env.QUARRY_API_URL || 'http://quarry-control:8081'

type QuarryJob = {
  id: string
  kind: string
  status: string
  params?: Record<string, unknown>
  result?: { pages?: Array<{ url: string; title?: string; content_type?: string }> }
  created_at?: number
}

type QuarryEnvelope<T> = {
  data?: T | null
  meta?: Record<string, unknown> | null
  error?: { code?: string; message?: string } | null
}

/**
 * Wave 11 §3: two-phase crawl.
 *
 *   POST /api/ingestion/crawl
 *     → standard crawl (existing behavior).
 *
 *   POST /api/ingestion/crawl?phase=discover
 *     → kicks Quarry's `crawl_discover` job which sitemaps/spiders the
 *       URL without fetching content. Returns a list of candidate page
 *       URLs the operator can preview before committing.
 *
 *   POST /api/ingestion/crawl?phase=discover&autoCommit=true
 *     → Phase 4 onboarding helper. Discovers + auto-commits in one
 *       call (no operator preview), capped by `maxPages`.
 */
export async function POST(request: NextRequest) {
  try {
    const body = await request.json()

    if (!body.url || typeof body.url !== 'string') {
      return NextResponse.json(
        { error: 'Missing or invalid url parameter' },
        { status: 400 },
      )
    }

    const phase = request.nextUrl.searchParams.get('phase')
    const autoCommit = request.nextUrl.searchParams.get('autoCommit') === 'true'

    const params: Record<string, unknown> = { url: body.url }
    if (typeof body.maxPages === 'number' && body.maxPages > 0) {
      params.max_pages = Math.min(body.maxPages, 500)
    }
    if (typeof body.maxDepth === 'number' && body.maxDepth > 0) {
      params.max_depth = body.maxDepth
    }
    if (phase === 'discover') {
      params.auto_commit = autoCommit
    }

    const kind = phase === 'discover' ? 'crawl_discover' : 'crawl'

    const response = await fetch(`${QUARRY_URL}/v1/jobs/`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Accept: 'application/json',
      },
      body: JSON.stringify({ kind, params }),
      signal: AbortSignal.timeout(60_000),
    })

    const envelope = (await response.json().catch(() => null)) as QuarryEnvelope<QuarryJob> | null

    if (!response.ok) {
      const message = envelope?.error?.message || `Quarry returned ${response.status}`
      return NextResponse.json({ error: message }, { status: response.status })
    }

    const job = envelope?.data ?? null

    // Discover phase: shape the response so the CrawlSourceModal can
    // render its preview checklist directly. The job-result `pages`
    // array is the source of truth; we normalize to `{url, title}`.
    if (phase === 'discover') {
      const pages = Array.isArray(job?.result?.pages)
        ? job!.result!.pages!.map((page) => ({
            url: page.url,
            title: page.title ?? undefined,
            contentType: page.content_type ?? undefined,
          }))
        : []
      return NextResponse.json({
        jobId: job?.id ?? null,
        pages,
        totalDiscovered: pages.length,
      })
    }

    return NextResponse.json({
      job,
      jobId: job?.id ?? null,
    })
  } catch (err) {
    return NextResponse.json(
      { error: err instanceof Error ? err.message : 'Quarry not available or timed out' },
      { status: 503 },
    )
  }
}
