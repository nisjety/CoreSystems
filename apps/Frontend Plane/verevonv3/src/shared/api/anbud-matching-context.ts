import { getMe } from './settings-client'
import { getSession } from '@/shared/session/session-store'
import { loadWebsites } from './marketing/website-client'
import { loadSearch } from './marketing/search-client'

export type TenderMatchingContext = { companyKeywords: string[]; userKeywords: string[] }
const generic = new Set(['ceo', 'founder', 'owner', 'manager', 'director', 'senior', 'junior', 'head', 'of', 'and', 'the', 'leder', 'daglig', 'administrerende'])
export function professionalKeywords(position: string, department: string): string[] {
  return [...new Set(`${department} ${position}`.toLocaleLowerCase().match(/[\p{L}\p{N}]+/gu) ?? [])]
    .filter(term => term.length >= 2 && term.length <= 80 && !generic.has(term)).slice(0, 20)
}
const empty = (): TenderMatchingContext => ({ companyKeywords: [], userKeywords: [] })

/** Reads the active organisation's own primary-site evidence and this user's professional
 * fields through their existing authorized APIs. Nothing is persisted or shared across users.
 * Missing/expired SEO evidence simply contributes no terms; it is never invented.
 */
export async function loadTenderMatchingContext(signal?: AbortSignal): Promise<TenderMatchingContext> {
  const session = getSession()
  const org = session.activeOrg?.id
  const userId = session.user?.id
  if (!org || signal?.aborted) return empty()
  const controller = new AbortController()
  const cancel = () => controller.abort()
  signal?.addEventListener('abort', cancel, { once: true })
  const timeout = setTimeout(cancel, 1800)
  try {
    const [professional, seo] = await Promise.allSettled([
      getMe(controller.signal).then(user => professionalKeywords(user.position, user.department)),
      (async () => {
        const sites = await loadWebsites(controller.signal)
        const own = sites.find(site => site.primary && site.ownership.status !== 'unverified' && site.integrations.searchConsole.status === 'connected')
        if (!own) return []
        const data = await loadSearch('FY', own.site, controller.signal, own.id)
        const age = data.updatedAt ? Date.now() - Date.parse(data.updatedAt) : Infinity
        if (data.status !== 'connected' || age < 0 || age > 30 * 86400000) return []
        return [...data.queries].filter(row => row.impressions > 0)
          .sort((a, b) => b.clicks - a.clicks || b.impressions - a.impressions)
          .map(row => row.key.trim()).filter(term => term.length >= 2 && [...term].length <= 80 && !/(?:@|https?:\/\/|www\.|\b\d{7,}\b)/i.test(term))
          .filter((term, index, terms) => terms.indexOf(term) === index).slice(0, 50)
      })(),
    ])
    if (signal?.aborted || getSession().activeOrg?.id !== org || getSession().user?.id !== userId) return empty()
    return { companyKeywords: seo.status === 'fulfilled' ? seo.value : [], userKeywords: professional.status === 'fulfilled' ? professional.value : [] }
  } finally {
    clearTimeout(timeout)
    signal?.removeEventListener('abort', cancel)
  }
}
