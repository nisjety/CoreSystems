import { NextRequest, NextResponse } from 'next/server'

const ORG_CORE_URL = process.env.ORG_CORE_URL || 'http://org-core:8080'
const QUARRY_URL = process.env.QUARRY_URL || 'http://localhost:9090'

export interface DashboardStats {
  memberCount: number | null
  sourceCount: number | null
  documentCount: number | null
  crawledPages: number | null
  crawlStatus: 'idle' | 'running' | 'done' | 'error' | null
  lastCrawlAt: string | null
}

function extractCount(data: unknown): number | null {
  if (typeof data === 'number') return data
  if (Array.isArray(data)) return data.length
  if (data && typeof data === 'object') {
    const d = data as Record<string, unknown>
    if (typeof d.total === 'number') return d.total
    if (typeof d.count === 'number') return d.count
    if (Array.isArray(d.data)) return d.data.length
    if (Array.isArray(d.members)) return d.members.length
    if (Array.isArray(d.items)) return d.items.length
  }
  return null
}

async function safeFetch(url: string, cookies?: string): Promise<unknown> {
  const headers: Record<string, string> = { 'Accept': 'application/json' }
  if (cookies) headers['Cookie'] = cookies
  try {
    const res = await fetch(url, {
      cache: 'no-store',
      headers,
      signal: AbortSignal.timeout(3000),
    })
    if (!res.ok) return null
    return res.json()
  } catch {
    return null
  }
}

export async function GET(request: NextRequest) {
  const cookies = request.headers.get('cookie') ?? undefined
  const stats: DashboardStats = {
    memberCount: null,
    sourceCount: null,
    documentCount: null,
    crawledPages: null,
    crawlStatus: null,
    lastCrawlAt: null,
  }

  await Promise.allSettled([
    // Team member count from org-core — forward session cookies so auth passes
    safeFetch(`${ORG_CORE_URL}/api/v1/orgs/me/members`, cookies)
      .then((d) => { stats.memberCount = extractCount(d) }),

    // Active source count from Quarry
    safeFetch(`${QUARRY_URL}/api/v1/sources`)
      .then((d) => { stats.sourceCount = extractCount(d) }),

    // Document count from Quarry
    safeFetch(`${QUARRY_URL}/api/v1/documents`)
      .then((d) => { stats.documentCount = extractCount(d) }),

    // Pages indexed from latest crawl job
    safeFetch(`${QUARRY_URL}/api/v1/crawl/jobs?limit=1&sort=recent`)
      .then((d) => {
        if (d && typeof d === 'object') {
          const obj = d as Record<string, unknown>
          const jobs = Array.isArray(d)
            ? d
            : Array.isArray(obj.data)
              ? obj.data
              : Array.isArray(obj.jobs)
                ? obj.jobs
                : []
          if (jobs.length > 0) {
            const job = jobs[0] as Record<string, unknown>
            stats.crawledPages =
              typeof job.pagesIndexed === 'number' ? job.pagesIndexed
              : typeof job.pagesCrawled === 'number' ? job.pagesCrawled
              : typeof job.totalPages === 'number' ? job.totalPages
              : null
            stats.crawlStatus =
              (job.status as DashboardStats['crawlStatus']) ??
              (job.phase as DashboardStats['crawlStatus']) ??
              null
            stats.lastCrawlAt =
              typeof job.completedAt === 'string' ? job.completedAt
              : typeof job.updatedAt === 'string' ? job.updatedAt
              : typeof job.createdAt === 'string' ? job.createdAt
              : null
          }
        }
      }),
  ])

  return NextResponse.json(stats, {
    headers: { 'Cache-Control': 'public, s-maxage=60, stale-while-revalidate=120' },
  })
}
