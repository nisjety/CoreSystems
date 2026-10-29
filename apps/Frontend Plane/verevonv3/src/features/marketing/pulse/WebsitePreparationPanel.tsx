import { For, Show, createEffect, createSignal, createUniqueId, onCleanup } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { loadSourceCatalog, loadWebsitePreparation, prepareWebsite, type MarketingSource, type PreparationReason, type PreparationStep, type SourceCatalog, type WebsitePreparation } from '@/shared/api/marketing/preparation-client'
import type { DiscoveryContext } from '@/shared/api/marketing/discovery-client'
import './website-preparation-panel.css'

type Props = { websiteId: string; site: string; revision: number; canManageIntegrations?: boolean }
const names: Record<MarketingSource['key'], string> = { quarry: 'Quarry', pagespeed: 'PageSpeed Insights', search_console: 'Google Search Console', ga4: 'Google Analytics 4', clarity: 'Microsoft Clarity', google_ads: 'Google Ads', bing_webmaster: 'Bing Webmaster Tools', bing_ai: 'Bing AI Performance', crux: 'Chrome UX Report', web_vitals: 'First-party Web Vitals', google_trends: 'Google Trends', common_crawl: 'Common Crawl', cookiebot: 'Cookiebot', algolia: 'Algolia' }
const active = (value: WebsitePreparation | null) => value?.status === 'queued' || value?.status === 'running'

/** Reads never start provider work; the server owns preparation after URL/connection changes. */
export function WebsitePreparationPanel(props: Props) {
  const i18n = useI18n(), id = createUniqueId()
  const [snapshot, setSnapshot] = createSignal<WebsitePreparation | null>(null)
  const [catalog, setCatalog] = createSignal<SourceCatalog | null>(null)
  const [loading, setLoading] = createSignal(true), [saving, setSaving] = createSignal(false)
  const [error, setError] = createSignal(''), [catalogError, setCatalogError] = createSignal('')
  const [pollExpired, setPollExpired] = createSignal(false), [refreshKey, setRefreshKey] = createSignal(0)
  const context = (): DiscoveryContext => ({ websiteId: props.websiteId, site: props.site, websiteRevision: props.revision })
  const allowed = () => props.canManageIntegrations ?? false
  let generation = 0, timer: ReturnType<typeof setTimeout> | undefined, controller: AbortController | undefined
  let polls = 0
  const stamp = (value?: string) => value ? `${new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short', timeZone: 'UTC' }).format(new Date(value))} UTC` : '—'
  const status = (value: WebsitePreparation['status'] | PreparationStep['status']) => ({ idle: i18n.tr('Ikke klargjort', 'Not prepared'), queued: i18n.tr('I kø', 'Queued'), running: i18n.tr('Pågår', 'Running'), complete: i18n.tr('Fullført', 'Complete'), failed: i18n.tr('Mislyktes', 'Failed'), partial: i18n.tr('Delvis fullført', 'Partially complete'), unavailable: i18n.tr('Utilgjengelig', 'Unavailable') })[value]
  const reason = (value: PreparationReason) => ({ not_configured: i18n.tr('Ikke konfigurert', 'Not configured'), source_unavailable: i18n.tr('Kilden er utilgjengelig', 'Source unavailable'), quota_limited: i18n.tr('Kvotegrense nådd', 'Quota limited'), capacity_deferred: i18n.tr('Utsatt på grunn av kapasitet', 'Deferred for capacity'), timeout: i18n.tr('Tidsavbrudd', 'Timed out'), interrupted: i18n.tr('Avbrutt', 'Interrupted'), context_changed: i18n.tr('Nettstedet er endret', 'Website context changed') })[value]
  const sourceStatus = (value: MarketingSource) => !value.implemented ? i18n.tr('Planlagt—samler ikke data ennå', 'Planned—not collecting yet') : ({ available: i18n.tr('Tilgjengelig kontroll', 'Check available'), connected: i18n.tr('Tilkoblet; rapporttilgang kontrolleres separat', 'Connected; report access checked separately'), authorization_required: i18n.tr('Tilgang må fornyes', 'Access needs renewal'), needs_connection: i18n.tr('Koble til konto', 'Account connection needed'), requires_consent: i18n.tr('Trenger samtykke', 'Consent needed'), needs_selection: i18n.tr('Velg prosjekt', 'Project selection needed'), not_found: i18n.tr('Ingen samsvarende tilgang funnet', 'No matching access found'), source_unavailable: i18n.tr('Kilden er utilgjengelig', 'Source unavailable'), pending_verification: i18n.tr('Venter på verifisering', 'Pending verification'), not_connected: i18n.tr('Ikke tilkoblet', 'Not connected'), not_implemented: i18n.tr('Planlagt—samler ikke data ennå', 'Planned—not collecting yet') })[value.status]
  const refreshLabel = (value: MarketingSource['refresh']) => ({ preparation_job: i18n.tr('Ved klargjøring', 'During preparation'), dashboard_read: i18n.tr('Hentes ved rapportlesing', 'Fetched when reading reports'), snapshot_read: i18n.tr('Leser lagret rapport; innhenting styres separat', 'Reads saved snapshots; collection is managed separately'), public_signal_job: i18n.tr('Lagret offentlig rapport; daglig innhenting styres i Synlighet', 'Saved public report; daily collection is managed in Visibility'), trends_job: i18n.tr('Daglig, fra lagrede Search Console-søkeord', 'Daily, from saved Search Console keywords'), manual: i18n.tr('Manuelt', 'Manual'), not_available: i18n.tr('Ingen innhenting', 'No collection') })[value]

  function schedule(selected: DiscoveryContext, version: number, signal: AbortSignal, value: WebsitePreparation) {
    if (!active(value)) return
    if (polls >= 90) { setPollExpired(true); return }
    timer = setTimeout(() => { polls++; void read(selected, version, signal) }, 2000)
  }
  async function read(selected: DiscoveryContext, version: number, signal: AbortSignal) {
    try {
      const value = await loadWebsitePreparation(selected, signal)
      if (signal.aborted || version !== generation) return
      setSnapshot(value); setError(''); schedule(selected, version, signal, value)
    } catch {
      if (!signal.aborted && version === generation) setError(i18n.tr('Klargjøringsstatus kunne ikke lastes. Oppdater før du prøver igjen.', 'Preparation status could not be loaded. Refresh before retrying.'))
    } finally { if (!signal.aborted && version === generation) setLoading(false) }
  }
  createEffect(() => {
    const selected = context(), version = ++generation
    refreshKey(); allowed()
    controller?.abort(); clearTimeout(timer); polls = 0
    const current = new AbortController(); controller = current
    setSnapshot(null); setCatalog(null); setLoading(true); setSaving(false); setError(''); setCatalogError(''); setPollExpired(false)
    void read(selected, version, current.signal)
    void loadSourceCatalog(selected, current.signal).then((value) => {
      if (!current.signal.aborted && version === generation) setCatalog(value)
    }).catch(() => {
      if (!current.signal.aborted && version === generation) setCatalogError(i18n.tr('Kildeoversikten kunne ikke lastes.', 'Source catalogue could not be loaded.'))
    })
    onCleanup(() => { current.abort(); clearTimeout(timer) })
  })
  async function prepare() {
    if (!allowed() || loading() || saving() || error() || !snapshot() || active(snapshot())) return
    const selected = context(), version = generation, current = controller
    if (!current) return
    setSaving(true); setError(''); setPollExpired(false); polls = 0; clearTimeout(timer)
    try {
      const value = await prepareWebsite(selected, current.signal)
      if (!current.signal.aborted && version === generation && allowed()) { setSnapshot(value); schedule(selected, version, current.signal, value) }
    } catch (cause) {
      if (!current.signal.aborted && version === generation) setError(cause instanceof Error && cause.message === 'preparation_conflict' ? i18n.tr('Nettstedet er endret. Oppdater før du prøver igjen.', 'Website changed. Refresh before retrying.') : i18n.tr('Klargjøringen kunne ikke bekreftes. Oppdater status før du prøver igjen.', 'Preparation could not be confirmed. Refresh status before retrying.'))
    } finally { if (!current.signal.aborted && version === generation) setSaving(false) }
  }

  return <section class="website-preparation" aria-labelledby={`${id}-title`} data-testid="website-preparation-panel">
    <header><div><strong id={`${id}-title`}>{i18n.tr('Klargjøring av nettsted', 'Website preparation')}</strong><p>{i18n.tr('Oppdagelse og PageSpeed-kontroller for den valgte nettadressen.', 'Discovery and PageSpeed checks for the selected URL.')}</p></div><button type="button" disabled={loading() || saving()} onClick={() => setRefreshKey((value) => value + 1)}>{i18n.tr('Oppdater klargjøringsstatus', 'Refresh preparation status')}</button></header>
    <Show when={loading()}><p role="status">{i18n.tr('Laster klargjøring…', 'Loading preparation…')}</p></Show>
    <Show when={error()}><p role="alert">{error()}</p></Show>
    <Show when={snapshot()}>{(value) => <><p role="status"><strong>{status(value().status)}</strong></p><Show when={value().jobId}><details><summary>{i18n.tr('Kjøringskvittering', 'Preparation receipt')}</summary><code>{value().jobId}</code><p>{i18n.tr('I kø', 'Queued')}: {stamp(value().queuedAt)} · {i18n.tr('Startet', 'Started')}: {stamp(value().startedAt)} · {i18n.tr('Fullført', 'Completed')}: {stamp(value().completedAt)}</p></details></Show><ul class="website-preparation__steps"><For each={value().steps}>{(step) => <li><strong>{{ discovery: i18n.tr('Sideoppdagelse · Quarry', 'Page discovery · Quarry'), pagespeed_mobile: 'PageSpeed · mobile', pagespeed_desktop: 'PageSpeed · desktop' }[step.key]}</strong><span>{status(step.status)}</span><Show when={step.reason}>{(why) => <span>{reason(why())}</span>}</Show><span>{i18n.tr('Målt', 'Observed')}: {stamp(step.observedAt)}</span></li>}</For></ul></>}</Show>
    <Show when={pollExpired()}><p role="status">{i18n.tr('Automatisk statusoppdatering er stoppet etter tre minutter. Jobben kan fortsatt kjøre på serveren; oppdater status manuelt.', 'Automatic status refresh stopped after three minutes. The job may still be running on the server; refresh status manually.')}</p></Show>
    <Show when={allowed()} fallback={<p>{i18n.tr('En administrator kan klargjøre offentlige kontroller. Du har lesetilgang.', 'An administrator can prepare public checks. You have read-only access.')}</p>}><button type="button" disabled={loading() || saving() || !!error() || !snapshot() || active(snapshot())} onClick={() => void prepare()}>{saving() ? i18n.tr('Starter…', 'Starting…') : snapshot()?.status === 'failed' || snapshot()?.status === 'partial' ? i18n.tr('Prøv offentlige kontroller igjen', 'Retry public checks') : i18n.tr('Klargjør offentlige kontroller', 'Prepare public checks')}</button></Show>
    <p class="website-preparation__scope">{i18n.tr('Fullførte kontroller betyr ikke at private kontodata er klare. Her lagres en klargjøringskvittering. Innhenting av private rapporter styres separat i Synlighet; rapportpanelene leser lagrede data med tilgangskontroll. Egne sammenligningsanalyser kan fortsatt hente data direkte. En URL gir ikke konto- eller eierskapstilgang.', 'A completed public check does not mean private account data is ready. This stores a preparation receipt. Private snapshot collection is managed separately in Visibility; report panels read saved data with access checks. Separate comparison analyses may still fetch data directly. A URL does not grant account access or prove ownership.')}</p>
    <details><summary>{i18n.tr('Datakilder og støtte', 'Data sources and support')}</summary><Show when={catalogError()}><p role="alert">{catalogError()}</p></Show><Show when={catalog()}>{(value) => <ul class="website-preparation__sources"><For each={value().items}>{(item) => <li><strong>{names[item.key]}</strong><span>{sourceStatus(item)}</span><small>{refreshLabel(item.refresh)}</small></li>}</For></ul>}</Show></details>
  </section>
}
