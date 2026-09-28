import { requestJson } from './http'

type SharePointDocument = {
  document_id: string
  source: string
  type: string
  title: string
  document_date?: string
  updated_at?: string
  metadata?: Record<string, unknown>
}

export type AquatiqUpdate = { id: string; title: string; url: string; site: string; updatedAt?: string }
export type AquatiqUpdates = { pages: AquatiqUpdate[]; hasMore: boolean }

function sharePointUrl(value: unknown): URL | null {
  if (typeof value !== 'string') return null
  try {
    const url = new URL(value)
    return url.protocol === 'https:' && url.hostname.endsWith('.sharepoint.com') && !url.username && !url.password ? url : null
  } catch { return null }
}

/** Read indexed pages through Data's viewer-scoped gateway. No Graph tokens,
 * source registration, sync, or browser persistence. The connector does not
 * currently preserve news promotion metadata: do not claim these are a
 * complete feed of published news. Known home/draft pages are excluded. */
export async function loadAquatiqUpdates(orgId: string, signal?: AbortSignal): Promise<AquatiqUpdates> {
  const payload = await requestJson<{ documents: SharePointDocument[] | null; total?: number }>(
    '/api/v1/knowledge/documents?type=sharepoint_page&limit=100&offset=0',
    { headers: { 'X-Org-Id': orgId }, signal },
  )
  // documents-api's Go nil slice serializes as null when the count is zero.
  if (payload.documents === null && payload.total === 0) return { pages: [], hasMore: false }
  if (!Array.isArray(payload.documents)) throw new Error('SharePoint pages response is unavailable')
  const pages = payload.documents.flatMap((document): AquatiqUpdate[] => {
    const metadata = document.metadata ?? {}
    const url = sharePointUrl(metadata.web_url)
    if (document.source !== 'sharepoint' || document.type !== 'sharepoint_page' || !url
      || metadata.page_layout === 'home' || metadata.publishing_level === 'draft') return []
    const site = sharePointUrl(metadata.site_web_url)
    return [{ id: document.document_id, title: document.title || String(metadata.name || 'SharePoint'),
      url: url.href, site: site?.href ?? url.origin, updatedAt: document.document_date || document.updated_at }]
  })
  pages.sort((a, b) => (Date.parse(b.updatedAt ?? '') || 0) - (Date.parse(a.updatedAt ?? '') || 0))
  return { pages, hasMore: (payload.total ?? payload.documents.length) > payload.documents.length }
}
