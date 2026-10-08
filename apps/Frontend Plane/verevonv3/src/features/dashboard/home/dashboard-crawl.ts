import { createEffect, createSignal } from 'solid-js'
import type { RunItem, ScheduleItem } from '@/shared/api/ingestions-client'
import { getSession } from '@/shared/session/session-store'

/** The Hent ut cards on Home are a glance, not the ingestions page. */
export const crawlCardRows = 4
/** Where both cards' rows and footer links go: the page keeps its view in local state, so there is no per-run link. */
export const ingestionsHref = '/ingestions'

type Tr = (no: string, en: string) => string

/** Loads one ingestions list for the current user and workspace. It is read-only;
 * a response for an earlier user, workspace or retry is dropped and its request aborted. */
export function useCrawlList<T>(load: (signal: AbortSignal) => Promise<T[]>) {
  const session = getSession()
  const [items, setItems] = createSignal<T[]>([])
  const [loading, setLoading] = createSignal(true)
  const [error, setError] = createSignal(false)
  const [refresh, setRefresh] = createSignal(0)
  createEffect(
    () => ({ user: session.user?.id, org: session.activeOrg?.id, refresh: refresh() }),
    ({ user, org }) => {
      let cancelled = false
      const controller = new AbortController()
      setItems([])
      setError(false)
      setLoading(Boolean(user && org))
      if (user && org) void load(controller.signal).then((next) => {
        if (!Array.isArray(next)) throw new Error('Unexpected ingestions response')
        if (!cancelled) setItems(next)
      }).catch(() => { if (!cancelled) setError(true) })
        .finally(() => { if (!cancelled) setLoading(false) })
      return () => { cancelled = true; controller.abort() }
    },
  )
  return { items, loading, error, retry: () => setRefresh((value) => value + 1) }
}

const timeOf = (value: unknown) => {
  const time = typeof value === 'string' ? Date.parse(value) : Number.NaN
  return Number.isNaN(time) ? undefined : time
}

/** Newest first, as the ingestions page's Runs view lists them. */
export function latestCrawlRuns(runs: RunItem[]) {
  return [...runs].sort((a, b) => (timeOf(b.createdAt) ?? 0) - (timeOf(a.createdAt) ?? 0)).slice(0, crawlCardRows)
}

/** Soonest next run first. A schedule with no next run follows, newest first. */
export function upcomingCrawlSchedules(schedules: ScheduleItem[]) {
  return [...schedules].sort((a, b) => {
    const [nextA, nextB] = [timeOf(a.nextRunAt), timeOf(b.nextRunAt)]
    if (nextA !== undefined && nextB !== undefined) return nextA - nextB
    if (nextA !== undefined) return -1
    if (nextB !== undefined) return 1
    return (timeOf(b.createdAt) ?? 0) - (timeOf(a.createdAt) ?? 0)
  }).slice(0, crawlCardRows)
}

/** Same day-and-month label as the recent-work card; blank when the API sent no usable date. */
export function crawlDateLabel(value: string | null | undefined, locale: string) {
  if (!value) return ''
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? '' : date.toLocaleDateString(locale === 'no' ? 'nb-NO' : 'en-GB', { day: 'numeric', month: 'short' })
}

const normalized = (value: unknown) => (typeof value === 'string' ? value.trim().toLowerCase() : '')

/** Kind names as the ingestions page's run and schedule forms spell them. */
export function crawlKindLabel(kind: unknown, tr: Tr) {
  const name = normalized(kind)
  switch (name) {
    case 'scrape': return tr('Scrape', 'Scrape')
    case 'crawl': return tr('Crawl', 'Crawl')
    case 'extract': return tr('Uttrekk', 'Extract')
    case 'batch': return tr('Batch', 'Batch')
    case 'search': return tr('Søk', 'Search')
    case 'agent': return tr('Agent', 'Agent')
    default: return name.charAt(0).toUpperCase() + name.slice(1)
  }
}

/** The status values the ingestions page groups on, in the words it uses for them.
 * Anything else shows as sent, with underscores turned into spaces, as its status badge does. */
export function crawlStatusLabel(status: unknown, tr: Tr) {
  const name = normalized(status)
  switch (name) {
    case 'completed': case 'success': case 'done': return tr('Fullført', 'Completed')
    case 'running': case 'in_progress': case 'syncing': return tr('Kjører', 'Running')
    case 'queued': case 'pending': return tr('I kø', 'Queued')
    case 'active': return tr('Aktiv', 'Active')
    case 'paused': return tr('Pauset', 'Paused')
    case 'failed': case 'error': return tr('Feilet', 'Failed')
    case 'timeout': return tr('Tidsavbrudd', 'Timed out')
    default: return name.replace(/_/g, ' ')
  }
}

/** "Crawl · Fullført", without a dangling separator when the API left a part out. */
export const crawlMetaLine = (...parts: string[]) => parts.filter(Boolean).join(' · ')
