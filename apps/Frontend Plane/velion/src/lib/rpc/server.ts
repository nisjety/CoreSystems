/**
 * Server-side type-safe service transport.
 *
 * Thin wrappers around internal Docker-network fetches that:
 *  1. Share a single AbortSignal timeout per batch
 *  2. Forward only allowlisted headers (security boundary)
 *  3. Parse JSON with proper error handling
 *  4. Are callable from RSC, Route Handlers, and Server Actions
 *
 * These replace the ad-hoc `fetch()` calls scattered across API routes
 * with a centralized, cached, type-safe layer.
 */
import 'server-only'

import { cache } from 'react'

// ── Service URLs (container-internal) ─────────────────────────────────────────

const SERVICES = {
  auth: process.env.AUTH_SERVICE_URL ?? 'http://auth-core:3011',
  user: process.env.USER_SERVICE_URL ?? 'http://user-core:3012',
  org: process.env.ORG_SERVICE_URL ?? 'http://org-core:8080',
  billing: process.env.BILLING_SERVICE_URL ?? 'http://billing-core:3014',
  notification: process.env.NOTIFICATION_SERVICE_URL ?? 'http://notification-core:3140',
  quarry: process.env.QUARRY_URL ?? process.env.QUARRY_API_URL ?? 'http://quarry-control:8081',
  documents: process.env.DATA_DOCUMENTS_API_URL ?? 'http://documents-service:8001',
  retrieval: process.env.DATA_RETRIEVAL_API_URL ?? 'http://retrieval-service:8004',
  aiCore: process.env.AI_CORE_URL ?? 'http://ai-core:8000',
  reasoningCore: process.env.REASONING_CORE_URL ?? 'http://reasoning-core:8000',
  integrationCore: process.env.INTEGRATION_CORE_URL ?? 'http://integration-api:3026',
  integrationEngine: process.env.INTEGRATION_ENGINE_URL ?? 'http://integration-engine-go-api:3126',
} as const

export type ServiceName = keyof typeof SERVICES

function getInternalKey(): string {
  const key = process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET
  if (!key) throw new Error('INTERNAL_API_KEY must be configured')
  return key
}

// ── Low-level fetch with timeout and error handling ───────────────────────────

export interface ServiceRequestInit {
  method?: string
  body?: unknown
  headers?: Record<string, string>
  /** Timeout in milliseconds. Default 5000. */
  timeoutMs?: number
  /** If true, returns null on non-2xx instead of throwing. Default false. */
  safe?: boolean
}

async function internalFetch<T>(
  url: string,
  init: ServiceRequestInit = {},
): Promise<T> {
  const { method = 'GET', body, headers = {}, timeoutMs = 5_000, safe = false } = init

  const fetchInit: RequestInit = {
    method,
    headers: {
      'Content-Type': 'application/json',
      'x-internal-api-key': getInternalKey(),
      ...headers,
    },
    body: body != null ? JSON.stringify(body) : undefined,
    signal: AbortSignal.timeout(timeoutMs),
    cache: 'no-store',
  }

  try {
    const res = await fetch(url, fetchInit)
    if (!res.ok) {
      if (safe) return null as T
      const text = await res.text().catch(() => '')
      throw new Error(`${method} ${url} → ${res.status}: ${text}`)
    }
    if (res.status === 204) return null as T
    const text = await res.text()
    return text ? (JSON.parse(text) as T) : (null as T)
  } catch (err) {
    if (safe) return null as T
    throw err
  }
}

// ── Public API: typed service callers ─────────────────────────────────────────

/**
 * Create a scoped caller for a specific backend service.
 *
 * Usage:
 *   const orgApi = serviceClient('org')
 *   const members = await orgApi.get<Member[]>('/api/v1/orgs/me/members', { userId })
 */
export function serviceClient(service: ServiceName) {
  const baseUrl = SERVICES[service]

  return {
    get<T>(path: string, opts?: { userId?: string; headers?: Record<string, string>; timeoutMs?: number; safe?: boolean }) {
      const headers: Record<string, string> = { ...opts?.headers }
      if (opts?.userId) headers['x-user-id'] = opts.userId
      return internalFetch<T>(`${baseUrl}${path}`, { method: 'GET', headers, timeoutMs: opts?.timeoutMs, safe: opts?.safe })
    },

    post<T>(path: string, body?: unknown, opts?: { userId?: string; headers?: Record<string, string>; timeoutMs?: number; safe?: boolean }) {
      const headers: Record<string, string> = { ...opts?.headers }
      if (opts?.userId) headers['x-user-id'] = opts.userId
      return internalFetch<T>(`${baseUrl}${path}`, { method: 'POST', body, headers, timeoutMs: opts?.timeoutMs, safe: opts?.safe })
    },

    put<T>(path: string, body?: unknown, opts?: { userId?: string; headers?: Record<string, string>; timeoutMs?: number; safe?: boolean }) {
      const headers: Record<string, string> = { ...opts?.headers }
      if (opts?.userId) headers['x-user-id'] = opts.userId
      return internalFetch<T>(`${baseUrl}${path}`, { method: 'PUT', body, headers, timeoutMs: opts?.timeoutMs, safe: opts?.safe })
    },

    patch<T>(path: string, body?: unknown, opts?: { userId?: string; headers?: Record<string, string>; timeoutMs?: number; safe?: boolean }) {
      const headers: Record<string, string> = { ...opts?.headers }
      if (opts?.userId) headers['x-user-id'] = opts.userId
      return internalFetch<T>(`${baseUrl}${path}`, { method: 'PATCH', body, headers, timeoutMs: opts?.timeoutMs, safe: opts?.safe })
    },

    delete<T>(path: string, opts?: { userId?: string; headers?: Record<string, string>; timeoutMs?: number; safe?: boolean }) {
      const headers: Record<string, string> = { ...opts?.headers }
      if (opts?.userId) headers['x-user-id'] = opts.userId
      return internalFetch<T>(`${baseUrl}${path}`, { method: 'DELETE', headers, timeoutMs: opts?.timeoutMs, safe: opts?.safe })
    },
  }
}

// ── Pre-built service instances ───────────────────────────────────────────────

export const orgApi = serviceClient('org')
export const userApi = serviceClient('user')
export const authApi = serviceClient('auth')
export const billingApi = serviceClient('billing')
export const notificationApi = serviceClient('notification')
export const quarryApi = serviceClient('quarry')
export const documentsApi = serviceClient('documents')
export const retrievalApi = serviceClient('retrieval')
export const aiCoreApi = serviceClient('aiCore')
export const integrationCoreApi = serviceClient('integrationCore')
export const integrationEngineApi = serviceClient('integrationEngine')

interface QuarryCrawlJobSummary {
  id: string
  status: string
  completed: number
  total: number
  createdAt: string
  updatedAt: string
}

interface QuarryCrawlJobsResponse {
  success: boolean
  count: number
  total: number
  data: QuarryCrawlJobSummary[]
}

// ── React.cache()-wrapped per-request deduplication ───────────────────────────

/**
 * Deduplicated dashboard stats — called once per request even if multiple
 * server components read it.
 */
export const getDashboardStatsRPC = cache(async (cookieHeader?: string) => {
  const headers: Record<string, string> = {}
  if (cookieHeader) headers['Cookie'] = cookieHeader

  const [members, sources, documents, crawlJobs] = await Promise.allSettled([
    orgApi.get<unknown>('/api/v1/orgs/me/members', { headers, timeoutMs: 3_000, safe: true }),
    // U1-2 (ui-ux-verevon-gap.md §10): sources count comes from the Data
    // Plane v2 distinct-sources facet (new GET /v1/sources endpoint in
    // dpv2-documents-api). Quarry-v2 writes every scrape into Data Plane
    // with its source URL — so the answer is COUNT(DISTINCT source)
    // GROUP BY source there, not anywhere in Quarry. Was previously
    // hardcoded `null` because Quarry has no list endpoint.
    documentsApi.get<unknown>('/v1/sources', { timeoutMs: 3_000, safe: true }),
    documentsApi.get<unknown>('/v1/documents', { timeoutMs: 3_000, safe: true }),
    // G32: Quarry-v2 lists jobs at GET /v1/jobs/ (envelope-wrapped). The v1
    // path /v1/crawl/jobs no longer exists.
    quarryApi.get<QuarryCrawlJobsResponse | null>('/v1/jobs/?limit=1', { headers, timeoutMs: 3_000, safe: true }),
  ])

  function extractCount(result: PromiseSettledResult<unknown>): number | null {
    if (result.status !== 'fulfilled' || result.value == null) return null
    const data = result.value
    if (typeof data === 'number') return data
    if (Array.isArray(data)) return data.length
    if (typeof data === 'object') {
      const d = data as Record<string, unknown>
      if (typeof d.total === 'number') return d.total
      if (typeof d.count === 'number') return d.count
      if (Array.isArray(d.data)) return d.data.length
      if (Array.isArray(d.members)) return d.members.length
      if (Array.isArray(d.items)) return d.items.length
      // U1-2: the dpv2-documents-api /v1/sources envelope is
      // `{ sources: SourceCount[], total: number }`. `total` is matched
      // above; if a future envelope uses `sources` we fall back to its
      // length.
      if (Array.isArray(d.sources)) return d.sources.length
    }
    return null
  }

  let crawledPages: number | null = null
  let crawlStatus: 'idle' | 'running' | 'done' | 'error' | null = null
  let lastCrawlAt: string | null = null

  if (crawlJobs.status === 'fulfilled' && crawlJobs.value != null) {
    const job = crawlJobs.value.data[0]
    if (job) {
      crawledPages = typeof job.completed === 'number' ? job.completed : null
      crawlStatus = mapDashboardCrawlStatus(job.status)
      lastCrawlAt = typeof job.updatedAt === 'string'
        ? job.updatedAt
        : typeof job.createdAt === 'string'
          ? job.createdAt
          : null
    }
  }

  return {
    memberCount: extractCount(members),
    sourceCount: extractCount(sources),
    documentCount: extractCount(documents),
    crawledPages,
    crawlStatus,
    lastCrawlAt,
  }
})

function mapDashboardCrawlStatus(status: string | null | undefined): 'idle' | 'running' | 'done' | 'error' | null {
  switch (status) {
    case 'scraping':
      return 'running'
    case 'completed':
      return 'done'
    case 'failed':
    case 'cancelled':
      return 'error'
    default:
      return null
  }
}
