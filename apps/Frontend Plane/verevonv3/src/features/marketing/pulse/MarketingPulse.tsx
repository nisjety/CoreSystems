import { VisibilitySummary } from '@/features/visibility/components/VisibilitySummary'
import {
  BarChart3,
  CalendarDays,
  ChevronDown,
  Gauge,
  LayoutGrid,
  Presentation,
  RefreshCw,
} from 'lucide-solid'
import { For, Show, createEffect, createMemo, createSignal, on, onCleanup, untrack } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { buildPulseChart, loadPulse, type PulseChartMetric, type PulseQuarter, type PulseSnapshot } from '../../../shared/api/marketing/pulse-client.ts'
import { loadWebsites, readWebsitePreference, saveWebsitePreference, type Website } from '../../../shared/api/marketing/website-client.ts'
import { WebsiteManager } from './WebsiteManager.tsx'
import { buildPulseTimeline, formatPulseDate, pulsePeriodDates } from '../../../shared/api/marketing/pulse-timeline.ts'
import { SearchDashboard } from './SearchDashboard.tsx'
import { ProviderOverview } from './ProviderOverview'
import type { GA4Snapshot } from '@/shared/api/marketing/ga4-client'
import type { ClaritySnapshot } from '@/shared/api/marketing/clarity-client'
import type { PageSpeedSnapshot } from '@/shared/api/marketing/pagespeed-client'
import { ReportDisclosure } from './ReportDisclosure'
import './pulse-dashboard.css'
import { MarketingWorkQueue } from './MarketingWorkQueue.tsx'
import type { MarketingWorkSnapshot } from '../../../shared/api/marketing/marketing-work-client.ts'
import { PageSpeedPanel } from './PageSpeedPanel.tsx'
import { PublicSignalsPanel } from './PublicSignalsPanel'
import { WebVitalsPanel } from './WebVitalsPanel'
import { AlgoliaPanel, CookiebotPanel } from './SiteSourcesPanel'
import { WebsiteDiscoveryPanel } from './WebsiteDiscoveryPanel.tsx'
import { SiteAuditPanel } from './SiteAuditPanel'
import { AuditMonitoringPanel } from './AuditMonitoringPanel'
import { AuditInsightsPanel } from './AuditInsightsPanel'
import { WeeklyReportPanel } from './WeeklyReportPanel'
import { GA4Panel } from './GA4Panel.tsx'
import { ClarityPanel } from './ClarityPanel.tsx'
import { AnalyticsSyncPanel } from './AnalyticsSyncPanel'
import { CollectionInventoryPanel } from '@/features/visibility/components/CollectionInventoryPanel'
import { loadSourceCatalog, type SourceCatalog } from '@/shared/api/marketing/preparation-client'
import { ConnectedServices, isAddedSource, pulseServiceNames, type PulseServiceKey } from './ConnectedServices'
import { ProviderSetup } from './ProviderSetup'
import { createPulseSync } from './pulse-sync'
import { syncSiteSource } from '@/shared/api/marketing/site-sources-client'
import { syncWebVitals } from '@/shared/api/marketing/web-vitals-client'
import { requestAnalyticsSync } from '@/shared/api/marketing/analytics-sync-client'
import './marketing-pulse.css'

const QUARTERS = ['Q1', 'Q2', 'Q3', 'Q4'] as const
const PERIODS = [...QUARTERS, 'FY'] as const
type WorkspaceView = 'calendar' | 'board'
type RangeHandle = 'start' | 'end'

function currentQuarter(): typeof QUARTERS[number] {
  return QUARTERS[Math.floor(new Date().getUTCMonth() / 3)] ?? 'Q1'
}

function hasCompletedDayThisYear() {
  const now = new Date()
  return now.getUTCMonth() > 0 || now.getUTCDate() > 1
}

/**
 * Both surfaces store only a selected website ID in browser storage and render
 * normalized projections. Credentials and organization authority stay server-side.
 * This component never receives OAuth credentials,
 * connection IDs, or raw provider responses.
 */
export function MarketingPulse(props: { canManageIntegrations?: boolean; canConfigureMonitoring?: boolean } = {}) {
  const i18n = useI18n()
  const [quarter, setQuarter] = createSignal<PulseQuarter>(currentQuarter())
  const [metric, setMetric] = createSignal<PulseChartMetric>('clicks')
  const [workspaceView, setWorkspaceView] = createSignal<WorkspaceView>('board')
  const [snapshot, setSnapshot] = createSignal<PulseSnapshot | null>(null)
  const [requestFailed, setRequestFailed] = createSignal(false)
  const [websites, setWebsites] = createSignal<Website[]>([])
  const [selectedId, setSelectedId] = createSignal(readWebsitePreference())
  const [registryFailed, setRegistryFailed] = createSignal(false)
  const [registryLoading, setRegistryLoading] = createSignal(true)
  const [registryRefresh, setRegistryRefresh] = createSignal(0)
  const [workRefresh, setWorkRefresh] = createSignal(0)
  const [auditRefresh, setAuditRefresh] = createSignal(0)
  const [insightsRefresh, setInsightsRefresh] = createSignal(0)
  const [analyticsRefresh, setAnalyticsRefresh] = createSignal(0)
  const [rangeStartIndex, setRangeStartIndex] = createSignal<number | null>(null)
  const [rangeEndIndex, setRangeEndIndex] = createSignal<number | null>(null)
  const [workSnapshot, setWorkSnapshot] = createSignal<MarketingWorkSnapshot | null>(null)
  const [catalog, setCatalog] = createSignal<SourceCatalog | null>(null)
  const [catalogLoading, setCatalogLoading] = createSignal(true), [catalogError, setCatalogError] = createSignal(false)
  const [catalogRefresh, setCatalogRefresh] = createSignal(0)
  const [selectedService, setSelectedService] = createSignal<PulseServiceKey>('search_console')
  const [ga4Overview, setGA4Overview] = createSignal<GA4Snapshot | null>(null)
  const [clarityOverview, setClarityOverview] = createSignal<ClaritySnapshot | null>(null)
  const [speedOverview, setSpeedOverview] = createSignal<PageSpeedSnapshot | null>(null)
  const [sitePickerOpen, setSitePickerOpen] = createSignal(false)
  let registryVersion = 0
  const selectedWebsite = createMemo(() => websites().find((website) => website.id === selectedId()))
  const activeSite = () => selectedWebsite()?.site
  const activeHost = () => activeSite() ? new URL(activeSite()!).hostname : i18n.tr('et nettsted', 'a website')
  const canManage = () => props.canManageIntegrations ?? false
  createEffect(on(() => `${selectedWebsite()?.id}|${selectedWebsite()?.revision}|${selectedService()}`, () => {
    setGA4Overview(null); setClarityOverview(null); setSpeedOverview(null)
  }))
  let extensionController: AbortController | undefined
  createEffect(on(() => `${selectedWebsite()?.id}|${selectedWebsite()?.site}|${selectedWebsite()?.revision}|${canManage()}`, () => extensionController?.abort()))
  onCleanup(() => extensionController?.abort())
  async function checkProviderConnection(provider: PulseServiceKey): Promise<'ready' | 'pending' | 'error'> {
    const website = selectedWebsite()
    if (!website || !canManage()) return 'pending'
    extensionController?.abort(); const current = new AbortController(); extensionController = current
    const context = { websiteId: website.id, site: website.site, websiteRevision: website.revision }
    const valid = () => !current.signal.aborted && selectedWebsite()?.id === website.id && selectedWebsite()?.revision === website.revision && canManage()
    try {
      const value = await loadSourceCatalog(context, current.signal)
      if (!valid()) return 'pending'
      setCatalog(value); setCatalogError(false)
      if (value.items.find(item => item.key === provider)?.status !== 'connected') return 'pending'
      if (provider === 'cookiebot') {
        const report = await syncSiteSource(context, provider, current.signal)
        if (!['connected','partial'].includes(report.status)) return 'error'
      } else if (provider === 'web_vitals') {
        const report = await syncWebVitals(context, 'production', current.signal)
        if (report.status === 'not_configured') return 'pending'
      } else if (provider === 'clarity') await requestAnalyticsSync({ ...context, quarter: quarter() }, current.signal)
      if (!valid()) return 'pending'
      setAnalyticsRefresh(count => count + 1)
      return 'ready'
    } catch { return current.signal.aborted ? 'pending' : 'error' }
  }
  const refreshReports = () => { setAnalyticsRefresh(value => value + 1); setAuditRefresh(value => value + 1); setCatalogRefresh(value => value + 1) }
  const sync = createPulseSync(() => { const website = selectedWebsite(); return website ? { websiteId: website.id, site: website.site, websiteRevision: website.revision, quarter: quarter() } : undefined }, () => catalog()?.items ?? [], canManage, refreshReports)
  createEffect(() => {
    catalogRefresh()
    const website = selectedWebsite(), controller = new AbortController()
    setCatalogLoading(true); setCatalogError(false); setCatalog(null)
    if (!website) { setCatalogLoading(false); return }
    void loadSourceCatalog({ websiteId: website.id, site: website.site, websiteRevision: website.revision }, controller.signal).then(value => {
      if (!controller.signal.aborted) setCatalog(value)
    }).catch(() => { if (!controller.signal.aborted) setCatalogError(true) }).finally(() => { if (!controller.signal.aborted) setCatalogLoading(false) })
    onCleanup(() => controller.abort())
  })

  // Account discovery can settle after onboarding has opened the workspace.
  // Refresh registry metadata for a bounded handoff window and on window focus;
  // unchanged records do not remount reports or interrupt an active Sync.
  createEffect(() => {
    const controller = new AbortController()
    let until = Date.now() + 180_000, interval: ReturnType<typeof setInterval> | undefined
    let pending = false
    const read = async () => {
      if (pending || registryLoading() || controller.signal.aborted) return
      pending = true; const version = registryVersion
      try {
        const values = await loadWebsites(controller.signal)
        if (controller.signal.aborted || version !== registryVersion) return
        if (JSON.stringify(values) !== JSON.stringify(websites())) {
          registryVersion++
          setWebsites(values)
          if (!values.some(value => value.id === selectedId())) {
            setSelectedId(values[0]?.id)
            if (values[0]) saveWebsitePreference(values[0].id)
          }
        }
      } catch { /* The next bounded refresh or foreground visit can retry. */ }
      finally { pending = false }
    }
    const start = () => { interval = setInterval(() => { if (Date.now() >= until) { clearInterval(interval); interval = undefined } else void read() }, 15_000) }
    start()
    const foreground = () => { until = Date.now() + 180_000; if (interval === undefined) start(); void read() }
    window.addEventListener('focus', foreground)
    onCleanup(() => { controller.abort(); clearInterval(interval); window.removeEventListener('focus', foreground) })
  })

  createEffect(() => {
    registryRefresh()
    const preferredId = untrack(selectedId)
    const controller = new AbortController()
    setRegistryLoading(true); setRegistryFailed(false)
    void loadWebsites(controller.signal).then((values) => {
      if (controller.signal.aborted) return
      registryVersion++
      setWebsites(values)
      const id = values.find((website) => website.id === preferredId)?.id ?? values[0]?.id
      setSelectedId(id)
      if (id) saveWebsitePreference(id)
    }).catch(() => { if (!controller.signal.aborted) { setRegistryFailed(true); setWebsites([]) } })
      .finally(() => { if (!controller.signal.aborted) setRegistryLoading(false) })
    onCleanup(() => controller.abort())
  })

  createEffect(() => {
    analyticsRefresh()
    const selectedQuarter = quarter()
    const selectedSite = activeSite()
    const website = selectedWebsite()
    const controller = new AbortController()
    setSnapshot(null)
    setRequestFailed(false)
    if (!selectedSite || !website) return
    void website.revision
    void loadPulse(selectedQuarter, selectedSite, controller.signal, website.id)
      .then((data) => {
        if (!controller.signal.aborted && data.site === selectedSite) setSnapshot(data)
      })
      .catch(() => {
        if (controller.signal.aborted) return
        setRequestFailed(true)
      })
    onCleanup(() => controller.abort())
  })

  const connected = createMemo(() => snapshot()?.status === 'connected')
  const sourceNotConfigured = createMemo(() => snapshot()?.reason === 'not_configured')
  const propertyUnverified = createMemo(() => snapshot()?.reason === 'property_unverified')
  const sourceOutage = createMemo(() => snapshot()?.reason === 'source_unavailable')
  const snapshotPending = createMemo(() => snapshot()?.reason === 'snapshot_pending' || snapshot()?.reason === 'snapshot_expired')
  const authorizationRequired = createMemo(() => snapshot()?.reason === 'authorization_required')
  const bars = createMemo(() => {
    const current = snapshot()
    return current?.status === 'connected' ? buildPulseChart(current.period, current.days, metric()) : []
  })
  const periodDates = createMemo(() => {
    const period = snapshot()?.period
    return period ? pulsePeriodDates(period) : []
  })
  createEffect(() => {
    const dates = periodDates()
    setRangeStartIndex(dates.length ? 0 : null)
    setRangeEndIndex(dates.length ? dates.length - 1 : null)
  })
  const chartSelection = createMemo(() => {
    const dates = periodDates()
    if (!dates.length) return null
    const last = dates.length - 1
    const start = Math.max(0, Math.min(rangeStartIndex() ?? 0, last))
    const end = Math.max(start, Math.min(rangeEndIndex() ?? last, last))
    return {
      startDate: dates[start]!,
      endDate: dates[end]!,
      startIndex: start,
      endIndex: end,
      left: start / dates.length * 100,
      width: (end - start + 1) / dates.length * 100,
    }
  })
  const totalFor = (selectedMetric: PulseChartMetric) => {
    const current = snapshot()
    const selection = chartSelection()
    if (!connected() || !current || !selection) return null
    const isFullPeriod = selection.startIndex === 0 && selection.endIndex === periodDates().length - 1
    if (isFullPeriod) return current.totals?.[selectedMetric] ?? null
    const observed = current.days.filter((day) => day.date >= selection.startDate && day.date <= selection.endDate)
    return observed.length ? observed.reduce((total, day) => total + day[selectedMetric], 0) : null
  }
  const value = createMemo(() => totalFor(metric()))
  const timeline = createMemo(() => {
    const current = snapshot()
    return current ? buildPulseTimeline(current.period, current.days.map((day) => day.date)) : null
  })
  const lastAvailableInSelection = createMemo(() => {
    const selection = chartSelection()
    if (!selection) return null
    return snapshot()?.days
      .filter((day) => day.date >= selection.startDate && day.date <= selection.endDate)
      .map((day) => day.date)
      .sort()
      .at(-1) ?? null
  })
  const dateLabel = (date: string, includeYear = false) => formatPulseDate(date, localeDateTime(i18n.locale()), includeYear)
  const periodDateRange = createMemo(() => {
    const period = snapshot()?.period
    return period && timeline() ? `${dateLabel(period.startDate, true)} – ${dateLabel(period.endDate, true)}` : '—'
  })
  const dateRange = createMemo(() => {
    const selection = chartSelection()
    return selection ? `${dateLabel(selection.startDate, true)} – ${dateLabel(selection.endDate, true)}` : periodDateRange()
  })
  const display = (count: number | null | undefined) => count == null ? '—' : new Intl.NumberFormat('nb-NO').format(count)
  const selectionDateRange = createMemo(() => {
    const selection = chartSelection()
    return selection ? `${dateLabel(selection.startDate, true)} – ${dateLabel(selection.endDate, true)}` : periodDateRange()
  })
  const chartAccessibleLabel = createMemo(() => {
    const label = metric() === 'clicks' ? i18n.tr('Daglige søkesignaler', 'Daily search signals') : i18n.tr('Daglige søkevisninger', 'Daily search impressions')
    return `${label} · ${selectionDateRange()} · ${display(value())}`
  })
  let timelineTrack: HTMLDivElement | undefined
  let draggingRangeHandle: RangeHandle | null = null
  const selectionIncludes = (date: string) => {
    const selection = chartSelection()
    return Boolean(selection && date >= selection.startDate && date <= selection.endDate)
  }
  const rangeHandlePosition = (selection: NonNullable<ReturnType<typeof chartSelection>>, handle: RangeHandle) => {
    const position = handle === 'start' ? selection.left : selection.left + selection.width
    if (selection.startIndex !== selection.endIndex) return `${position}%`
    return `calc(${position}% ${handle === 'start' ? '- 8px' : '+ 8px'})`
  }
  const rangeIndexFromClientX = (clientX: number) => {
    const length = periodDates().length
    const bounds = timelineTrack?.getBoundingClientRect()
    if (!bounds || bounds.width <= 0 || length === 0) return null
    const ratio = Math.max(0, Math.min(1, (clientX - bounds.left) / bounds.width))
    return Math.round(ratio * (length - 1))
  }
  const updateRange = (handle: RangeHandle, nextIndex: number) => {
    const selection = chartSelection()
    if (!selection) return
    if (handle === 'start') setRangeStartIndex(Math.max(0, Math.min(nextIndex, selection.endIndex)))
    else setRangeEndIndex(Math.min(periodDates().length - 1, Math.max(nextIndex, selection.startIndex)))
  }
  const startRangeDrag = (event: PointerEvent, handle: RangeHandle) => {
    event.preventDefault()
    draggingRangeHandle = handle
    ;(event.currentTarget as HTMLButtonElement).setPointerCapture?.(event.pointerId)
    const index = rangeIndexFromClientX(event.clientX)
    if (index !== null) updateRange(handle, index)
  }
  const continueRangeDrag = (event: PointerEvent, handle: RangeHandle) => {
    if (draggingRangeHandle !== handle) return
    const index = rangeIndexFromClientX(event.clientX)
    if (index !== null) updateRange(handle, index)
  }
  const finishRangeDrag = (event: PointerEvent, handle: RangeHandle) => {
    if (draggingRangeHandle !== handle) return
    draggingRangeHandle = null
    const target = event.currentTarget as HTMLButtonElement
    if (target.hasPointerCapture?.(event.pointerId)) target.releasePointerCapture(event.pointerId)
  }
  const moveRangeWithKeyboard = (event: KeyboardEvent, handle: RangeHandle) => {
    const selection = chartSelection()
    if (!selection) return
    const last = periodDates().length - 1
    const current = handle === 'start' ? selection.startIndex : selection.endIndex
    const next = event.key === 'ArrowLeft' ? current - 1
      : event.key === 'ArrowRight' ? current + 1
        : event.key === 'Home' ? 0
          : event.key === 'End' ? last : null
    if (next === null) return
    event.preventDefault()
    updateRange(handle, next)
  }
  function chooseSite(id: string) {
    if (!websites().some((website) => website.id === id)) return
    saveWebsitePreference(id)
    setSelectedId(id)
  }
  function updateWebsite(website: Website) {
    registryVersion++
    setWebsites((values) => values.some((current) => current.id === website.id) ? values.map((current) => current.id === website.id ? website : current) : [...values, website])
    chooseSite(website.id)
  }
  function sourceMessage() {
    if (registryFailed()) return i18n.tr('Nettstedsregisteret er utilgjengelig.', 'Website registry is unavailable.')
    if (!selectedWebsite()) return registryLoading() ? i18n.tr('Laster nettsteder…', 'Loading websites…') : i18n.tr('Legg til et nettsted for å starte.', 'Add a website to get started.')
    if (snapshotPending()) return i18n.tr('Lagret søkerapport mangler eller er utløpt. Bruk Synkroniser for å oppdatere.', 'The saved search snapshot is missing or expired. Use Sync to update.')
    if (authorizationRequired()) return i18n.tr('Search Console-tilgangen må kontrolleres i Integration Core.', 'Search Console authorization needs attention in Integration Core.')
    if (sourceNotConfigured()) return i18n.tr('Google Search Console er ikke koblet til ennå.', 'Google Search Console is not connected yet.')
    if (propertyUnverified()) return i18n.tr('Nettstedet er ikke verifisert i Search Console-kontoen.', 'This site is not verified in the Search Console account.')
    if (requestFailed()) return i18n.tr('Markedsføringsdata er utilgjengelig for dette arbeidsområdet.', 'Marketing data is unavailable for this workspace.')
    if (sourceOutage()) return i18n.tr('Kilden er midlertidig utilgjengelig.', 'Source temporarily unavailable.')
    if (connected() && snapshot()?.propertyScope === 'domain') return i18n.tr('Data for hele domenet fra Google Search Console', 'Domain-wide data from Google Search Console')
    if (connected()) return i18n.tr('Data fra Google Search Console', 'Data from Google Search Console')
    return i18n.tr('Sjekker kildestatus…', 'Checking source status…')
  }
  function chartMessage() {
    if (registryFailed()) return i18n.tr('Nettstedsregisteret kunne ikke lastes.', 'Website registry could not be loaded.')
    if (!selectedWebsite()) return registryLoading() ? i18n.tr('Laster nettsteder…', 'Loading websites…') : i18n.tr('Legg til et nettsted for å hente data.', 'Add a website to load data.')
    if (snapshotPending()) return i18n.tr('Venter på et ferskt lagret søkerapport.', 'Waiting for a fresh saved search snapshot.')
    if (authorizationRequired()) return i18n.tr('Lagrede søkedata skjules til tilgangen er kontrollert.', 'Saved search data is hidden until access is checked.')
    if (sourceNotConfigured()) return i18n.tr('Koble til Google Search Console for å hente ekte søkedata.', 'Connect Google Search Console to load real search data.')
    if (propertyUnverified()) return i18n.tr('Verifiser dette nettstedet i Search Console for å hente data.', 'Verify this site in Search Console to load data.')
    if (requestFailed()) return i18n.tr('Ingen tilkobling til markedsføringsdata.', 'No connection to marketing data.')
    if (sourceOutage()) return i18n.tr('Søkesignaler kunne ikke lastes akkurat nå.', 'Search signals could not be loaded right now.')
    if (connected()) return i18n.tr('Ingen registrerte signaler i denne perioden.', 'No recorded signals in this period.')
    return i18n.tr('Laster søkesignaler…', 'Loading search signals…')
  }
  function metricSourceMessage() {
    if (connected()) return 'Google Search Console'
    if (snapshotPending()) return i18n.tr('Venter på innhenting', 'Awaiting collection')
    if (authorizationRequired()) return i18n.tr('Tilgang må kontrolleres', 'Authorization needs attention')
    if (sourceNotConfigured()) return i18n.tr('Google Search Console er ikke koblet til', 'Google Search Console is not connected')
    if (propertyUnverified()) return i18n.tr('Nettstedet er ikke verifisert', 'Site is not verified')
    if (requestFailed()) return i18n.tr('Datatilgang utilgjengelig', 'Data access unavailable')
    if (sourceOutage()) return i18n.tr('Kilden er utilgjengelig', 'Source unavailable')
    return i18n.tr('Sjekker kildestatus', 'Checking source status')
  }
  const serviceVisible = (key: PulseServiceKey) => selectedService() === key && (key === 'website' || catalog()?.items.some(source => source.key === key && isAddedSource(source.status)))
  let activeReport: HTMLDivElement | undefined
  const inspectSyncResult = (key: string) => {
    const provider: PulseServiceKey = key === 'analytics' ? (['search_console','ga4','clarity'] as const).find(name => catalog()?.items.some(source => source.key === name && isAddedSource(source.status))) ?? 'website' : key === 'cookiebot' || key === 'algolia' || key === 'web_vitals' ? key : 'website'
    setSelectedService(provider)
    queueMicrotask(() => {
      const target = key === 'public' ? activeReport?.querySelector('[data-testid="public-signals-panel"]') : activeReport?.firstElementChild
      if (target?.closest('details')) target.closest('details')!.open = true
      target?.scrollIntoView({ block: 'start' })
    })
  }

  return (
    <div class="marketing-pulse" data-testid="marketing-pulse">
      <header class="marketing-pulse__masthead">
        <div>
          <h1>{i18n.tr('Synlighet', 'Visibility')}</h1>
          <p>{i18n.tr('SEO og søkemarkedsføring for', 'SEO and search marketing for')} <strong>{activeHost()}</strong>.</p>
        </div>
        <div class="marketing-pulse__masthead-actions" aria-label={i18n.tr('Synlighet-kontroller', 'Visibility controls')}>
          <div class="marketing-pulse__masthead-actions-row">
            <Show when={selectedService() === 'search_console' || selectedService() === 'ga4'}><div class="marketing-pulse__quarter-switch" aria-label={i18n.tr('Periodevelger', 'Period selector')}>
              <For each={PERIODS}>{(item, index) => <button type="button" disabled={item === 'FY' ? !hasCompletedDayThisYear() : index() > QUARTERS.indexOf(currentQuarter())} classList={{ 'is-active': quarter() === item, 'is-full-year': item === 'FY' }} aria-pressed={quarter() === item} onClick={() => setQuarter(item)}>{item === 'FY' ? i18n.tr('Hele året', 'Full year') : item}</button>}</For>
            </div></Show>
            <Show when={selectedService() === 'search_console'}><label class="marketing-pulse__metric-select">
              <select aria-label={i18n.tr('Diagrammåling', 'Chart metric')} value={metric()} onChange={(event) => setMetric(event.currentTarget.value as PulseChartMetric)}>
                <option value="clicks">{i18n.tr('Klikk', 'Clicks')}</option>
                <option value="impressions">{i18n.tr('Visninger', 'Impressions')}</option>
              </select>
              <ChevronDown size={13} />
            </label></Show>
            <button type="button" class="marketing-pulse__sync-button" title={i18n.tr('Hent tilkoblede kilder og aktiver bakgrunnsoppdatering', 'Fetch connected sources and enable background updates')} disabled={!selectedWebsite() || catalogLoading() || catalogError() || sync.busy() || !canManage()} onClick={() => { void sync.sync() }} aria-busy={sync.busy()}><RefreshCw size={13} classList={{ 'is-spinning': sync.busy() }}/>{sync.busy() ? i18n.tr('Synkroniserer…', 'Syncing…') : i18n.tr('Synkroniser', 'Sync')}</button>
            <button type="button" aria-expanded={sitePickerOpen()} aria-controls="marketing-pulse-sites" onClick={() => setSitePickerOpen((open) => !open)}>
              {i18n.tr('Nettsider', 'Websites')}
            </button>
          </div>
          <p id="marketing-pulse-connect-note" class="marketing-pulse__connection-note">
            {selectedService() === 'search_console' ? sourceMessage() : `${pulseServiceNames[selectedService()]} · ${i18n.tr('Valgt datakilde', 'Selected data source')}`}
            <Show when={selectedService() === 'search_console' && snapshot()?.reportingProperty}>{(property)=><span> · {property()}</span>}</Show>
          </p>
          <Show when={sync.results().length}><details class="marketing-pulse__sync-receipt"><summary aria-live="polite">{sync.busy() ? i18n.tr('Henter tilkoblede kilder…', 'Collecting connected sources…') : sync.results().some(result => result.status === 'attention') ? i18n.tr('Synkronisering ferdig · noen kontroller trenger oppfølging', 'Sync finished · some checks need attention') : i18n.tr('Synkronisering fullført', 'Sync complete')}</summary><ul><For each={sync.results()}>{result => <li><span>{{ website: 'Quarry / PageSpeed', public: 'CrUX / Common Crawl', analytics: 'Google / Clarity', algolia: 'Algolia', cookiebot: 'Cookiebot', web_vitals: 'Web Vitals' }[result.key]}</span><Show when={result.status === 'attention'} fallback={<span>{result.status === 'complete' ? i18n.tr('Oppdatert', 'Updated') : i18n.tr('Henter…', 'Collecting…')}</span>}><button type="button" onClick={() => inspectSyncResult(result.key)}>{i18n.tr('Se kildens status', 'Check source status')}</button></Show></li>}</For></ul></details></Show>
          <Show when={registryFailed()}><button type="button" onClick={() => setRegistryRefresh((count) => count + 1)}>{i18n.tr('Last nettsteder på nytt', 'Reload websites')}</button></Show>
          <Show when={sitePickerOpen()}>
            <WebsiteManager mode="websites" canManageIntegrations={props.canManageIntegrations ?? true} websites={websites()} selected={selectedWebsite()} onSelect={chooseSite} onUpdated={updateWebsite} onClose={() => setSitePickerOpen(false)} />
          </Show>
        </div>
      </header>

      <Show when={selectedWebsite()}>{website => <ConnectedServices sources={catalog()?.items ?? []} loading={catalogLoading()} error={catalogError()} canManage={canManage()} selected={selectedService()} onSelect={setSelectedService} onOpenReport={() => document.getElementById('pulse-selected-signals')?.scrollIntoView({ block: 'start' })} onRefresh={() => setCatalogRefresh(value => value + 1)} renderSetup={provider => <ProviderSetup provider={provider} website={website()} canManage={canManage()} onUpdated={updateWebsite} onCheck={() => checkProviderConnection(provider)}/>}/>}</Show>
      <div id="pulse-selected-signals" data-provider={selectedService()} class="marketing-pulse__selected-signals" aria-live="polite">
      <Show when={selectedService() === 'search_console'}>
      <section class="marketing-pulse__workspace" aria-label={i18n.tr('Søkesynlighet', 'Search visibility')}>
        <div class="marketing-pulse__signal-surface">
          <div class="marketing-pulse__signal-heading">
            <div>
              <strong data-testid="pulse-primary-value">{display(value())}</strong>
              <p>{metric() === 'clicks' ? i18n.tr('Organiske klikk', 'Organic clicks') : i18n.tr('Søkevisninger', 'Search impressions')}
                <span> · {quarter() === 'FY' ? new Date().getUTCFullYear() : `${quarter()} ${new Date().getUTCFullYear()}`}</span>
              </p>
              <p class="marketing-pulse__date-range" data-testid="pulse-date-range">{dateRange()}</p>
            </div>
            <Show when={connected()}><span class="marketing-pulse__state-pill">{i18n.tr('Google Search Console', 'Google Search Console')}</span></Show>
          </div>
          <div class="marketing-pulse__chart" role="img" data-testid="pulse-activity-chart" data-start-date={snapshot()?.period.startDate} data-end-date={snapshot()?.period.endDate} data-selection-start-date={chartSelection()?.startDate} data-selection-end-date={chartSelection()?.endDate} aria-label={connected() ? chartAccessibleLabel() : `${i18n.tr('Tom signaltidslinje', 'Empty signal timeline')} · ${dateRange()}`}>
            <Show when={bars().some((bar) => bar.hasValue)} fallback={
              <div class="marketing-pulse__chart-empty"><BarChart3 size={18} strokeWidth={1.5} /><span>{chartMessage()}</span></div>
            }>
              <Show when={chartSelection()}>{(selection) => (
                <div class="marketing-pulse__chart-context" data-testid="pulse-chart-context" data-start-date={selection().startDate} data-end-date={selection().endDate} style={{ left: `${Math.min(78, Math.max(22, selection().left + selection().width / 2))}%` }}>
                  <span>{metric() === 'clicks' ? i18n.tr('Organiske klikk', 'Organic clicks') : i18n.tr('Søkevisninger', 'Search impressions')}</span>
                  <p>{selectionDateRange()}</p>
                  <strong>{display(value())}</strong>
                  <small>Google Search Console</small>
                </div>
              )}</Show>
              <div class="marketing-pulse__bars" style={{ 'grid-template-columns': `repeat(${bars().length}, minmax(0, 1fr))` }} aria-hidden="true">
                <For each={bars()}>{(bar, index) => <span data-pulse-date={bar.date} data-pulse-bar={bar.hasValue ? '' : undefined} data-pulse-selected={bar.hasValue && selectionIncludes(bar.date) ? '' : undefined} title={dateLabel(bar.date, true)} classList={{ 'is-empty': !bar.hasValue, 'is-selected': bar.hasValue && selectionIncludes(bar.date), 'is-outside-range': bar.hasValue && !selectionIncludes(bar.date) }} style={{ height: bar.height === 0 ? '1px' : `${bar.height}%`, background: bar.hasValue && selectionIncludes(bar.date) ? `hsl(${Math.round(42 - (index() / Math.max(bars().length - 1, 1)) * 20)} 88% 66%)` : undefined }} />}</For>
              </div>
            </Show>
          </div>
          <div class="marketing-pulse__timeline" data-testid="pulse-timeline-scrubber" data-start-date={snapshot()?.period.startDate} data-end-date={snapshot()?.period.endDate} data-selection-start-date={chartSelection()?.startDate} data-selection-end-date={chartSelection()?.endDate} role="group" aria-label={i18n.tr('Datofilter for signaler', 'Signal date filter')}>
            <div class="marketing-pulse__timeline-track" ref={(element) => { timelineTrack = element }}>
              <Show when={chartSelection()}>{(selection) => <>
                <span class="marketing-pulse__timeline-range" style={{ left: `${selection().left}%`, width: `${selection().width}%` }} />
                <button type="button" class="marketing-pulse__timeline-handle" data-pulse-range-handle="start" role="slider" aria-label={i18n.tr('Fra dato', 'Start date')} aria-valuemin={1} aria-valuemax={periodDates().length} aria-valuenow={selection().startIndex + 1} aria-valuetext={dateLabel(selection().startDate, true)} style={{ left: rangeHandlePosition(selection(), 'start') }} onPointerDown={(event) => startRangeDrag(event, 'start')} onPointerMove={(event) => continueRangeDrag(event, 'start')} onPointerUp={(event) => finishRangeDrag(event, 'start')} onPointerCancel={(event) => finishRangeDrag(event, 'start')} onKeyDown={(event) => moveRangeWithKeyboard(event, 'start')} />
                <button type="button" class="marketing-pulse__timeline-handle" data-pulse-range-handle="end" role="slider" aria-label={i18n.tr('Til dato', 'End date')} aria-valuemin={1} aria-valuemax={periodDates().length} aria-valuenow={selection().endIndex + 1} aria-valuetext={dateLabel(selection().endDate, true)} style={{ left: rangeHandlePosition(selection(), 'end') }} onPointerDown={(event) => startRangeDrag(event, 'end')} onPointerMove={(event) => continueRangeDrag(event, 'end')} onPointerUp={(event) => finishRangeDrag(event, 'end')} onPointerCancel={(event) => finishRangeDrag(event, 'end')} onKeyDown={(event) => moveRangeWithKeyboard(event, 'end')} />
              </>}</Show>
            </div>
            <div class="marketing-pulse__timeline-labels" aria-hidden="true">
              <For each={timeline()?.ticks ?? []}>{(tick) => <span data-pulse-date={tick.date} style={{ left: `${tick.position}%` }}>{dateLabel(tick.date)}</span>}</For>
            </div>
            <p class="marketing-pulse__data-availability" data-testid="pulse-data-availability">
              <Show when={connected() && lastAvailableInSelection()} fallback={connected() ? i18n.tr('Ingen dagsdata tilgjengelig i det valgte tidsrommet.', 'No daily data available in the selected range.') : ''}>
                {i18n.tr('Data til og med', 'Data through')} {dateLabel(lastAvailableInSelection() ?? '', true)}
                <span> · {i18n.tr('Tomme dager betyr at data ikke er tilgjengelig.', 'Gaps mean data is not available.')}</span>
              </Show>
            </p>
          </div>
        </div>
      </section>

      <section class="marketing-pulse__metrics" aria-label={i18n.tr('Nøkkeltall', 'Key metrics')}>
        <article class="marketing-pulse__metric-card" data-state={connected() ? 'connected' : 'unavailable'} data-testid="pulse-summary-card">
          <div><BarChart3 size={16} strokeWidth={1.7} /><span>{i18n.tr('Organiske klikk', 'Organic clicks')}</span></div>
          <strong data-testid="pulse-metric-value">{display(totalFor('clicks'))}</strong>
          <p>{metricSourceMessage()}</p>
        </article>
        <article class="marketing-pulse__metric-card" data-state={connected() ? 'connected' : 'unavailable'} data-testid="pulse-summary-card">
          <div><Presentation size={16} strokeWidth={1.7} /><span>{i18n.tr('Søkevisninger', 'Search impressions')}</span></div>
          <strong data-testid="pulse-metric-value">{display(totalFor('impressions'))}</strong>
          <p>{metricSourceMessage()}</p>
        </article>
        <article class="marketing-pulse__metric-card" data-state={workSnapshot() ? 'connected' : 'unavailable'} data-testid="pulse-summary-card">
          <div><Gauge size={16} strokeWidth={1.7} /><span>{i18n.tr('Prosjektmomentum', 'Project momentum')}</span></div>
          <strong data-testid="pulse-momentum-value">{workSnapshot()?.summary.completionPercent == null ? '—' : `${new Intl.NumberFormat(localeDateTime(i18n.locale()), { maximumFractionDigits: 1 }).format(workSnapshot()!.summary.completionPercent!)}%`}</strong>
          <p>{workSnapshot() ? `${workSnapshot()!.summary.done} / ${workSnapshot()!.summary.total - workSnapshot()!.summary.dismissed} ${i18n.tr('fullført', 'completed')}` : i18n.tr('Henter oppgaver fra arbeidskøen', 'Loading tasks from the work queue')}</p>
          <div class="marketing-pulse__metric-momentum" data-testid="pulse-project-momentum-card" aria-label={i18n.tr('Oppgavefremdrift', 'Task progress')}>
            <For each={Array.from({ length: 14 })}>{(_, index) => <span classList={{ 'is-complete': workSnapshot()?.summary.completionPercent != null && index() < Math.round(workSnapshot()!.summary.completionPercent! / 100 * 14) }} />}</For>
          </div>
        </article>
      </section>

      </Show>
      <Show when={selectedService() !== 'search_console'}><ProviderOverview provider={selectedService()} ga4={ga4Overview()} clarity={clarityOverview()} pageSpeed={speedOverview()}/></Show>
      </div>
      <Show when={selectedWebsite()}>{website => <VisibilitySummary context={{ websiteId: website().id, site: website().site, websiteRevision: website().revision }} quarter={quarter()}/>}</Show>
      <Show when={selectedWebsite()}>{website=><CollectionInventoryPanel context={{websiteId:website().id,site:website().site,websiteRevision:website().revision}} quarter={quarter()} refreshKey={analyticsRefresh()+auditRefresh()} onManage={()=>setSitePickerOpen(true)}/>}</Show>
      <Show when={selectedWebsite()}>{(website) => <>
        <Show when={`${website().id}:${website().revision}:${selectedService()}:${analyticsRefresh()}`} keyed>{context => <div ref={activeReport} class="pulse-service-report" data-context={context}>
        <Show when={serviceVisible('search_console')}>
        <SearchDashboard site={website().site} websiteId={website().id} revision={website().revision} quarter={quarter()} refreshKey={analyticsRefresh()} canManageIntegrations={canManage()} savedFindings={workSnapshot()?.tasks.flatMap((task) => task.evidence.kind === 'gsc_opportunity' ? [`${task.evidence.key}:${task.evidence.opportunityType}`] : []) ?? []} onTaskSaved={() => setWorkRefresh((value) => value + 1)} />
        </Show>
        <Show when={serviceVisible('ga4')}><GA4Panel site={website().site} websiteId={website().id} revision={website().revision} quarter={quarter()} refreshKey={analyticsRefresh()} onSnapshot={setGA4Overview} onManageWebsite={() => setSitePickerOpen(true)} /></Show>
        <Show when={serviceVisible('clarity')}><ClarityPanel site={website().site} websiteId={website().id} revision={website().revision} refreshKey={analyticsRefresh()} onSnapshot={setClarityOverview} onManageWebsite={() => setSitePickerOpen(true)} /></Show>
        <Show when={serviceVisible('web_vitals')}><WebVitalsPanel site={website().site} websiteId={website().id} revision={website().revision} canManageIntegrations={canManage()} /></Show>
        <Show when={serviceVisible('cookiebot')}><CookiebotPanel site={website().site} websiteId={website().id} revision={website().revision} canManageIntegrations={canManage()} /></Show>
        <Show when={serviceVisible('algolia')}><AlgoliaPanel showConnectionSetup={false} site={website().site} websiteId={website().id} revision={website().revision} canManageIntegrations={canManage()} /></Show>
        <Show when={serviceVisible('website')}>
        <div class="pulse-dashboard pulse-dashboard--web" data-testid="web-performance-dashboard">
        <PageSpeedPanel site={website().site} websiteId={website().id} revision={website().revision} refreshKey={auditRefresh()} onSnapshot={setSpeedOverview} savedFindings={workSnapshot()?.tasks.flatMap((task) => task.evidence.kind === 'pagespeed_check' ? [`${task.evidence.strategy}:${task.evidence.key}`] : []) ?? []} onTaskSaved={() => setWorkRefresh((value) => value + 1)} />
        <ReportDisclosure source={i18n.tr('Feltdata og nettarkiv', 'Field data and web archives')} title={i18n.tr('CrUX og offentlige signaler', 'CrUX and public signals')}><PublicSignalsPanel site={website().site} websiteId={website().id} revision={website().revision} canManageIntegrations={props.canManageIntegrations ?? false} /></ReportDisclosure>
        <ReportDisclosure source="Quarry-v2" title={i18n.tr('Oppdagelse og revisjon', 'Discovery and auditing')}>
        <WebsiteDiscoveryPanel site={website().site} websiteId={website().id} revision={website().revision} />
        <SiteAuditPanel site={website().site} websiteId={website().id} revision={website().revision} onWorkSaved={() => setWorkRefresh((value) => value + 1)} onSettled={() => setInsightsRefresh((value) => value + 1)} />
        <AuditMonitoringPanel site={website().site} websiteId={website().id} revision={website().revision} canConfigure={props.canConfigureMonitoring ?? true} />
        <AuditInsightsPanel site={website().site} websiteId={website().id} revision={website().revision} refreshKey={insightsRefresh()} />
        <WeeklyReportPanel site={website().site} websiteId={website().id} revision={website().revision} refreshKey={insightsRefresh()} />
        </ReportDisclosure>
        </div>
        </Show>
        </div>}</Show>
        <div class="marketing-pulse__advanced-sync"><AnalyticsSyncPanel site={website().site} websiteId={website().id} revision={website().revision} quarter={quarter()} canManageIntegrations={canManage()} onSettled={refreshReports} /></div>
      </>}</Show>

      <footer class="marketing-pulse__breakdown" data-testid="pulse-workspace-breakdown" aria-labelledby="marketing-pulse-breakdown-title">
        <header>
          <div class="marketing-pulse__breakdown-header-copy">
            <span>{i18n.tr('Arbeidsflate', 'Workspace')}</span>
            <h2 id="marketing-pulse-breakdown-title">{i18n.tr('Fordeling', 'Breakdown')}</h2>
            <p>{i18n.tr('Oppgaveliste og logg fylles bare med dokumentert arbeid.', 'Backlog and log fill only with documented work.')}</p>
          </div>
          <div class="marketing-pulse__view-switch" data-testid="pulse-workspace-view-switch" role="group" aria-label={i18n.tr('Visning for arbeidsflate', 'Workspace view')}>
            <button
              type="button"
              classList={{ 'is-active': workspaceView() === 'calendar' }}
              aria-pressed={workspaceView() === 'calendar'}
              onClick={() => setWorkspaceView('calendar')}
            >
              <CalendarDays size={17} strokeWidth={1.9} aria-hidden="true" />
              <span>{i18n.tr('Kalender', 'Calendar')}</span>
            </button>
            <button
              type="button"
              classList={{ 'is-active': workspaceView() === 'board' }}
              aria-pressed={workspaceView() === 'board'}
              onClick={() => setWorkspaceView('board')}
            >
              <LayoutGrid size={17} strokeWidth={1.9} aria-hidden="true" />
              <span>{i18n.tr('Tavle', 'Board')}</span>
            </button>
          </div>
        </header>

        <Show when={selectedWebsite()}>{(website) => <MarketingWorkQueue canWrite={props.canConfigureMonitoring??canManage()} websiteId={website().id} site={website().site} revision={website().revision} refreshKey={workRefresh()} view={workspaceView()} onTechnicalRechecked={() => setAuditRefresh((value) => value + 1)} onSnapshot={(value) => setWorkSnapshot(value && value.websiteId === website().id && value.site === website().site ? value : null)} />}</Show>

      </footer>
    </div>
  )
}
