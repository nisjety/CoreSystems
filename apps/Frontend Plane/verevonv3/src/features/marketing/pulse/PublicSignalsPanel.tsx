import { For, Show, createEffect, createSignal, createUniqueId, onCleanup } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { loadPublicSignals, requestPublicSignals, setPublicSignalsEnabled, type PublicProviderReason, type PublicSignalsSnapshot } from '@/shared/api/marketing/public-signals-client'
import type { DiscoveryContext } from '@/shared/api/marketing/discovery-client'
import './public-signals-panel.css'

type Props = { websiteId: string; site: string; revision: number; canManageIntegrations?: boolean }
const active = (value: PublicSignalsSnapshot | null) => value?.status === 'queued' || value?.status === 'running'

/** Public evidence is already saved: rendering never enrolls or starts collection. */
export function PublicSignalsPanel(props: Props) {
  const i18n = useI18n(), id = createUniqueId()
  const [snapshot, setSnapshot] = createSignal<PublicSignalsSnapshot | null>(null)
  const [loading, setLoading] = createSignal(true), [saving, setSaving] = createSignal(false)
  const [error, setError] = createSignal(''), [pollExpired, setPollExpired] = createSignal(false), [reportExpired, setReportExpired] = createSignal(false)
  const [refresh, setRefresh] = createSignal(0)
  const allowed = () => props.canManageIntegrations ?? false
  const context = (): DiscoveryContext => ({ websiteId: props.websiteId, site: props.site, websiteRevision: props.revision })
  let generation = 0, polls = 0, timer: ReturnType<typeof setTimeout> | undefined, expiryTimer: ReturnType<typeof setTimeout> | undefined, controller: AbortController | undefined
  const stamp = (value?: string) => value ? `${new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short', timeZone: 'UTC' }).format(new Date(value))} UTC` : '—'
  const number = (value: number | null | undefined, digits = 0) => value == null ? '—' : new Intl.NumberFormat(localeDateTime(i18n.locale()), { maximumFractionDigits: digits }).format(value)
  const report = () => reportExpired() ? undefined : snapshot()?.report ?? snapshot()?.cachedReport
  const usingCache = () => !snapshot()?.report && !!report()
  const label = (value: PublicSignalsSnapshot['status']) => ({ idle: i18n.tr('Ingen offentlig rapport ennå', 'No public report yet'), queued: i18n.tr('I kø', 'Queued'), running: i18n.tr('Henter offentlig evidens', 'Collecting public evidence'), complete: i18n.tr('Offentlige kontroller ferdige', 'Public checks finished'), failed: i18n.tr('Innhenting mislyktes', 'Collection failed') })[value]
  const reason = (value?: PublicProviderReason) => ({ not_configured: i18n.tr('Ikke konfigurert', 'Not configured'), no_data: i18n.tr('Ingen offentlig rapportdata funnet', 'No public report data found'), quota_limited: i18n.tr('Leverandøren ber oss vente. Kvote eller midlertidig begrensning; ingen umiddelbare gjentatte forsøk.', 'The provider asks us to wait. Quota or temporary backpressure; no immediate repeated retries.'), source_unavailable: i18n.tr('Kilden er utilgjengelig', 'Source unavailable'), timeout: i18n.tr('Leverandøren svarte ikke innen tidsgrensen. Dette er ikke bevis på at data mangler. Prøv ved neste innhenting.', 'The provider timed out. This is not evidence of no data. Retry at the next collection.') })[value ?? 'source_unavailable']
  function accept(value: PublicSignalsSnapshot) {
    clearTimeout(expiryTimer); setSnapshot(value)
    const saved = value.report ?? value.cachedReport
    const expires = value.report ? Math.min(Date.parse(value.expiresAt ?? ''), Date.parse(value.report.observedAt) + 86400000) : Date.parse(value.cachedReport?.observedAt ?? '') + 2 * 86400000
    const remaining = saved ? expires - Date.now() : 0
    setReportExpired(!!saved && remaining <= 0)
    if (saved && remaining > 0) expiryTimer = setTimeout(() => setReportExpired(true), Math.min(remaining + 1, 2 * 86400000 + 1))
  }
  function schedule(selected: DiscoveryContext, version: number, signal: AbortSignal, value: PublicSignalsSnapshot) {
    if (!active(value)) return
    if (polls >= 90) { setPollExpired(true); return }
    timer = setTimeout(() => { polls++; void read(selected, version, signal) }, 2000)
  }
  async function read(selected: DiscoveryContext, version: number, signal: AbortSignal) {
    try {
      const value = await loadPublicSignals(selected, signal)
      if (signal.aborted || generation !== version) return
      accept(value); setError(''); schedule(selected, version, signal, value)
    } catch (failure) {
      if (!signal.aborted && generation === version) { if (failure instanceof Error && ['public_signals_unavailable_authority','public_signals_conflict'].includes(failure.message)) setSnapshot(null); setError(i18n.tr('Status for offentlige signaler kunne ikke lastes. Oppdater status før du prøver igjen.', 'Public signal status could not be loaded. Refresh status before retrying.')) }
    } finally { if (!signal.aborted && generation === version) setLoading(false) }
  }
  createEffect(() => {
    const selected = context(), version = ++generation
    refresh(); allowed()
    controller?.abort(); clearTimeout(timer); clearTimeout(expiryTimer); polls = 0
    const current = new AbortController(); controller = current
    setSnapshot(null); setLoading(true); setSaving(false); setError(''); setPollExpired(false); setReportExpired(false)
    void read(selected, version, current.signal)
    onCleanup(() => { current.abort(); clearTimeout(timer); clearTimeout(expiryTimer) })
  })
  async function change(enabled?: boolean) {
    if (!allowed() || loading() || saving() || error() || !snapshot() || enabled === undefined && active(snapshot())) return
    const current = controller, selected = context(), version = generation
    if (!current) return
    setSaving(true); setError(''); setPollExpired(false); polls = 0; clearTimeout(timer)
    try {
      const value = enabled === undefined ? await requestPublicSignals(selected, current.signal) : await setPublicSignalsEnabled(selected, enabled, current.signal)
      if (current.signal.aborted || generation !== version || !allowed()) return
      accept(value); schedule(selected, version, current.signal, value)
    } catch (cause) {
      if (!current.signal.aborted && generation === version) setError(cause instanceof Error && cause.message === 'public_signals_conflict' ? i18n.tr('Nettstedet er endret. Oppdater status.', 'Website changed. Refresh status.') : i18n.tr('Endringen kunne ikke bekreftes. Oppdater status før du prøver igjen.', 'The change could not be confirmed. Refresh status before retrying.'))
    } finally { if (!current.signal.aborted && generation === version) setSaving(false) }
  }

  return <section class="public-signals" aria-labelledby={`${id}-title`} aria-busy={loading() || saving()} data-testid="public-signals-panel">
    <header><div><span>{i18n.tr('Offentlig webevidens', 'Public web evidence')}</span><h2 id={`${id}-title`}>{i18n.tr('Reell ytelse og arkivspor', 'Real-world performance and archive traces')}</h2><p>{i18n.tr('Lagrede signaler for nøyaktig valgt nettsted, uavhengig av rapportkvartalet.', 'Saved signals for the exact selected website, independent of the reporting quarter.')}</p></div><button type="button" disabled={loading() || saving()} onClick={() => setRefresh((value) => value + 1)}>{i18n.tr('Oppdater status for offentlige signaler', 'Refresh public signal status')}</button></header>
    <Show when={loading()}><p role="status">{i18n.tr('Laster offentlige signaler…', 'Loading public signals…')}</p></Show>
    <Show when={error()}><p role="alert">{error()}</p></Show>
    <Show when={snapshot()}>{(value) => <><p role="status"><strong>{label(value().status)}</strong> · {value().enabled ? i18n.tr('Daglig innhenting aktiv', 'Daily collection enabled') : i18n.tr('Daglig innhenting pauset', 'Daily collection paused')}</p><Show when={value().status === 'failed'}><p>{i18n.tr('Ingen ny rapport ble bekreftet. Kontroller kan være avbrutt eller kilden utilgjengelig.', 'No new report was confirmed. Checks may have been interrupted or the source unavailable.')}</p></Show><Show when={value().status === 'complete' && !report()}><p>{i18n.tr('Den lagrede offentlige rapporten mangler eller er utløpt. Hent på nytt for nye målinger; dette betyr ikke at nettstedet er feilfritt.', 'The saved public report is unavailable or expired. Collect again for new observations; this does not mean the website is healthy.')}</p></Show><Show when={value().jobId}><details><summary>{i18n.tr('Innhentingskvittering', 'Collection receipt')}</summary><code>{value().jobId}</code><p>{i18n.tr('Fullført', 'Completed')}: {stamp(value().completedAt)} · {i18n.tr('Neste innhenting', 'Next collection')}: {stamp(value().nextSyncAt)} · {i18n.tr('Rapport utløper', 'Report expires')}: {stamp(value().expiresAt)}</p></details></Show></>}</Show>
    <Show when={usingCache()}><p role="status" data-testid="public-signals-cached">{i18n.tr('Siste lagrede rapport vises mens innhenting pågår eller prøves igjen. Målt', 'Showing the last saved report while collection runs or retries. Observed')} {stamp(report()?.observedAt)}. {i18n.tr('Dette er tidligere evidens, ikke et nytt vellykket forsøk. Rapporten fjernes etter 48 timer.', 'This is previous evidence, not a new successful check. It expires after 48 hours.')}</p></Show>
    <Show when={report()}>{(value) => <>
      <section class="public-signals__source" aria-labelledby={`${id}-crux`}>
        <h3 id={`${id}-crux`}>Chrome UX Report · CrUX</h3>
        <p>{i18n.tr('Feltdata fra faktiske Chrome-besøk: 28-dagers vindu, 75-persentilen. Oppgitt adresse sjekkes først. Ved manglende dekning prøves den eksakte www- eller ikke-www-varianten automatisk for hver enhet. Dette er ikke Lighthouse-labdata og følger ikke valgt kvartal.', 'Field data from actual Chrome visits: 28-day window, 75th percentile. The entered origin is checked first. If coverage is missing, its exact www or non-www counterpart is checked automatically for each device. This is not Lighthouse lab data and does not follow the selected quarter.')}</p>
        <div class="public-signals__devices"><For each={value().crux}>{(device) => <article>
          <h4>{device.formFactor === 'PHONE' ? i18n.tr('Mobil', 'Phone') : i18n.tr('Datamaskin', 'Desktop')}</h4>
          <Show when={device.status === 'connected'} fallback={<p>
            {device.reason === 'no_data' ? i18n.tr('Google har ingen offentlig CrUX-rapport for dette opphavet og denne enheten. Dette er manglende dekning, ikke en feil med kontotilkoblingen.', 'Google has no public CrUX report for this origin and device. This is missing coverage, not an account connection error.') : reason(device.reason)}
            <Show when={device.reason === 'not_configured'}><span>{i18n.tr('CrUX krever en begrenset API-nøkkel i Integration Core. Dette settes opp i utrullingen, ikke med brukerens Google-pålogging.', 'CrUX requires a restricted deployment API key in Integration Core, not the user’s Google sign-in.')}</span></Show>
          </p>}>
            <p data-testid={`crux-${device.formFactor}-origin`}>{i18n.tr('Rapporteringsopphav', 'Reporting origin')}: <code>{device.reportingOrigin ?? props.site.slice(0, -1)}</code><Show when={device.reportingOrigin && device.reportingOrigin !== props.site.slice(0, -1)}> · {i18n.tr('funnet automatisk', 'found automatically')}</Show></p>
            <p>{device.period?.startDate} – {device.period?.endDate}</p>
            <dl><For each={['lcpMs', 'inpMs', 'cls'] as const}>{(key) => <div><dt>{{ lcpMs: 'LCP', inpMs: 'INP', cls: 'CLS' }[key]}</dt><dd data-testid={`crux-${device.formFactor}-${key}`}>{number(device.metrics?.[key], key === 'cls' ? 4 : 0)}<Show when={device.metrics?.[key] != null && key !== 'cls'}><small> ms</small></Show></dd></div>}</For></dl>
          </Show>
        </article>}</For></div>
        <Show when={value().crux.some((device) => device.reason === 'no_data')}><p>{i18n.tr('CrUX krever tilstrekkelig kvalifisert Chrome-trafikk og offentlig indekserbare sider. Tilkobling av Analytics eller Clarity oppretter ikke CrUX-dekning. Lighthouse er en separat labtest.', 'CrUX requires sufficient eligible Chrome traffic and publicly indexable pages. Connecting Analytics or Clarity does not create CrUX coverage. Lighthouse is a separate lab test.')}</p><a href="https://developer.chrome.com/docs/crux/methodology" target="_blank" rel="noopener noreferrer">{i18n.tr('Om Googles CrUX-dekning', 'About Google’s CrUX coverage')}</a></Show>
        <p>{i18n.tr('Manglende målinger vises som —, ikke null. Manglende data er ikke en bestått Core Web Vitals-vurdering.', 'Missing metrics appear as —, not zero. Missing data is not a passing Core Web Vitals assessment.')}</p>
      </section>
      <section class="public-signals__source" aria-labelledby={`${id}-crawl`}>
        <h3 id={`${id}-crawl`}>Common Crawl</h3>
        <p>{i18n.tr('Den eksakte www-/ikke-www-varianten prøves automatisk ved manglende arkivdata eller en vertsforespørsel som feiler. Kvotegrenser respekteres.', 'The exact www/non-www counterpart is checked automatically for missing archive data or a failed host query. Quota limits are respected.')}</p>
        <p>{i18n.tr('Begrenset metadata fra siste tilgjengelige indeks: ikke en backlink-oversikt, en aktuell 404-kontroll eller en komplett historikk. Et arkivspor beviser ikke AI-trening eller AI-synlighet.', 'Limited metadata from the latest available index: not a backlink inventory, a current 404 check or a complete history. An archive trace does not prove AI training or AI visibility.')}</p>
        <Show when={value().commonCrawl.index}><p>{i18n.tr('Valgt indeks', 'Selected index')}: <code>{value().commonCrawl.index}</code></p></Show>
        <Show when={value().commonCrawl.status === 'connected'} fallback={<p>{value().commonCrawl.reason === 'no_data' ? i18n.tr('Ingen arkivspor funnet for denne verten i valgt indeks. Dette er ikke en kontroll av hele Common Crawl-historikken.', 'No archive captures found for this host in the selected index. This is not a check of all Common Crawl history.') : reason(value().commonCrawl.reason)}</p>}>
          <p data-testid="commoncrawl-reporting-host">{i18n.tr('Rapporteringsvert', 'Reporting host')}: <code>{value().commonCrawl.reportingHost ?? new URL(props.site).hostname}</code><Show when={value().commonCrawl.reportingHost && value().commonCrawl.reportingHost !== new URL(props.site).hostname}> · {i18n.tr('funnet automatisk', 'found automatically')}</Show></p>
          <p>{i18n.tr('Maksimalt 20 arkivspor', 'At most 20 archive captures')}<Show when={value().commonCrawl.truncated}> · {i18n.tr('Begrenset resultat', 'Truncated result')}</Show></p>
          <div class="public-signals__table"><table><caption>{i18n.tr('Historiske arkivspor · ikke nåværende sidestatus', 'Historical archive captures · not current page status')}</caption><thead><tr><th>URL</th><th>{i18n.tr('Arkivert', 'Captured')}</th><th>{i18n.tr('Registrert HTTP-status', 'Recorded HTTP status')}</th></tr></thead><tbody><For each={value().commonCrawl.captures}>{(capture) => <tr><td><a href={capture.url} target="_blank" rel="noopener noreferrer">{capture.url}</a></td><td>{stamp(capture.capturedAt)}</td><td>{capture.httpStatus}</td></tr>}</For></tbody></table></div>
        </Show>
      </section>
      <p>{i18n.tr('Kildesjekk fullført', 'Source check completed')}: {stamp(value().observedAt)}</p>
    </>}</Show>
    <Show when={pollExpired()}><p role="status">{i18n.tr('Automatisk statusoppdatering stoppet etter tre minutter. Serverjobben kan fortsatt kjøre. Oppdater manuelt.', 'Automatic status refresh stopped after three minutes. The server job may still be running. Refresh manually.')}</p></Show>
    <Show when={allowed()} fallback={<p>{i18n.tr('En administrator kan konfigurere offentlig innhenting. Du har lesetilgang.', 'An administrator can configure public collection. You have read-only access.')}</p>}><div class="public-signals__controls"><button type="button" disabled={loading() || saving() || !!error() || !snapshot() || active(snapshot())} onClick={() => void change()}>{i18n.tr('Hent offentlige signaler og aktiver daglig innhenting', 'Collect public signals and enable daily sync')}</button><Show when={snapshot() && snapshot()?.status !== 'idle'}><button type="button" disabled={loading() || saving() || !!error()} onClick={() => void change(!snapshot()!.enabled)}>{snapshot()?.enabled ? i18n.tr('Pause daglig innhenting', 'Pause daily sync') : i18n.tr('Gjenoppta daglig innhenting', 'Resume daily sync')}</button></Show></div></Show>
    <footer>{i18n.tr('Rapportlesing starter ikke innhenting. Daglig innhenting og lagrede rapporter er nettstedsspesifikke. En fullført jobb betyr at kildene ble kontrollert, ikke at alle kilder har data. Rapporter utløper etter 24 timer. Pausing stanser fremtidige planlagte jobber, men sletter ikke lagrede rapporter.', 'Reading reports does not start collection. Daily collection and saved reports are site-specific. A finished job means sources were checked, not that every source has data. Reports expire after 24 hours. Pausing stops future scheduled jobs but does not delete saved reports.')}</footer>
  </section>
}
