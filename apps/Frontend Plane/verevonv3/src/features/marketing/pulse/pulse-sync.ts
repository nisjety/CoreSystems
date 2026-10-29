import { createEffect, createSignal, onCleanup, untrack } from 'solid-js'
import { loadAnalyticsSync, requestAnalyticsSync } from '@/shared/api/marketing/analytics-sync-client'
import { loadPublicSignals, requestPublicSignals } from '@/shared/api/marketing/public-signals-client'
import { loadWebsitePreparation, prepareWebsite, type MarketingSource } from '@/shared/api/marketing/preparation-client'
import { queueAlgoliaCollection, loadAlgoliaCollection } from '@/shared/api/marketing/algolia-collection-client'
import { syncSiteSource } from '@/shared/api/marketing/site-sources-client'
import { syncWebVitals } from '@/shared/api/marketing/web-vitals-client'
import type { AnalyticsSyncContext } from '@/shared/api/marketing/analytics-sync-client'

export type PulseSyncResult = { key: string; status: 'running' | 'complete' | 'attention' }
type Work = { key: string; run: () => Promise<boolean>; read?: () => Promise<boolean> }

/** False means still running. A provider issue is recorded per source, never
 * promoted to a successful sync or allowed to cancel unrelated collectors. */
export function createPulseSync(context: () => AnalyticsSyncContext | undefined, sources: () => readonly MarketingSource[], allowed: () => boolean, onSettled: () => void) {
  const [busy, setBusy] = createSignal(false), [results, setResults] = createSignal<PulseSyncResult[]>([])
  let controller: AbortController | undefined
  createEffect(() => {
    const value = context()
    void value?.websiteId; void value?.site; void value?.websiteRevision; void value?.quarter; allowed()
    controller?.abort(); setBusy(false); setResults([])
  })
  onCleanup(() => controller?.abort())
  async function sync() {
    const selected = untrack(context)
    if (!allowed() || busy() || !selected) return
    controller?.abort(); const current = new AbortController(); controller = current
    const signal = current.signal, added = new Set(sources().filter(value => value.status === 'connected').map(value => value.key))
    const analytics = { ...selected }, selectedSteps = new Set(['search_console', 'ga4', 'clarity'].filter(key => added.has(key as MarketingSource['key'])))
    const analyticsReady = (value: Awaited<ReturnType<typeof loadAnalyticsSync>>) => {
      if (value.status === 'running' || value.status === 'queued') return false
      if (value.steps.some(step => selectedSteps.has(step.key === 'gsc' ? 'search_console' : step.key) && step.status !== 'connected' && step.reason !== 'no_data')) throw new Error('provider_attention')
      return true
    }
    const publicReady = (value: Awaited<ReturnType<typeof loadPublicSignals>>) => {
      if (value.status === 'queued' || value.status === 'running') return false
      if (value.status !== 'complete' || !value.report || [...value.report.crux, value.report.commonCrawl].some(item => item.status !== 'connected' && item.reason !== 'no_data')) throw new Error('provider_attention')
      return true
    }
    const preparationReady = (value: Awaited<ReturnType<typeof loadWebsitePreparation>>) => {
      if (value.status === 'queued' || value.status === 'running') return false
      if (value.status !== 'complete') throw new Error('provider_attention')
      return true
    }
    const jobs: Work[] = [
      { key: 'website', run: async () => preparationReady(await prepareWebsite(selected, signal)), read: async () => preparationReady(await loadWebsitePreparation(selected, signal)) },
      { key: 'public', run: async () => publicReady(await requestPublicSignals(selected, signal)), read: async () => publicReady(await loadPublicSignals(selected, signal)) },
    ]
    if (selectedSteps.size) jobs.push({ key: 'analytics', run: async () => analyticsReady(await requestAnalyticsSync(analytics, signal)), read: async () => analyticsReady(await loadAnalyticsSync(analytics, signal)) })
    if (added.has('algolia')) {
      const ready = (value: Awaited<ReturnType<typeof loadAlgoliaCollection>>) => { if (value.status === 'queued' || value.status === 'syncing') return false; if (value.status !== 'ready') throw new Error('provider_attention'); return true }
      jobs.push({ key: 'algolia', run: async () => ready(await queueAlgoliaCollection(selected, signal)), read: async () => ready(await loadAlgoliaCollection(selected, signal)) })
    }
    if (added.has('cookiebot')) jobs.push({ key: 'cookiebot', run: async () => { const report = await syncSiteSource(selected, 'cookiebot', signal); if (report.status !== 'connected') throw new Error('provider_attention'); return true } })
    if (added.has('web_vitals')) jobs.push({ key: 'web_vitals', run: async () => { const report = await syncWebVitals(selected, 'production', signal); if (report.status === 'not_configured') throw new Error('provider_attention'); return true } })
    const update = (key: string, status: PulseSyncResult['status']) => { if (!signal.aborted) setResults(values => values.map(value => value.key === key ? { key, status } : value)) }
    setResults(jobs.map(job => ({ key: job.key, status: 'running' }))); setBusy(true)
    const execute = async (job: Work, read = false) => {
      try { if (await (read ? job.read!() : job.run())) update(job.key, 'complete') }
      catch { update(job.key, 'attention') }
    }
    await Promise.allSettled(jobs.map(job => execute(job)))
    for (let poll = 0; !signal.aborted && poll < 90; poll++) {
      const pending = jobs.filter(job => job.read && results().some(result => result.key === job.key && result.status === 'running'))
      if (!pending.length) break
      await new Promise<void>(resolve => { const finish = () => { clearTimeout(timer); signal.removeEventListener('abort', finish); resolve() }; const timer = setTimeout(finish, 2000); signal.addEventListener('abort', finish, { once: true }); if (signal.aborted) finish() })
      if (!signal.aborted) await Promise.allSettled(pending.map(job => execute(job, true)))
    }
    if (!signal.aborted) { setResults(values => values.map(value => value.status === 'running' ? { ...value, status: 'attention' } : value)); setBusy(false); onSettled() }
  }
  return { busy, results, sync }
}
