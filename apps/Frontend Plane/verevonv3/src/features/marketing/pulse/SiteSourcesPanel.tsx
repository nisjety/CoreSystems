import { For, Show, createEffect, createSignal, createUniqueId, onCleanup } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { loadSiteSource, syncSiteSource, type CookiebotData, type AlgoliaData, type SiteSourceProvider, type SiteSourceReport, type SourceComponentStatus } from '@/shared/api/marketing/site-sources-client'
import type { DiscoveryContext } from '@/shared/api/marketing/discovery-client'
import './site-sources-panel.css'
import { AlgoliaConnectionPanel } from './AlgoliaConnectionPanel'
import { AlgoliaCollectionPanel } from './AlgoliaCollectionPanel'
import { AlgoliaTermsTable } from './AlgoliaTermsTable'

type Props = { websiteId: string; site: string; revision: number; canManageIntegrations?: boolean; refreshKey?: number; onReport?: (value: SiteSourceReport | null) => void }
function useSourceFormatting() {
  const i18n = useI18n()
  return { i18n, number: (value: number | undefined) => value === undefined ? '—' : new Intl.NumberFormat(localeDateTime(i18n.locale())).format(value), stamp: (value: string) => `${new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short', timeZone: 'UTC' }).format(new Date(value))} UTC` }
}
function ComponentStatus(props: { status: SourceComponentStatus }) {
  const { i18n } = useSourceFormatting()
  return <p class="site-source__status">{{ connected: i18n.tr('Rapportert', 'Reported'), no_data: i18n.tr('Ingen data rapportert', 'No data reported'), access_denied: i18n.tr('Leverandørtilgang avvist', 'Provider access denied'), plan_required: i18n.tr('Leverandørplanen støtter ikke denne rapporten', 'Provider plan does not support this report'), rate_limited: i18n.tr('Leverandørkvote nådd; vent før neste forsøk', 'Provider quota reached; wait before retrying'), unavailable: i18n.tr('Leverandørrapporten er utilgjengelig', 'Provider report unavailable') }[props.status]}</p>
}
function CookiebotEvidence(props: { data: CookiebotData }) {
  const { i18n, number, stamp } = useSourceFormatting()
  const totals = () => props.data.consents.totals
  const inventory = () => props.data.inventory
  return <><p>{i18n.tr('Rapporteringsdomene', 'Reporting domain')}: <code>{props.data.reportingDomain}</code></p>
    <section aria-label={i18n.tr('Samtykkehendelser', 'Consent events')}><h3>{i18n.tr('Samtykkehendelser', 'Consent events')}</h3><ComponentStatus status={props.data.consents.status} /><p>{props.data.consents.reportedDays} {i18n.tr('av 7 fullførte UTC-datoer rapportert. Manglende datoer fylles ikke med null.', 'of 7 completed UTC dates reported. Missing dates are not filled with zero.')}</p><p>{i18n.tr('Totaltallene er hendelser, ikke unike besøkende. Preferanser, statistikk og markedsføring kan overlappe.', 'Totals are events, not unique visitors. Preferences, statistics and marketing can overlap.')}</p>
      <Show when={totals()}>{(value) => <dl><For each={['optIn', 'optOut', 'preferences', 'statistics', 'marketing'] as const}>{(key) => <div><dt>{{ optIn: i18n.tr('Valgte inn', 'Opt-in'), optOut: i18n.tr('Valgte ut', 'Opt-out'), preferences: i18n.tr('Preferanser', 'Preferences'), statistics: i18n.tr('Statistikk', 'Statistics'), marketing: i18n.tr('Markedsføring', 'Marketing') }[key]}</dt><dd>{number(value()[key])}</dd></div>}</For></dl>}</Show>
    </section><section aria-label={i18n.tr('Datert informasjonskapselinventar', 'Dated cookie inventory')}><h3>{i18n.tr('Datert informasjonskapselinventar', 'Dated cookie inventory')}</h3><ComponentStatus status={inventory().status} /><p>{i18n.tr('Leverandørens daterte skann, ikke bevis på nåværende etterlevelse eller et juridisk samsvarsstempel.', 'The provider’s dated scan, not proof of current compliance or a legal compliance certification.')}</p>
      <Show when={inventory().status === 'connected'}><p>{i18n.tr('Skannet', 'Scanned')}: {stamp(inventory().scannedAt!)}</p><dl><For each={['total', 'thirdParty', 'secure', 'httpOnly'] as const}>{(key) => <div><dt>{{ total: i18n.tr('Informasjonskapsler', 'Cookies'), thirdParty: i18n.tr('Tredjepart', 'Third-party'), secure: 'Secure', httpOnly: 'HttpOnly' }[key]}</dt><dd>{number(inventory()[key])}</dd></div>}</For></dl><dl><For each={['necessary', 'preferences', 'statistics', 'marketing', 'unclassified'] as const}>{(key) => <div><dt>{{ necessary: i18n.tr('Nødvendige', 'Necessary'), preferences: i18n.tr('Preferanser', 'Preferences'), statistics: i18n.tr('Statistikk', 'Statistics'), marketing: i18n.tr('Markedsføring', 'Marketing'), unclassified: i18n.tr('Uklassifiserte', 'Unclassified') }[key]}</dt><dd>{number(inventory().categories?.[key])}</dd></div>}</For></dl></Show>
    </section></>
}
function AlgoliaEvidence(props: { data: AlgoliaData }) {
  const { i18n, number } = useSourceFormatting()
  const metrics = () => props.data.analytics
  const rate = () => metrics().noResultRate == null ? '—' : new Intl.NumberFormat(localeDateTime(i18n.locale()), { style: 'percent', maximumFractionDigits: 2 }).format(metrics().noResultRate!)
  return <><p>{i18n.tr('Eksisterende indeks', 'Existing index')} · {props.data.region === 'provider' ? i18n.tr('Leverandørstyrt', 'Provider-managed') : props.data.region.toUpperCase()}: <code>{props.data.indexName}</code></p><ComponentStatus status={metrics().status} /><p>{metrics().reportedDays} {i18n.tr('av 7 fullførte UTC-datoer rapportert. Manglende datoer fylles ikke med null.', 'of 7 completed UTC dates reported. Missing dates are not filled with zero.')}</p><Show when={metrics().status === 'connected'}><dl><div><dt>{i18n.tr('Søk', 'Searches')}</dt><dd>{number(metrics().searches)}</dd></div><div><dt>{i18n.tr('Søk uten resultat', 'Searches without results')}</dt><dd>{number(metrics().noResultSearches)}</dd></div><div><dt>{i18n.tr('Andel uten resultat', 'No-result rate')}</dt><dd data-testid="algolia-no-result-rate">{rate()}</dd></div></dl></Show>
    <AlgoliaTermsTable title={i18n.tr('Mest brukte søkefraser','Top searches')} terms={props.data.topSearches} /><AlgoliaTermsTable title={i18n.tr('Søkefraser uten resultat','No-result terms')} terms={props.data.noResultTerms} />
    <p>{i18n.tr('Samlede søkefraser fra den valgte indeksen er private for arbeidsområdet. Ingen bruker-ID-er, individuelle søkelogger eller indeksendringer. Hver rapportdel viser sin egen tilgjengelighet; null søk har ingen beregnbar andel uten resultat.','Aggregate search terms from the selected index are private to this workspace. No user IDs, individual search logs or index changes. Each component shows its own availability; zero searches have no calculable no-result rate.')}</p></>
}
function SiteSourcePanel(props: Props & { provider: SiteSourceProvider }) {
  const { i18n, stamp } = useSourceFormatting(), id = createUniqueId()
  const [report, setReport] = createSignal<SiteSourceReport | null>(null), [loading, setLoading] = createSignal(true), [saving, setSaving] = createSignal(false)
  const [error, setError] = createSignal(''), [refresh, setRefresh] = createSignal(0), [expired, setExpired] = createSignal(false)
  const allowed = () => props.canManageIntegrations ?? false
  const name = () => props.provider === 'cookiebot' ? 'Cookiebot' : 'Algolia'
  const context = (): DiscoveryContext => ({ websiteId: props.websiteId, site: props.site, websiteRevision: props.revision })
  let generation = 0, controller: AbortController | undefined, timer: ReturnType<typeof setTimeout> | undefined
  async function read(selected: DiscoveryContext, selectedProvider: SiteSourceProvider, current: AbortController, version: number, sync = false) {
    try {
      const value = await (sync ? syncSiteSource : loadSiteSource)(selected, selectedProvider, current.signal)
      if (current.signal.aborted || version !== generation || sync && !allowed()) return
      clearTimeout(timer); setReport(value); props.onReport?.(value); setError('')
      const midnight = new Date(`${new Date(value.observedAt).toISOString().slice(0, 10)}T00:00:00Z`).getTime() + 86400000
      const remaining = Math.min(Date.parse(value.observedAt) + 6 * 3600000, midnight) - Date.now()
      const hasEvidence = value.status !== 'not_loaded'
      setExpired(hasEvidence && remaining <= 0)
      if (hasEvidence && remaining > 0) timer = setTimeout(() => setExpired(true), Math.min(remaining + 1, 6 * 3600000 + 1))
    } catch {
      if (!current.signal.aborted && version === generation) { setReport(null); setError(i18n.tr('Leverandørstatus kunne ikke lastes. Oppdater status før et nytt forsøk.', 'Provider status could not be loaded. Refresh status before retrying.')) }
    } finally { if (!current.signal.aborted && version === generation) { setLoading(false); setSaving(false) } }
  }
  createEffect(() => {
    const selected = context(), selectedProvider = props.provider, version = ++generation
    refresh(); void props.refreshKey; allowed(); controller?.abort(); clearTimeout(timer)
    const current = new AbortController(); controller = current
    setReport(null); props.onReport?.(null); setLoading(true); setSaving(false); setError(''); setExpired(false)
    void read(selected, selectedProvider, current, version)
    onCleanup(() => { current.abort(); clearTimeout(timer) })
  })
  const sync = () => {
    if (!allowed() || loading() || saving() || !controller) return
    setSaving(true); setError(''); void read(context(), props.provider, controller, generation, true)
  }
  return <section class="site-source" aria-labelledby={`${id}-title`} aria-busy={loading() || saving()} data-testid={`site-source-${props.provider}`}><header><div><span>{name()}</span><h2 id={`${id}-title`}>{props.provider === 'cookiebot' ? i18n.tr('Samtykke og datert inventar', 'Consent and dated inventory') : i18n.tr('Eksisterende søkeanalytikk', 'Existing search analytics')}</h2><p>{i18n.tr('Siste syv fullførte UTC-datoer; følger ikke valgt kvartal. Kun lesing.', 'Last seven completed UTC dates; independent of the selected quarter. Read-only.')}</p></div><div class="site-source__controls"><button type="button" disabled={loading() || saving()} onClick={() => setRefresh((value) => value + 1)}>{i18n.tr('Oppdater lagret status', 'Refresh saved status')}</button><Show when={allowed()}><button type="button" disabled={loading() || saving()} onClick={sync}>{i18n.tr(`Hent ${name()}-rapport`, `Fetch ${name()} report`)}</button></Show></div></header>
    <Show when={loading()}><p role="status">{i18n.tr('Laster lagret leverandørstatus…', 'Loading saved provider status…')}</p></Show><Show when={saving()}><p role="status">{i18n.tr('Henter leverandørrapport…', 'Fetching provider report…')}</p></Show><Show when={error()}><p role="alert">{error()}</p></Show>
    <Show when={expired()}><p role="status">{i18n.tr('Den lagrede rapporten er utløpt. Oppdater status; en administrator kan hente en ny rapport.', 'The saved snapshot has expired. Refresh status; an administrator can fetch a new report.')}</p></Show>
    <Show when={!expired() && report()}>{(current) => <><p role="status">{{ not_loaded: i18n.tr('Ingen lagret leverandørrapport', 'No saved provider snapshot'), not_configured: i18n.tr('Leverandøren er ikke konfigurert', 'Provider not configured'), connected: i18n.tr('Leverandørobservasjoner tilgjengelige', 'Provider observations available'), partial: i18n.tr('Delvis leverandørrapport', 'Partial provider report'), unavailable: i18n.tr('Leverandørkontroller utilgjengelige', 'Provider checks unavailable') }[current().status]}</p><p>{current().window.startDate} – {current().window.endDate} · UTC</p>
      <Show when={report()!.status === 'not_loaded'}><p>{i18n.tr('Ingen fersk rapport er lastet. Dette betyr ikke at tilkoblingen mangler. Oppdater status for å lese lagrede data, eller hent en ny rapport som administrator.', 'No fresh snapshot is loaded. This does not mean the connection is missing. Refresh status to read saved data, or fetch a new report as an administrator.')}</p></Show>
      <Show when={report()!.status === 'not_configured'}><p>{props.provider === 'cookiebot' ? i18n.tr('Cookiebot trenger eksisterende installasjon, domenegruppe og Data API-tilgang i beskyttet serverkonfigurasjon i Integration Core. Ingen API-nøkler skal legges inn i nettleseren.', 'Cookiebot needs its existing installation, domain group and Data API access in protected server configuration in Integration Core. Never enter API keys in the browser.') : i18n.tr('Koble Algolia-konto og velg eksisterende applikasjon og indeks ovenfor. Begrenset analytics-nøkkel i beskyttet serverkonfigurasjon er kun et operatørstyrt alternativ. Ingen Admin-nøkkel eller API-nøkler i nettleseren.', 'Connect your Algolia account and choose the existing application and index above. A restricted analytics key in protected server configuration is an operator-managed fallback only. No Admin key or API keys in the browser.')}</p></Show>
      <Show when={report()!.cookiebot}>{(value) => <CookiebotEvidence data={value()} />}</Show><Show when={report()!.algolia}>{(value) => <AlgoliaEvidence data={value()} />}</Show><p>{report()!.status === 'not_loaded' ? i18n.tr('Lest status', 'Status read') : report()!.status === 'not_configured' ? i18n.tr('Konfigurasjon kontrollert', 'Configuration checked') : i18n.tr('Kildesjekk fullført', 'Source check completed')}: {stamp(report()!.observedAt)}</p>
    </>}</Show><footer>{i18n.tr('Statusoppdatering leser bare en lagret rapport. Leverandørinnhenting krever administratorhandling. Lagrede observasjoner er gyldige i opptil seks timer, senest til neste UTC-dato; tilganger kontrolleres på nytt ved innhenting. Manglende data blir ikke til null.', 'Refreshing status only reads a saved report. Provider collection requires an administrator action. Saved observations are fresh for up to six hours, no later than the next UTC date; permissions are checked again when collecting. Missing data never becomes zero.')}</footer>
  </section>
}
export const CookiebotPanel = (props: Props) => <SiteSourcePanel {...props} provider="cookiebot" />
export function AlgoliaPanel(props: Props & { showConnectionSetup?: boolean }) {
  const [mappingRefresh, setMappingRefresh] = createSignal(0), [reportRefresh,setReportRefresh]=createSignal(0), [snapshot,setSnapshot]=createSignal<SiteSourceReport|null>(null)
  return <><Show when={props.showConnectionSetup !== false}><AlgoliaConnectionPanel {...props} onMappingChanged={() => {setMappingRefresh(n=>n+1);setReportRefresh(n=>n+1)}} /></Show><AlgoliaCollectionPanel {...props} refreshKey={mappingRefresh()} snapshot={snapshot()} onReportChanged={()=>setReportRefresh(n=>n+1)} /><SiteSourcePanel {...props} refreshKey={reportRefresh()} onReport={setSnapshot} provider="algolia" /></>
}
