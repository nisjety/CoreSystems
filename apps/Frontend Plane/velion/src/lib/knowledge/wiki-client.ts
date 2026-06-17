import 'server-only'
import {
  wikiPageSchema,
  wikiPageVersionSchema,
  wikiSourceLogSchema,
  type WikiPage,
  type WikiPageVersion,
  type WikiSourceLog,
} from '@/types/data-plane/wiki_v1'

/**
 * Wave 11.C-a — server-side client for `wiki-store-go` (Data Plane v2,
 * compose host `wiki-store:8011`).
 *
 * Endpoints covered:
 *   POST /v1/wiki/pages                                  → CreatePage
 *   GET  /v1/wiki/pages/{pageID}                         → GetPage
 *   GET  /v1/wiki/pages/by-path?org_id=&path=            → GetPageByPath
 *   POST /v1/wiki/pages/{pageID}/versions                → UpdateVersion
 *   GET  /v1/wiki/pages/{pageID}/versions                → ListVersions
 *   GET  /v1/wiki/pages/{pageID}/backlinks               → GetBacklinks
 *   GET  /v1/wiki/pages/{pageID}/source-logs             → ListSourceLogs
 *
 * The service has no "list all pages" endpoint yet, so the wiki sidebar
 * relies on (a) operator-bookmarked paths in localStorage and (b) the
 * by-path lookup. That gap is a Wave 11.C-b follow-up.
 */

const WIKI_SERVICE_URL =
  process.env.WIKI_SERVICE_URL ||
  process.env.WIKI_STORE_URL ||
  'http://wiki-store:8011'

const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET || ''

const TIMEOUT_MS = 15_000

interface BaseInit {
  orgId: string
  userId?: string
}

function headers(base: BaseInit, contentType: 'json' | 'none' = 'json'): HeadersInit {
  const h: Record<string, string> = {
    Accept: 'application/json',
    'X-Internal-Api-Key': INTERNAL_API_KEY,
    'X-Org-ID': base.orgId,
    'X-Org-Id': base.orgId, // wiki-store-go middleware reads this casing
  }
  if (base.userId) h['X-User-Id'] = base.userId
  if (contentType === 'json') h['Content-Type'] = 'application/json'
  return h
}

async function call<T>(
  path: string,
  init: RequestInit,
  base: BaseInit,
  parser?: (raw: unknown) => T,
): Promise<{ ok: true; data: T } | { ok: false; status: number; error: string }> {
  let response: Response
  try {
    response = await fetch(`${WIKI_SERVICE_URL}${path}`, {
      ...init,
      headers: { ...headers(base, init.body ? 'json' : 'none'), ...(init.headers ?? {}) },
      cache: 'no-store',
      signal: AbortSignal.timeout(TIMEOUT_MS),
    })
  } catch (error) {
    return {
      ok: false,
      status: 503,
      error: error instanceof Error ? error.message : 'Network error',
    }
  }
  if (!response.ok) {
    const detail = await response.text().catch(() => '')
    return { ok: false, status: response.status, error: detail || response.statusText }
  }
  const raw = (await response.json().catch(() => null)) as unknown
  const data = parser ? parser(raw) : (raw as T)
  return { ok: true, data }
}

// ─── reads ────────────────────────────────────────────────────────────────

export async function getWikiPage(
  pageId: string,
  base: BaseInit,
): Promise<WikiPage | null> {
  const result = await call<WikiPage | null>(
    `/v1/wiki/pages/${encodeURIComponent(pageId)}?org_id=${encodeURIComponent(base.orgId)}`,
    { method: 'GET' },
    base,
    (raw) => {
      if (!raw || typeof raw !== 'object') return null
      const candidate = (raw as { page?: unknown }).page ?? raw
      const parsed = wikiPageSchema.safeParse(candidate)
      return parsed.success ? parsed.data : null
    },
  )
  return result.ok ? result.data : null
}

export async function getWikiPageByPath(
  path: string,
  base: BaseInit,
): Promise<WikiPage | null> {
  const result = await call<WikiPage | null>(
    `/v1/wiki/pages/by-path?org_id=${encodeURIComponent(base.orgId)}&path=${encodeURIComponent(path)}`,
    { method: 'GET' },
    base,
    (raw) => {
      if (!raw || typeof raw !== 'object') return null
      const candidate = (raw as { page?: unknown }).page ?? raw
      const parsed = wikiPageSchema.safeParse(candidate)
      return parsed.success ? parsed.data : null
    },
  )
  return result.ok ? result.data : null
}

export async function getCurrentVersion(
  pageId: string,
  base: BaseInit,
): Promise<WikiPageVersion | null> {
  const result = await call<WikiPageVersion | null>(
    `/v1/wiki/pages/${encodeURIComponent(pageId)}?org_id=${encodeURIComponent(base.orgId)}`,
    { method: 'GET' },
    base,
    (raw) => {
      if (!raw || typeof raw !== 'object') return null
      const candidate = (raw as { version?: unknown }).version
      if (!candidate) return null
      const parsed = wikiPageVersionSchema.safeParse(candidate)
      return parsed.success ? parsed.data : null
    },
  )
  return result.ok ? result.data : null
}

export async function listPageVersions(
  pageId: string,
  base: BaseInit,
): Promise<WikiPageVersion[]> {
  const result = await call<WikiPageVersion[]>(
    `/v1/wiki/pages/${encodeURIComponent(pageId)}/versions?org_id=${encodeURIComponent(base.orgId)}`,
    { method: 'GET' },
    base,
    (raw) => {
      const list = (raw as { versions?: unknown[] } | null)?.versions ?? []
      return list
        .map((v) => wikiPageVersionSchema.safeParse(v))
        .filter(
          (p): p is { success: true; data: WikiPageVersion } => p.success,
        )
        .map((p) => p.data)
    },
  )
  return result.ok ? result.data : []
}

export async function getBacklinks(
  pageId: string,
  base: BaseInit,
): Promise<Array<{ page_id: string; title: string; path: string }>> {
  const result = await call<Array<{ page_id: string; title: string; path: string }>>(
    `/v1/wiki/pages/${encodeURIComponent(pageId)}/backlinks?org_id=${encodeURIComponent(base.orgId)}`,
    { method: 'GET' },
    base,
    (raw) => {
      const list =
        (raw as { backlinks?: Array<Record<string, unknown>> } | null)?.backlinks ??
        ((raw as { items?: Array<Record<string, unknown>> } | null)?.items ?? [])
      return list
        .map((row) => ({
          page_id: typeof row.page_id === 'string' ? row.page_id : '',
          title: typeof row.title === 'string' ? row.title : '',
          path: typeof row.path === 'string' ? row.path : '',
        }))
        .filter((r) => r.page_id !== '')
    },
  )
  return result.ok ? result.data : []
}

export async function listSourceLogs(
  pageId: string,
  base: BaseInit,
): Promise<WikiSourceLog | null> {
  const result = await call<WikiSourceLog | null>(
    `/v1/wiki/pages/${encodeURIComponent(pageId)}/source-logs?org_id=${encodeURIComponent(base.orgId)}`,
    { method: 'GET' },
    base,
    (raw) => {
      const list = (raw as { logs?: unknown[] } | null)?.logs ?? []
      const latest = list[0]
      if (!latest) return null
      const parsed = wikiSourceLogSchema.safeParse(latest)
      return parsed.success ? parsed.data : null
    },
  )
  return result.ok ? result.data : null
}

// ─── writes ───────────────────────────────────────────────────────────────

export async function createPage(
  body: {
    title: string
    path: string
    initial_content: string
    workspace_id?: string
  },
  base: BaseInit,
): Promise<{ ok: true; data: { page: WikiPage; version: WikiPageVersion } } | { ok: false; error: string; status: number }> {
  return call<{ page: WikiPage; version: WikiPageVersion }>(
    '/v1/wiki/pages',
    {
      method: 'POST',
      body: JSON.stringify({
        org_id: base.orgId,
        workspace_id: body.workspace_id ?? 'default',
        title: body.title,
        path: body.path,
        initial_content: body.initial_content,
      }),
    },
    base,
    (raw) => raw as { page: WikiPage; version: WikiPageVersion },
  )
}

export async function updateVersion(
  pageId: string,
  body: { content: string; edit_reason: string; user_id?: string },
  base: BaseInit,
): Promise<{ ok: true; data: { version: WikiPageVersion } } | { ok: false; error: string; status: number }> {
  return call<{ version: WikiPageVersion }>(
    `/v1/wiki/pages/${encodeURIComponent(pageId)}/versions`,
    {
      method: 'POST',
      body: JSON.stringify({
        org_id: base.orgId,
        new_content: body.content,
        edit_reason: body.edit_reason,
        proposed_by_user: body.user_id ?? base.userId,
      }),
    },
    base,
    (raw) => raw as { version: WikiPageVersion },
  )
}
