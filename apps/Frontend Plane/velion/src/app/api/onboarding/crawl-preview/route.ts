/**
 * Phase 1 onboarding · live crawl preview (SSE).
 *
 * The wizard's "snippet drop" animation used to cycle through hard-coded
 * placeholder cards. This route makes it real: it kicks a `crawl` job on
 * `quarry-control` (port 8081), polls its event log for per-page
 * `page_fetched` events, and streams each one back as a multi-type
 * snippet event the WebsiteStep folds into its falling-card UI.
 *
 *   client                       this route                  quarry-control
 *     │  POST /api/onboarding/      │                              │
 *     │  crawl-preview {url}        │  POST /v1/jobs/              │
 *     │ ──────────────────────────► │   {kind:"crawl", params:{}}  │
 *     │                             │ ───────────────────────────► │
 *     │                             │ ◄── { id, status:"accepted" }│
 *     │                             │                              │
 *     │                             │  GET /v1/jobs/{id}/events    │
 *     │                             │  ?after_seq=N&limit=100      │
 *     │                             │ ───────────────────────────► │
 *     │                             │ ◄── [Event, Event, ...]      │
 *     │ ◄── SSE event: snippet      │                              │
 *     │ ◄── SSE event: snippet      │  (loop until run_completed,  │
 *     │ ◄── SSE event: done         │   run_failed, or timeout)    │
 *
 * The event shape (`snippet`) carries `{kind, title, excerpt?, thumbUrl?,
 * url, contentType}`. `kind` is one of `text | image | file | link` —
 * derived from the upstream content type so the UI can pick the right
 * card style. Live pages are also stored into Data Plane as org-scoped
 * `quarry` documents; the UI stream stays lightweight while persistence
 * fetches markdown bodies from Quarry Edge in the background.
 *
 * Quarry's `Job` record has no `result` field — pages flow through the
 * append-only event log only. We poll `/v1/jobs/{id}/events?after_seq=N`
 * and watch for `page_fetched` (success) events; the loop exits on the
 * terminal `run_completed` / `run_failed` events or on the overall
 * deadline. If Quarry is unreachable from the start, the route falls
 * back to a small set of synthesized snippets so the animation still
 * resolves in finite time. Onboarding must never hang on a flaky
 * control plane.
 */

import { NextRequest } from 'next/server'
import { promises as dns } from 'node:dns'

import { resolveActiveOrgContext } from '@/lib/server/active-org'

export const runtime = 'nodejs'
export const dynamic = 'force-dynamic'

const QUARRY_URL = (
  process.env.QUARRY_API_URL || 'http://quarry-control:8081'
).replace(/\/+$/, '')
const QUARRY_EDGE_URL = (
  process.env.QUARRY_EDGE_URL || 'http://quarry-edge:8082'
).replace(/\/+$/, '')
const DOCUMENTS_SERVICE_URL = (
  process.env.DATAPLANE_API_URL ||
  process.env.DOCUMENTS_SERVICE_URL ||
  process.env.DOCS_SERVICE_URL ||
  'http://data-documents-service:8001'
).replace(/\/+$/, '')
const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET

const TOTAL_DEADLINE_MS = 10_000
const POLL_INTERVAL_MS = 800
const POLL_FETCH_TIMEOUT_MS = 2_500
const BODY_FETCH_TIMEOUT_MS = 5_000
const KNOWLEDGE_STORE_WAIT_MS = 3_500
const MAX_SNIPPETS = 8
const MAX_DOCUMENT_CHARS = 60_000
/**
 * When the control plane accepts the job but no orchestrator picks
 * it up (e.g. dev environment with quarry-control running but no
 * quarry-runtime worker attached), `/v1/jobs/{id}/events` keeps
 * returning empty arrays. After this many consecutive empty polls
 * with no control-plane events we abandon the live path and fall
 * back to synthetic snippets — the wizard's SAFE_ADVANCE timer is
 * 12 s, so we must hit fallback well before that.
 */
const ABANDON_LIVE_AFTER_EMPTY_POLLS = 8

type SnippetKind = 'text' | 'image' | 'file' | 'link'

interface SnippetEvent {
  id: string
  kind: SnippetKind
  title: string
  excerpt?: string
  thumbUrl?: string
  url: string
  contentType?: string
}

interface QuarryEnvelope<T> {
  data?: T | null
  error?: { code?: string; message?: string } | null
}

interface QuarryJob {
  id: string
  status: string
}

interface QuarryEvent {
  event_id?: string
  job_id?: string
  type: string
  seq: number
  ts?: string
  payload?: Record<string, unknown> | null
}

interface CrawlPreviewBody {
  url?: string
  brief?: string
  /** Hard cap so a runaway crawl can't flood the stream. */
  maxPages?: number
}

interface ActiveCrawlPage {
  url: string
  title?: string
  contentType?: string
  status?: number
  fingerprint?: string
  links?: number
}

interface KnowledgeStoreResult {
  stored: boolean
  bodyFetched: boolean
  url: string
  error?: string
}

interface QuarryFormatRef {
  artifact_id?: string
  bytes?: number
}

interface QuarryScrapeEnvelope {
  data?: {
    formats?: {
      markdown?: QuarryFormatRef
      html?: QuarryFormatRef
    }
    metadata?: {
      title?: string
    }
  }
}

const TERMINAL_EVENT_TYPES = new Set([
  'run_completed',
  'run_failed',
  'run_cancelled',
])

function sse(event: string, payload: unknown): string {
  return `event: ${event}\ndata: ${JSON.stringify(payload)}\n\n`
}

/**
 * Derive the right snippet card style from the upstream MIME type.
 * `content_type` from Quarry is the raw HTTP header value
 * (`text/html; charset=utf-8`) so we lowercase + strip parameters first.
 */
function snippetKindFor(contentType: string | undefined): SnippetKind {
  const ct = (contentType ?? '').toLowerCase().split(';')[0]?.trim() ?? ''
  if (ct.startsWith('image/')) return 'image'
  if (
    ct === 'application/pdf' ||
    ct === 'application/msword' ||
    ct ===
      'application/vnd.openxmlformats-officedocument.wordprocessingml.document' ||
    ct === 'application/vnd.ms-excel' ||
    ct ===
      'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet' ||
    ct === 'text/csv'
  ) {
    return 'file'
  }
  if (ct.startsWith('text/')) return 'text'
  return 'link'
}

/**
 * Trim a URL to the path portion (`/about`, `/docs/api`) for the card
 * title — the full URL is too long for the small drop cards.
 */
function shortenUrl(url: string): string {
  try {
    const parsed = new URL(url)
    const path = parsed.pathname.replace(/\/+$/, '')
    return path === '' ? parsed.host : path
  } catch {
    return url.slice(0, 32)
  }
}

function buildSnippet(
  url: string,
  title: string | undefined,
  contentType: string | undefined,
  index: number,
): SnippetEvent {
  const kind = snippetKindFor(contentType)
  return {
    id: `${index}-${url}`,
    kind,
    title: shortenUrl(url),
    excerpt: title || (kind === 'text' ? 'Indexerer side …' : undefined),
    // For images, the URL is the thumbnail — the WebsiteStep renders it
    // as <img src={thumbUrl}> with crossOrigin="anonymous". CORS-blocked
    // sites just show the fallback icon.
    thumbUrl: kind === 'image' ? url : undefined,
    url,
    contentType,
  }
}

/**
 * Pull `{url, title, content_type}` out of a `page_fetched` event payload.
 * Field names are best-effort across emission sites — try the most
 * specific (`final_url`) before the generic (`url`).
 */
function pageFieldsFromPayload(
  payload: Record<string, unknown> | null | undefined,
): ActiveCrawlPage | null {
  if (!payload || typeof payload !== 'object') return null
  const urlVal =
    (typeof payload.final_url === 'string' && payload.final_url) ||
    (typeof payload.url === 'string' && payload.url) ||
    (typeof payload.target_url === 'string' && payload.target_url) ||
    ''
  if (!urlVal) return null
  const title =
    (typeof payload.title === 'string' && payload.title) || undefined
  const contentType =
    (typeof payload.content_type === 'string' && payload.content_type) ||
    (typeof payload.contentType === 'string' && payload.contentType) ||
    undefined
  const status =
    typeof payload.status === 'number'
      ? payload.status
      : typeof payload.status_code === 'number'
        ? payload.status_code
        : undefined
  const fingerprint =
    (typeof payload.fingerprint === 'string' && payload.fingerprint) ||
    undefined
  const links =
    typeof payload.links === 'number'
      ? payload.links
      : Array.isArray(payload.links)
        ? payload.links.length
        : undefined
  return { url: urlVal, title, contentType, status, fingerprint, links }
}

async function fetchPageBody(url: string): Promise<{ body: string; title?: string }> {
  const scrapeResponse = await fetch(`${QUARRY_EDGE_URL}/v1/scrape`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
    body: JSON.stringify({ url, formats: ['markdown'] }),
    signal: AbortSignal.timeout(BODY_FETCH_TIMEOUT_MS),
    cache: 'no-store',
  })
  if (!scrapeResponse.ok) {
    throw new Error(`quarry-edge scrape HTTP ${scrapeResponse.status}`)
  }

  const envelope = (await scrapeResponse
    .json()
    .catch(() => null)) as QuarryScrapeEnvelope | null
  const ref = envelope?.data?.formats?.markdown
  if (!ref?.artifact_id) {
    throw new Error('quarry-edge returned no markdown artifact')
  }

  const bytesResponse = await fetch(
    `${QUARRY_EDGE_URL}/v1/artifacts/${encodeURIComponent(ref.artifact_id)}/bytes`,
    {
      method: 'GET',
      signal: AbortSignal.timeout(BODY_FETCH_TIMEOUT_MS),
      cache: 'no-store',
    },
  )
  if (!bytesResponse.ok) {
    throw new Error(`quarry-edge artifact HTTP ${bytesResponse.status}`)
  }

  return {
    body: await bytesResponse.text(),
    title: envelope?.data?.metadata?.title,
  }
}

function fallbackPageContent(page: ActiveCrawlPage, seedUrl: string, jobId: string): string {
  return [
    `# ${page.title || shortenUrl(page.url)}`,
    '',
    `Source URL: ${page.url}`,
    `Seed URL: ${seedUrl}`,
    `Crawl job: ${jobId}`,
    page.contentType ? `Content type: ${page.contentType}` : null,
    typeof page.status === 'number' ? `HTTP status: ${page.status}` : null,
  ]
    .filter(Boolean)
    .join('\n')
}

function cleanDocumentTitle(page: ActiveCrawlPage, title?: string): string {
  const value = (title || page.title || shortenUrl(page.url)).trim()
  if (!value) return shortenUrl(page.url)
  return value.length > 180 ? `${value.slice(0, 177)}...` : value
}

function truncateContent(content: string): string {
  const trimmed = content.trim()
  if (trimmed.length <= MAX_DOCUMENT_CHARS) return trimmed
  return `${trimmed.slice(0, MAX_DOCUMENT_CHARS)}\n\n[Content truncated for onboarding ingest]`
}

type KnowledgeActor = {
  userId: string
  userEmail: string
  userName: string
  orgId: string
}

async function resolveKnowledgeActor(): Promise<KnowledgeActor | null> {
  if (!INTERNAL_API_KEY) return null
  const actor = await resolveActiveOrgContext().catch(() => null)
  if (!actor?.orgId) return null
  return {
    userId: actor.userId,
    userEmail: actor.userEmail,
    userName: actor.userName,
    orgId: actor.orgId,
  }
}

async function storeCrawledPageToKnowledgebase({
  actor,
  page,
  seedUrl,
  jobId,
}: {
  actor: KnowledgeActor
  page: ActiveCrawlPage
  seedUrl: string
  jobId: string
}): Promise<KnowledgeStoreResult> {
  try {
    let body = ''
    let title: string | undefined
    try {
      const scraped = await fetchPageBody(page.url)
      body = scraped.body
      title = scraped.title
    } catch {
      // Keep the page represented in the knowledge base even if the
      // markdown artifact cannot be materialised this time. Source
      // remains `quarry`, so a later crawl of the same org+URL updates
      // this placeholder through Data Plane's existing dedupe path.
      body = ''
    }

    const bodyFetched = body.trim().length > 0
    const content = truncateContent(
      bodyFetched ? body : fallbackPageContent(page, seedUrl, jobId),
    )

    const response = await fetch(`${DOCUMENTS_SERVICE_URL}/v1/documents`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Internal-Api-Key': INTERNAL_API_KEY ?? '',
        'X-Org-ID': actor.orgId,
        'X-User-Id': actor.userId,
        'X-User-Email': actor.userEmail,
        'X-User-Name': actor.userName,
        'X-Service-Name': 'frontend-onboarding',
      },
      body: JSON.stringify({
        source: 'quarry',
        type: 'webpage',
        title: cleanDocumentTitle(page, title),
        content,
        metadata: {
          crawl_job_id: jobId,
          content_type: page.contentType,
          extracted_at: new Date().toISOString(),
          fingerprint: page.fingerprint,
          links: page.links,
          page_status: page.status,
          source_url: seedUrl,
          url: page.url,
          via: 'onboarding.website',
          body_bytes: body.length,
        },
      }),
      signal: AbortSignal.timeout(BODY_FETCH_TIMEOUT_MS),
      cache: 'no-store',
    })

    if (!response.ok) {
      return {
        stored: false,
        bodyFetched,
        url: page.url,
        error: `documents-service HTTP ${response.status}`,
      }
    }

    return { stored: true, bodyFetched, url: page.url }
  } catch (error) {
    return {
      stored: false,
      bodyFetched: false,
      url: page.url,
      error: error instanceof Error ? error.message : 'unknown ingest error',
    }
  }
}

async function summarizeKnowledgeStores(
  tasks: Array<Promise<KnowledgeStoreResult>>,
): Promise<{
  stored: number
  failed: number
  bodiesFetched: number
  pending: number
}> {
  if (tasks.length === 0) {
    return { stored: 0, failed: 0, bodiesFetched: 0, pending: 0 }
  }

  const timeout = new Promise<'timeout'>((resolve) => {
    setTimeout(() => resolve('timeout'), KNOWLEDGE_STORE_WAIT_MS)
  })
  const settled = await Promise.race([Promise.all(tasks), timeout])
  if (settled === 'timeout') {
    return { stored: 0, failed: 0, bodiesFetched: 0, pending: tasks.length }
  }

  return settled.reduce(
    (acc, item) => ({
      stored: acc.stored + (item.stored ? 1 : 0),
      failed: acc.failed + (item.stored ? 0 : 1),
      bodiesFetched: acc.bodiesFetched + (item.bodyFetched ? 1 : 0),
      pending: 0,
    }),
    { stored: 0, failed: 0, bodiesFetched: 0, pending: 0 },
  )
}

/**
 * Hard-coded fallback used when Quarry isn't reachable. The shape
 * matches a real discover response so the WebsiteStep treats them as
 * indistinguishable from live data.
 */
function fallbackSnippets(seedUrl: string): SnippetEvent[] {
  const base = (() => {
    try {
      return new URL(seedUrl).origin
    } catch {
      return seedUrl
    }
  })()
  const items: Array<{ path: string; kind: SnippetKind; title: string }> = [
    { path: '/', kind: 'text', title: 'Forside' },
    { path: '/about', kind: 'text', title: 'Om oss' },
    { path: '/pricing', kind: 'text', title: 'Pris og planer' },
    { path: '/docs', kind: 'text', title: 'Dokumentasjon' },
    { path: '/blog/launch.pdf', kind: 'file', title: 'Lanseringsnotat' },
    { path: '/team.jpg', kind: 'image', title: 'Team' },
    { path: '/contact', kind: 'link', title: 'Kontakt' },
  ]
  return items.map((item, i) => ({
    id: `fallback-${i}`,
    kind: item.kind,
    title: item.path,
    excerpt: item.title,
    thumbUrl: item.kind === 'image' ? `${base}${item.path}` : undefined,
    url: `${base}${item.path}`,
    contentType:
      item.kind === 'image'
        ? 'image/jpeg'
        : item.kind === 'file'
          ? 'application/pdf'
          : 'text/html',
  }))
}

/**
 * SSRF guard. The wizard takes any URL the user types; without this
 * check we'd happily POST `http://10.0.0.5:8080/admin` to Quarry and
 * use the velion server's network position to reach internal hosts.
 * Quarry-edge has its own security engine but the velion route is the
 * outermost trust boundary — we must validate before the request ever
 * leaves the process.
 *
 * Returns the parsed URL when safe, or a structured reason when not.
 * The caller emits `event: warning {code, message}` and falls through
 * to synthetic snippets so the wizard still completes.
 */
type SSRFBlock = { ok: false; code: SsrfReasonCode; message: string }
type SSRFOk = { ok: true; url: URL }
type SsrfReasonCode =
  | 'invalid_url'
  | 'bad_scheme'
  | 'no_hostname'
  | 'dns_failed'
  | 'private_address'

const BLOCKED_IPV4_RANGES: ReadonlyArray<{
  network: number
  mask: number
  label: string
}> = [
  { network: 0x00000000, mask: 0xff000000, label: '0.0.0.0/8 (this network)' },
  { network: 0x7f000000, mask: 0xff000000, label: '127.0.0.0/8 (loopback)' },
  { network: 0x0a000000, mask: 0xff000000, label: '10.0.0.0/8 (private)' },
  { network: 0xac100000, mask: 0xfff00000, label: '172.16.0.0/12 (private)' },
  { network: 0xc0a80000, mask: 0xffff0000, label: '192.168.0.0/16 (private)' },
  {
    network: 0xa9fe0000,
    mask: 0xffff0000,
    label: '169.254.0.0/16 (link-local)',
  },
  { network: 0x64400000, mask: 0xffc00000, label: '100.64.0.0/10 (CGNAT)' },
  { network: 0xe0000000, mask: 0xf0000000, label: '224.0.0.0/4 (multicast)' },
  { network: 0xf0000000, mask: 0xf0000000, label: '240.0.0.0/4 (reserved)' },
]

function ipv4ToInt(ip: string): number | null {
  const parts = ip.split('.')
  if (parts.length !== 4) return null
  let n = 0
  for (const p of parts) {
    const v = Number(p)
    if (!Number.isInteger(v) || v < 0 || v > 255) return null
    n = (n << 8) | v
  }
  return n >>> 0
}

function blockedIPv4Range(ip: string): string | null {
  const n = ipv4ToInt(ip)
  if (n === null) return null
  for (const r of BLOCKED_IPV4_RANGES) {
    if ((n & r.mask) === r.network) return r.label
  }
  return null
}

function blockedIPv6Range(ip: string): string | null {
  const lower = ip.toLowerCase()
  if (lower === '::1' || lower === '::') return 'loopback (IPv6)'
  // fc00::/7 — Unique Local Addresses (RFC 4193). First byte is fc or fd.
  if (lower.startsWith('fc') || lower.startsWith('fd')) return 'fc00::/7 (ULA)'
  // fe80::/10 — link-local. First 10 bits = 1111 1110 10xx → fe8x..feax..febx
  if (/^fe[89ab]/.test(lower)) return 'fe80::/10 (link-local)'
  // IPv4-mapped IPv6 (::ffff:1.2.3.4) — recheck against IPv4 ranges.
  const m = lower.match(/^::ffff:(\d+\.\d+\.\d+\.\d+)$/)
  if (m) return blockedIPv4Range(m[1])
  return null
}

async function validatePublicURL(rawUrl: string): Promise<SSRFOk | SSRFBlock> {
  let parsed: URL
  try {
    parsed = new URL(rawUrl)
  } catch {
    return { ok: false, code: 'invalid_url', message: 'That URL is not valid.' }
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    return {
      ok: false,
      code: 'bad_scheme',
      message: `URL must start with http:// or https:// (got ${parsed.protocol}).`,
    }
  }
  if (!parsed.hostname) {
    return { ok: false, code: 'no_hostname', message: 'URL has no hostname.' }
  }
  // If the hostname is already a literal IP, skip DNS and check it directly.
  // (URL.hostname strips brackets from IPv6 literals.)
  const literalReason =
    blockedIPv4Range(parsed.hostname) ?? blockedIPv6Range(parsed.hostname)
  if (literalReason) {
    return {
      ok: false,
      code: 'private_address',
      message: `Host is a private/reserved address: ${literalReason}.`,
    }
  }
  let addrs: Array<{ address: string; family: number }>
  try {
    addrs = await dns.lookup(parsed.hostname, { all: true, verbatim: true })
  } catch {
    return {
      ok: false,
      code: 'dns_failed',
      message: `Could not resolve ${parsed.hostname}.`,
    }
  }
  if (addrs.length === 0) {
    return {
      ok: false,
      code: 'dns_failed',
      message: `No DNS records for ${parsed.hostname}.`,
    }
  }
  for (const a of addrs) {
    const reason =
      a.family === 4 ? blockedIPv4Range(a.address) : blockedIPv6Range(a.address)
    if (reason) {
      return {
        ok: false,
        code: 'private_address',
        message: `${parsed.hostname} resolves to ${a.address} — ${reason}.`,
      }
    }
  }
  return { ok: true, url: parsed }
}

/**
 * Build a request-scoped idempotency key.
 *
 * This used to be derived from {url, cap, day}. That made retries too
 * sticky for onboarding: if a crawl failed once, every later submit of
 * the same URL on the same day reused the failed control-plane job and
 * never gave Quarry a chance to fetch the site again. The browser form
 * already disables duplicate submits, and `fetch()` does not retry the
 * POST automatically, so a fresh key per submit is the safer default.
 */
async function buildIdempotencyKey(
  seedUrl: string,
  cap: number,
): Promise<string> {
  const nonce = crypto.randomUUID()
  const input = `velion-onboarding-crawl|${seedUrl}|cap=${cap}|nonce=${nonce}`
  // Node's WebCrypto is available in Next.js node runtime.
  const data = new TextEncoder().encode(input)
  const digest = await crypto.subtle.digest('SHA-256', data)
  // Slice to 32 hex chars (128 bits) — bounded enough to satisfy the
  // 128-char index cap on control while preserving collision resistance.
  const hex = Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('')
  return `velion-crawl-${hex.slice(0, 32)}`
}

async function fetchWithTimeout(
  url: string,
  init: RequestInit,
  timeoutMs: number,
): Promise<Response | null> {
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), timeoutMs)
  try {
    return await fetch(url, {
      ...init,
      signal: controller.signal,
      cache: 'no-store',
    })
  } catch {
    return null
  } finally {
    clearTimeout(timer)
  }
}

/**
 * POST a `crawl` job to quarry-control. Returns the job id or `null`
 * when the control plane is unreachable / rejects the request.
 *
 * `idempotencyKey` is sent as the `Idempotency-Key` HTTP header.
 * Control returns the existing job (200) instead of creating a
 * duplicate (201) if the key has been seen before — making a user's
 * double-click or a network retry safe.
 */
async function createCrawlJob(
  seedUrl: string,
  maxPages: number,
  idempotencyKey: string,
): Promise<string | null> {
  const response = await fetchWithTimeout(
    `${QUARRY_URL}/v1/jobs/`,
    {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Accept: 'application/json',
        'Idempotency-Key': idempotencyKey,
      },
      body: JSON.stringify({
        kind: 'crawl',
        params: {
          url: seedUrl,
          max_pages: maxPages,
          max_depth: 1,
          auto_commit: true,
        },
      }),
    },
    POLL_FETCH_TIMEOUT_MS,
  )
  if (!response || !response.ok) return null
  const envelope = (await response
    .json()
    .catch(() => null)) as QuarryEnvelope<QuarryJob> | QuarryJob | null
  if (!envelope) return null
  // quarry-control returns the job record directly (no envelope) for
  // the create endpoint; the read endpoints use an envelope. Handle both.
  const direct = envelope as QuarryJob
  if (typeof direct.id === 'string' && direct.id) return direct.id
  const enveloped = envelope as QuarryEnvelope<QuarryJob>
  if (enveloped?.data?.id) return enveloped.data.id
  return null
}

/**
 * Poll `/v1/jobs/{id}/events?after_seq=...` once. Returns the next
 * after-seq cursor and whether a terminal event was seen.
 */
async function fetchEventsBatch(
  jobId: string,
  afterSeq: number,
): Promise<{ events: QuarryEvent[]; nextSeq: number; terminal: boolean }> {
  const url = `${QUARRY_URL}/v1/jobs/${encodeURIComponent(jobId)}/events?after_seq=${afterSeq}&limit=100`
  const response = await fetchWithTimeout(
    url,
    {
      method: 'GET',
      headers: { Accept: 'application/json' },
    },
    POLL_FETCH_TIMEOUT_MS,
  )
  if (!response || !response.ok) {
    return { events: [], nextSeq: afterSeq, terminal: false }
  }
  const raw = (await response.json().catch(() => null)) as unknown
  // The events endpoint returns a bare JSON array. Belt-and-braces: also
  // accept `{data: [...]}` in case the contract gets wrapped later.
  const events: QuarryEvent[] = Array.isArray(raw)
    ? (raw as QuarryEvent[])
    : Array.isArray((raw as { data?: unknown })?.data)
      ? ((raw as { data: QuarryEvent[] }).data)
      : []
  let nextSeq = afterSeq
  let terminal = false
  for (const ev of events) {
    if (typeof ev?.seq === 'number' && ev.seq > nextSeq) nextSeq = ev.seq
    if (ev?.type && TERMINAL_EVENT_TYPES.has(ev.type)) terminal = true
  }
  return { events, nextSeq, terminal }
}

export async function POST(request: NextRequest): Promise<Response> {
  let body: CrawlPreviewBody
  try {
    body = (await request.json()) as CrawlPreviewBody
  } catch {
    body = {}
  }

  const rawUrl = (body.url ?? '').trim()
  if (!rawUrl) {
    return new Response(
      JSON.stringify({ error: 'Missing required field: url' }),
      { status: 400, headers: { 'Content-Type': 'application/json' } },
    )
  }
  // The wizard's URL input strips `https?://` for display (the `https://`
  // prefix sits in a separate visual element) and submits the bare host.
  // Quarry's runtime parses with `url::Url::parse` which rejects strings
  // without a scheme. We normalise here: prepend `https://` only when
  // the input is unambiguous (no scheme at all). If it already looks
  // like a URL with a non-http(s) scheme (`file://`, `gopher://`, etc.)
  // we leave it as-is so the SSRF guard further down rejects it with
  // the precise `bad_scheme` reason instead of a misleading `dns_failed`.
  const hasAnyScheme = /^[a-z][a-z0-9+.-]*:/i.test(rawUrl)
  const seedUrl = hasAnyScheme ? rawUrl : `https://${rawUrl}`

  const cap = Math.max(1, Math.min(body.maxPages ?? MAX_SNIPPETS, 24))

  const stream = new ReadableStream<Uint8Array>({
    async start(controller) {
      const encoder = new TextEncoder()
      let closed = false
      const enqueue = (chunk: string) => {
        if (closed) return
        try {
          controller.enqueue(encoder.encode(chunk))
        } catch {
          closed = true
        }
      }
      const close = () => {
        if (closed) return
        closed = true
        try {
          controller.close()
        } catch {
          // already closed — ignore
        }
      }

      const onAbort = () => {
        closed = true
        try {
          controller.close()
        } catch {
          // ignore
        }
      }
      request.signal.addEventListener('abort', onAbort, { once: true })

      enqueue(sse('started', { url: seedUrl, cap }))
      const knowledgeActorPromise = resolveKnowledgeActor()

      // SSRF guard: refuse to forward private/loopback/link-local hosts to
      // Quarry. The wizard's URL input is user-controlled — without this
      // check the velion server's network position could be used to probe
      // internal services. On block we emit a structured warning and fall
      // through to synthetic snippets so the wizard still completes.
      const validation = await validatePublicURL(seedUrl)
      if (!validation.ok) {
        enqueue(
          sse('warning', {
            code: validation.code,
            message: validation.message,
          }),
        )
        const fallback = fallbackSnippets(seedUrl).slice(0, cap)
        for (let i = 0; i < fallback.length; i += 1) {
          if (closed) break
          enqueue(sse('snippet', fallback[i]))
          await new Promise((resolve) => setTimeout(resolve, 350))
        }
        enqueue(
          sse('done', {
            count: fallback.length,
            source: 'fallback',
            reason: validation.code,
          }),
        )
        request.signal.removeEventListener('abort', onAbort)
        close()
        return
      }

      // Fresh per-submit idempotency avoids the old "failed crawl is
      // sticky for the rest of the day" behavior while still keeping
      // accidental network retries bounded by the same request.
      const idempotencyKey = await buildIdempotencyKey(seedUrl, cap)
      const jobId = await createCrawlJob(seedUrl, cap, idempotencyKey)

      if (!jobId) {
        // Quarry unreachable or rejected the create. Fall back to the
        // illustrative set so onboarding still finishes.
        enqueue(
          sse('warning', {
            code: 'control_unreachable',
            message:
              'Live crawl preview unavailable — showing illustrative snippets.',
          }),
        )
        const fallback = fallbackSnippets(seedUrl).slice(0, cap)
        for (let i = 0; i < fallback.length; i += 1) {
          if (closed) break
          enqueue(sse('snippet', fallback[i]))
          await new Promise((resolve) => setTimeout(resolve, 350))
        }
        enqueue(
          sse('done', {
            count: fallback.length,
            source: 'fallback',
            reason: 'control_unreachable',
          }),
        )
        request.signal.removeEventListener('abort', onAbort)
        close()
        return
      }
      const knowledgeActor = await knowledgeActorPromise
      const knowledgeStoreTasks: Array<Promise<KnowledgeStoreResult>> = []
      if (!knowledgeActor) {
        enqueue(
          sse('warning', {
            code: 'knowledge_store_unavailable',
            message:
              'Website preview is live, but the active organization could not be resolved for knowledge-base storage.',
          }),
        )
      }

      // Live path: poll the event log until terminal / deadline / cap.
      let afterSeq = 0
      let emitted = 0
      let terminal = false
      let firstFailureMessage: string | null = null
      let consecutiveEmptyPolls = 0
      const deadline = Date.now() + TOTAL_DEADLINE_MS
      const seenUrls = new Set<string>()

      while (!closed && !terminal && emitted < cap && Date.now() < deadline) {
        const batch = await fetchEventsBatch(jobId, afterSeq)
        afterSeq = batch.nextSeq
        terminal = batch.terminal

        if (batch.events.length === 0) {
          consecutiveEmptyPolls += 1
        } else {
          consecutiveEmptyPolls = 0
        }
        if (
          emitted === 0 &&
          consecutiveEmptyPolls >= ABANDON_LIVE_AFTER_EMPTY_POLLS
        ) {
          // Control plane is up but nothing is actually crawling. Bail
          // out of the live loop now so the user sees fallback snippets
          // before the wizard's safety timer auto-advances. The bottom
          // `if (emitted === 0)` block handles the fallback emission.
          break
        }

        for (const ev of batch.events) {
          if (closed) break
          // Per-page branding payload (favicon, theme color, palette,
          // logo candidate, font family, og:image). Forwarded directly
          // to the client so the wizard can paint real brand colors as
          // soon as the seed page is parsed — well before the full
          // crawl finishes.
          if (ev?.type === 'branding_extracted') {
            const payload = ev.payload ?? {}
            const brandingUrl =
              typeof payload.url === 'string' ? payload.url : undefined
            const branding =
              payload.branding && typeof payload.branding === 'object'
                ? payload.branding
                : null
            if (brandingUrl && branding) {
              enqueue(sse('branding', { url: brandingUrl, branding }))
            }
            continue
          }
          if (ev?.type === 'page_failed' && !firstFailureMessage) {
            const payload = ev.payload ?? {}
            firstFailureMessage =
              typeof payload.error === 'string' ? payload.error : null
            continue
          }
          if (emitted >= cap) continue
          if (ev?.type !== 'page_fetched') continue
          const fields = pageFieldsFromPayload(ev.payload)
          if (!fields) continue
          if (seenUrls.has(fields.url)) continue
          seenUrls.add(fields.url)
          const snippet = buildSnippet(
            fields.url,
            fields.title,
            fields.contentType,
            emitted,
          )
          enqueue(sse('snippet', snippet))
          if (knowledgeActor) {
            knowledgeStoreTasks.push(
              storeCrawledPageToKnowledgebase({
                actor: knowledgeActor,
                page: fields,
                seedUrl,
                jobId,
              }),
            )
          }
          emitted += 1
        }

        if (terminal || emitted >= cap) break
        // Quarry's event log isn't push-based — wait a beat before polling
        // again so we don't hammer the control plane.
        await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS))
      }

      if (emitted === 0 && !closed) {
        // Job accepted but never produced any pages within the deadline.
        // Either no orchestrator picked it up, or the target site was
        // unreachable from the runtime, or the crawler was blocked by
        // robots/security. Fall back so the wizard doesn't show an
        // empty animation. The wizard surfaces the reason code.
        enqueue(
          sse('warning', {
            code: firstFailureMessage ? 'crawl_failed' : 'no_events',
            message: firstFailureMessage
              ? `Quarry could not fetch the seed page: ${firstFailureMessage}`
              : 'Live crawl preview unavailable — showing illustrative snippets.',
          }),
        )
        const fallback = fallbackSnippets(seedUrl).slice(0, cap)
        for (let i = 0; i < fallback.length; i += 1) {
          if (closed) break
          enqueue(sse('snippet', fallback[i]))
          await new Promise((resolve) => setTimeout(resolve, 350))
        }
        emitted = fallback.length
        enqueue(
          sse('done', { count: emitted, source: 'fallback', reason: 'no_events' }),
        )
      } else if (!closed) {
        const storeSummary = await summarizeKnowledgeStores(knowledgeStoreTasks)
        enqueue(
          sse('done', {
            count: emitted,
            source: 'live',
            jobId,
            stored: storeSummary.stored,
            storeFailed: storeSummary.failed,
            storePending: storeSummary.pending,
            bodiesFetched: storeSummary.bodiesFetched,
          }),
        )
      }
      request.signal.removeEventListener('abort', onAbort)
      close()
    },
  })

  return new Response(stream, {
    headers: {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache, no-transform',
      Connection: 'keep-alive',
      'X-Accel-Buffering': 'no',
    },
  })
}
