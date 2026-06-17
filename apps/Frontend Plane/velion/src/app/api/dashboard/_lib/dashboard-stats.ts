const ORG_CORE_URL = process.env.ORG_CORE_URL || 'http://org-core:8080';
const DOCUMENTS_URL = process.env.DATA_DOCUMENTS_API_URL || 'http://data-documents-service:8001';

export interface DashboardStats {
  memberCount: number | null;
  sourceCount: number | null;
  documentCount: number | null;
  crawledPages: number | null;
  crawlStatus: 'idle' | 'running' | 'done' | 'error' | null;
  lastCrawlAt: string | null;
}

function extractCount(data: unknown): number | null {
  if (typeof data === 'number') return data;
  if (Array.isArray(data)) return data.length;
  if (data && typeof data === 'object') {
    const d = data as Record<string, unknown>;
    if (typeof d.total === 'number') return d.total;
    if (typeof d.count === 'number') return d.count;
    if (Array.isArray(d.data)) return d.data.length;
    if (Array.isArray(d.members)) return d.members.length;
    if (Array.isArray(d.items)) return d.items.length;
  }
  return null;
}

async function safeFetch(url: string, cookieHeader?: string): Promise<unknown> {
  const headers: Record<string, string> = { Accept: 'application/json' };
  if (cookieHeader) {
    headers.Cookie = cookieHeader;
  }

  try {
    const response = await fetch(url, {
      headers,
      signal: AbortSignal.timeout(3000),
      next: { revalidate: 60 },
    });

    if (!response.ok) {
      return null;
    }

    return response.json();
  } catch {
    return null;
  }
}

export async function getDashboardStats(cookieHeader?: string): Promise<DashboardStats> {
  const stats: DashboardStats = {
    memberCount: null,
    sourceCount: null,
    documentCount: null,
    crawledPages: null,
    crawlStatus: null,
    lastCrawlAt: null,
  };

  await Promise.allSettled([
    safeFetch(`${ORG_CORE_URL}/api/v1/orgs/me/members`, cookieHeader).then((data) => {
      stats.memberCount = extractCount(data);
    }),
    // sourceCount — quarry has no sources list endpoint; leave null
    Promise.resolve(null).then((data) => {
      stats.sourceCount = extractCount(data);
    }),
    safeFetch(`${DOCUMENTS_URL}/v1/documents`).then((data) => {
      stats.documentCount = extractCount(data);
    }),
    // quarry has no /crawl/jobs list endpoint — crawl status is per job-id only
    Promise.resolve(null).then((data) => {
      if (!data || typeof data !== 'object') {
        return;
      }

      const obj = data as Record<string, unknown>;
      const jobs = Array.isArray(data)
        ? data
        : Array.isArray(obj.data)
          ? obj.data
          : Array.isArray(obj.jobs)
            ? obj.jobs
            : [];

      if (jobs.length === 0) {
        return;
      }

      const job = jobs[0] as Record<string, unknown>;
      stats.crawledPages =
        typeof job.pagesIndexed === 'number'
          ? job.pagesIndexed
          : typeof job.pagesCrawled === 'number'
            ? job.pagesCrawled
            : typeof job.totalPages === 'number'
              ? job.totalPages
              : null;
      stats.crawlStatus =
        (job.status as DashboardStats['crawlStatus']) ??
        (job.phase as DashboardStats['crawlStatus']) ??
        null;
      stats.lastCrawlAt =
        typeof job.completedAt === 'string'
          ? job.completedAt
          : typeof job.updatedAt === 'string'
            ? job.updatedAt
            : typeof job.createdAt === 'string'
              ? job.createdAt
              : null;
    }),
  ]);

  return stats;
}
