import { NextRequest, NextResponse } from 'next/server'
import { resolveActiveOrgContext } from '@/lib/server/active-org'

/**
 * Manually trigger ingestion of a completed Quarry crawl job into Data Plane.
 * POST /api/ingestion/ingest-job
 * Body: { crawlJobId: string, sourceUrl: string }
 * orgId is resolved server-side from the authenticated session — never trusted from client.
 *
 * Quarry-v2 content ingestion (G38, 2026-05-12):
 *   1. Resolve the job at `GET ${QUARRY_URL}/v1/jobs/{id}` (quarry-control).
 *   2. Page the event stream at `/v1/jobs/{id}/events` and pick `page_fetched`
 *      events to discover the URLs the crawler successfully reached.
 *   3. For each URL: POST `/v1/scrape` to quarry-edge to get a markdown
 *      `FormatRef`, then GET `/v1/artifacts/{id}/bytes` to materialise the
 *      bytes (added in §8.28). Quarry's fingerprint cache means the
 *      `/v1/scrape` call doesn't re-fetch network if the URL was just
 *      crawled.
 *   4. POST the document to Data Plane with a stable
 *      `X-Idempotency-Key: {org_id}:{crawl_job_id}:{url}` so retries are
 *      safe.
 *   5. Per-URL body failures don't abort the loop — onboarding moves on
 *      with whatever fraction of bodies landed; the rest stay as
 *      URL-placeholder documents (still discoverable in the knowledge
 *      base).
 *
 * Quarry-v2 migration history (G32 + G38 + §8.23):
 *   v1 returned `{ result: { products: [...] } }` inline on `GET /v1/jobs/{id}`.
 *   v2 returns the bare `Job` struct and exposes content out-of-band via
 *   artifacts. §8.23 fixed the route from 400'ing on the missing
 *   `result.products` shape but only wrote URL placeholders to Data Plane.
 *   §8.28 / G38 now writes real page bodies.
 */

// In-cluster Quarry control plane URL — `localhost:8092` was the v1 dev
// default and the wrong service for v2.
const QUARRY_URL = process.env.QUARRY_API_URL || 'http://quarry-control:8081'
// G38: separate edge URL for /v1/scrape + /v1/artifacts/:id/bytes (artifact-
// bytes retrieval). quarry-control owns metadata, quarry-edge owns content.
const QUARRY_EDGE_URL = process.env.QUARRY_EDGE_URL || 'http://quarry-edge:8082'
const DATAPLANE_URL = process.env.DATAPLANE_API_URL || 'http://data-documents-service:8001'
const INTERNAL_API_KEY =
  (process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET) as string
if (!INTERNAL_API_KEY) {
  throw new Error('INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET must be set')
}

interface QuarryJob {
  id: string
  kind: string
  status: string
  params?: Record<string, unknown>
  created_at?: number
}

interface QuarryEvent {
  event_id: string
  type: string
  ts: string
  seq: number
  job_id?: string
  run_id?: string
  payload?: Record<string, unknown>
}

interface PageFetchedPayload {
  url?: string
  status?: number
  fingerprint?: string
  links?: number
}

interface IngestResponse {
  success: boolean
  jobId: string
  jobStatus: string
  pagesFetched: number
  pagesFailed: number
  ingestedCount: number
  failedCount: number
  /** G38: how many of the ingested documents have a non-empty body. */
  bodiesFetched: number
  /** Kept for v1 caller compatibility (`onboarding-service.ts:_ingestCrawlResultsToDataPlane`). */
  totalProducts: number
  errors?: string[]
  message: string
}

interface ScrapedPage {
  body: string
  title: string
}

interface QuarryFormatRef {
  artifact_id: string
  bytes: number
}

interface QuarryScrapeEnvelope {
  data?: {
    url?: { final_url?: string; canonical?: string; requested?: string }
    formats?: { markdown?: QuarryFormatRef; html?: QuarryFormatRef }
    metadata?: { title?: string }
  }
}

async function fetchPageBody(url: string): Promise<ScrapedPage> {
  // (a) POST /v1/scrape — Quarry's fingerprint cache means a freshly-crawled
  // URL responds from cache without re-fetching the network.
  const scrapeRes = await fetch(`${QUARRY_EDGE_URL}/v1/scrape`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ url, formats: ['markdown'] }),
  })
  if (!scrapeRes.ok) {
    throw new Error(`quarry-edge /v1/scrape HTTP ${scrapeRes.status}`)
  }
  const envelope = (await scrapeRes.json()) as QuarryScrapeEnvelope
  const ref = envelope.data?.formats?.markdown
  if (!ref?.artifact_id) {
    throw new Error('no markdown artifact in scrape response')
  }
  // (b) GET /v1/artifacts/:id/bytes — see §8.28 in verevon-gap.md.
  const bytesRes = await fetch(
    `${QUARRY_EDGE_URL}/v1/artifacts/${encodeURIComponent(ref.artifact_id)}/bytes`,
  )
  if (!bytesRes.ok) {
    throw new Error(`quarry-edge /v1/artifacts/${ref.artifact_id}/bytes HTTP ${bytesRes.status}`)
  }
  const body = await bytesRes.text()
  return { body, title: envelope.data?.metadata?.title ?? url }
}

async function fetchQuarryEvents(jobId: string): Promise<QuarryEvent[]> {
  const events: QuarryEvent[] = []
  let afterSeq = 0
  // Page through the event log — `/v1/jobs/{id}/events?after_seq=N&limit=M`.
  // The default cap server-side is 100 per call.
  for (let i = 0; i < 50; i++) {
    const url = `${QUARRY_URL}/v1/jobs/${encodeURIComponent(jobId)}/events?after_seq=${afterSeq}&limit=100`
    const res = await fetch(url, {
      headers: { 'X-API-Key': process.env.QUARRY_API_KEY || 'dev-test-key-12345' },
    })
    if (!res.ok) {
      throw new Error(`quarry events HTTP ${res.status}`)
    }
    const batch = (await res.json()) as QuarryEvent[]
    if (!Array.isArray(batch) || batch.length === 0) {
      break
    }
    events.push(...batch)
    afterSeq = batch[batch.length - 1].seq
    if (batch.length < 100) {
      break
    }
  }
  return events
}

export async function POST(request: NextRequest) {
  try {
    const activeOrg = await resolveActiveOrgContext()
    if (!activeOrg) {
      return NextResponse.json({ error: 'Unauthorized' }, { status: 401 })
    }
    if (!activeOrg.orgId) {
      return NextResponse.json({ error: 'No active organization' }, { status: 403 })
    }

    const body = await request.json()
    const { crawlJobId, sourceUrl } = body

    if (!crawlJobId || !sourceUrl) {
      return NextResponse.json(
        { error: 'Missing required fields: crawlJobId, sourceUrl' },
        { status: 400 }
      )
    }

    // 1. Confirm the job exists in Quarry-v2 (and that we can reach it).
    const jobResponse = await fetch(`${QUARRY_URL}/v1/jobs/${encodeURIComponent(crawlJobId)}`, {
      headers: { 'X-API-Key': process.env.QUARRY_API_KEY || 'dev-test-key-12345' },
    })

    if (!jobResponse.ok) {
      return NextResponse.json(
        { error: `Failed to fetch crawl job from Quarry: ${jobResponse.status}` },
        { status: jobResponse.status }
      )
    }

    const job = (await jobResponse.json()) as QuarryJob

    // 2. Page through the event stream to count fetched/failed pages and
    //    collect URLs for a placeholder ingestion record.
    let events: QuarryEvent[] = []
    try {
      events = await fetchQuarryEvents(crawlJobId)
    } catch (err) {
      console.warn('ingest-job: failed to read events', err)
    }

    const pageFetched = events.filter((e) => e.type === 'page_fetched')
    const pageFailed = events.filter((e) => e.type === 'page_failed' || e.type === 'page_blocked')

    // 3. For each fetched URL: (a) POST /v1/scrape to quarry-edge to get a
    //    `NormalizedOutput` with a markdown FormatRef (Quarry uses its
    //    fingerprint cache, so a recently-crawled URL doesn't refetch
    //    network-side); (b) GET /v1/artifacts/:id/bytes to materialise the
    //    markdown; (c) POST the body to Data Plane with a stable
    //    idempotency key (`{org_id}:{crawl_job_id}:{url}`) so retries are
    //    safe. Per-URL failures don't abort the loop — onboarding moves on
    //    even when some pages fail to enrich.
    let ingestedCount = 0
    let failedCount = 0
    let bodiesFetched = 0
    const errors: string[] = []

    for (const evt of pageFetched) {
      const payload = (evt.payload || {}) as PageFetchedPayload
      const url = payload.url || sourceUrl

      let body = ''
      let title = url
      try {
        const scraped = await fetchPageBody(url)
        body = scraped.body
        title = scraped.title || url
        if (body.length > 0) {
          bodiesFetched++
        }
      } catch (err) {
        // Body retrieval is best-effort. Fall back to the URL-placeholder
        // document so the user still sees the page in their knowledge base.
        errors.push(`${url}: body fetch failed: ${err instanceof Error ? err.message : String(err)}`)
      }

      const idempotencyKey = `${activeOrg.orgId}:${crawlJobId}:${url}`
      const document = {
        org_id: activeOrg.orgId,
        source: `website-crawl:${sourceUrl}`,
        type: 'webpage',
        title,
        content: body,
        url,
        metadata: {
          crawl_job_id: crawlJobId,
          page_status: payload.status,
          fingerprint: payload.fingerprint,
          extracted_at: new Date().toISOString(),
          body_bytes: body.length,
        },
      }

      try {
        const ingestResponse = await fetch(`${DATAPLANE_URL}/internal/v1/documents`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'X-Internal-Api-Key': INTERNAL_API_KEY,
            'X-Org-Id': activeOrg.orgId,
            'X-User-Id': activeOrg.userId,
            'X-Service-Name': 'frontend',
            'X-Idempotency-Key': idempotencyKey,
          },
          body: JSON.stringify(document),
        })

        if (ingestResponse.ok) {
          ingestedCount++
        } else {
          failedCount++
          errors.push(`${url}: ${ingestResponse.status}`)
        }
      } catch (err) {
        failedCount++
        errors.push(`${url}: ${err instanceof Error ? err.message : String(err)}`)
      }
    }

    const response: IngestResponse = {
      // Success means the call itself worked — onboarding can move on even
      // when zero pages were fetched (an empty domain shouldn't strand the
      // user on the wizard step).
      success: true,
      jobId: job.id,
      jobStatus: job.status,
      pagesFetched: pageFetched.length,
      pagesFailed: pageFailed.length,
      ingestedCount,
      failedCount,
      bodiesFetched,
      totalProducts: pageFetched.length,
      errors: errors.length > 0 ? errors : undefined,
      message:
        pageFetched.length === 0
          ? `Crawl job ${crawlJobId} (${job.status}): 0 pages fetched yet`
          : `Ingested ${ingestedCount}/${pageFetched.length} pages (${bodiesFetched} with body) from crawl job ${crawlJobId}`,
    }

    return NextResponse.json(response)
  } catch (err) {
    console.error('Ingest job API error:', err)
    return NextResponse.json({ error: 'Internal server error' }, { status: 500 })
  }
}
