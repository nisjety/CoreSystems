import { For, Show, createEffect, createSignal, createUniqueId, on, onCleanup } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { loadAutomaticTrends, requestTrends, validTrendsQuery, type AutomaticTrends, type TrendsQuery, type TrendsReport } from '@/shared/api/marketing/trends-client'
import type { DiscoveryContext } from '@/shared/api/marketing/discovery-client'
import './public-signals-panel.css'
import './trends-panel.css'

type Props = { websiteId: string; site: string; revision: number; canManageIntegrations?: boolean; refreshKey?: number; onReport?: (report: TrendsReport | null) => void }
const colors = ['#4d7c69', '#6489af', '#bc8652', '#9566ad', '#b86678']

/** The default reads a precomputed report. Rendering never starts provider work. */
export function TrendsPanel(props: Props) {
  const i18n = useI18n(), id = createUniqueId()
  const [terms, setTerms] = createSignal(''), [geo, setGeo] = createSignal('NO')
  const [timeframe, setTimeframe] = createSignal<TrendsQuery['timeframe']>('today 12-m')
  const [manualReport, setReport] = createSignal<TrendsReport | null>(null), [loading, setLoading] = createSignal(false), [error, setError] = createSignal('')
  const [automatic, setAutomatic] = createSignal<AutomaticTrends | null>(null), [autoLoading, setAutoLoading] = createSignal(true), [autoError, setAutoError] = createSignal('')
  const report = () => manualReport() ?? automatic()?.report ?? null
  createEffect(() => props.onReport?.(report()))
  let controller: AbortController | undefined, autoController: AbortController | undefined, generation = 0, pollTimer: ReturnType<typeof setTimeout> | undefined, expiryTimer: ReturnType<typeof setTimeout> | undefined
  const query = (): TrendsQuery => ({ terms: terms().split(',').map((t) => t.trim()).filter(Boolean), geo: geo(), timeframe: timeframe() })
  const stamp = (v: string) => new Intl.DateTimeFormat(localeDateTime(i18n.locale()), { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(v))
  const periodLabel = (v: TrendsQuery['timeframe']) => ({ 'today 3-m': i18n.tr('3 måneder', '3 months'), 'today 12-m': i18n.tr('12 måneder', '12 months'), 'today 5-y': i18n.tr('5 år', '5 years') })[v]
  const context = (): DiscoveryContext => ({ websiteId: props.websiteId, site: props.site, websiteRevision: props.revision })
  async function readAutomatic(selected: DiscoveryContext, version: number, signal: AbortSignal, polls = 0) {
    try {
      const value = await loadAutomaticTrends(selected, signal)
      if (signal.aborted || generation !== version) return
      setAutomatic(value); setAutoError('')
      clearTimeout(expiryTimer)
      if (value.report && value.expiresAt) expiryTimer = setTimeout(() => {
        if (signal.aborted || generation !== version) return
        setAutomatic((previous) => previous ? { ...previous, report: undefined, status: previous.status === 'complete' ? 'pending' : previous.status } : previous)
        void readAutomatic(selected, version, signal)
      }, Math.max(1, Date.parse(value.expiresAt) - Date.now() + 1))
      if (['pending', 'running'].includes(value.status) && polls < 32) pollTimer = setTimeout(() => void readAutomatic(selected, version, signal, polls + 1), 15000)
    } catch {
      if (!signal.aborted && generation === version) { setAutomatic(null); setAutoError(i18n.tr('Den lagrede rapporten kunne ikke leses. Oppdater status for å prøve igjen.', 'The saved report could not be read. Refresh its status to retry.')) }
    } finally { if (!signal.aborted && generation === version) setAutoLoading(false) }
  }
  function refreshAutomatic() {
    autoController?.abort(); clearTimeout(pollTimer); clearTimeout(expiryTimer); const current = new AbortController(); autoController = current
    setAutoLoading(true); void readAutomatic(context(), generation, current.signal)
  }
  createEffect(on(() => [props.websiteId, props.site, props.revision, props.canManageIntegrations], () => {
    generation++; controller?.abort(); setReport(null); setError(''); setLoading(false); setTerms('')
    setAutomatic(null); setAutoError(''); refreshAutomatic()
  }))
  createEffect(on(() => props.refreshKey, () => refreshAutomatic(), { defer: true }))
  onCleanup(() => { controller?.abort(); autoController?.abort(); clearTimeout(pollTimer); clearTimeout(expiryTimer) })
  async function research(event: SubmitEvent) {
    event.preventDefault()
    const selected = { websiteId: props.websiteId, site: props.site, websiteRevision: props.revision }, q = query(), version = generation
    if (!props.canManageIntegrations || loading() || !validTrendsQuery(q)) return
    controller?.abort(); const current = new AbortController(); controller = current
    setLoading(true); setError(''); setReport(null)
    try {
      const value = await requestTrends(selected, q, current.signal)
      if (!current.signal.aborted && generation === version) setReport(value)
    } catch {
      if (!current.signal.aborted && generation === version) setError(i18n.tr('Rapporten kunne ikke hentes. Ingen søkeinteresse er registrert som null. Prøv senere.', 'The report could not be fetched. Missing interest has not been recorded as zero. Try later.'))
    } finally { if (!current.signal.aborted && generation === version) setLoading(false) }
  }
  const status = (v: TrendsReport['status']) => ({
    available: i18n.tr('Målt relativ søkeinteresse', 'Measured relative search interest'),
    no_data: i18n.tr('Google returnerte ingen serie for dette søket.', 'Google returned no series for this query.'),
    disabled: i18n.tr('Utviklingsadapteren er slått av på denne serveren.', 'The development adapter is disabled on this server.'),
    rate_limited: i18n.tr('Google begrenser forespørsler. Adapteren tar pause.', 'Google is rate-limiting requests. The adapter is paused.'),
    blocked: i18n.tr('Google avviste forespørselen. Vi omgår ikke blokkeringen.', 'Google blocked the request. We do not bypass the block.'),
    cooldown: i18n.tr('Adapteren venter før neste forespørsel.', 'The adapter is waiting before another request.'),
    timeout: i18n.tr('Tidsavbrudd hos Google. Datadekningen er ukjent.', 'Google timed out. Data coverage is unknown.'),
    upstream_error: i18n.tr('Den uoffisielle kilden svarte ikke som forventet. Ingen målinger vises.', 'The unofficial source did not respond as expected. No measurements are shown.'),
  })[v]
  const automaticStatus = (v: AutomaticTrends['status']) => ({
    pending: i18n.tr('Rapporten klargjøres i bakgrunnen.', 'The report is being prepared in the background.'),
    running: i18n.tr('Henter søkeinteresse for nettstedets søkeord.', 'Collecting interest for this site’s keywords.'),
    waiting_for_search_console: i18n.tr('Venter på ferske søkeord fra Search Console. Innhentingen starter automatisk når en autorisert søkeordsrapport er klar.', 'Waiting for fresh Search Console keywords. Collection starts automatically when an authorized keyword report is ready.'),
    waiting_for_keywords: i18n.tr('Search Console har ingen egnede søkeord med visninger i den lagrede rapporten ennå.', 'Search Console has no eligible queries with impressions in the saved report yet.'),
    complete: i18n.tr('Automatisk rapport fra Search Console-søkeord', 'Automatic report from Search Console keywords'),
    failed: i18n.tr('Bakgrunnsinnhentingen mislyktes eller er satt på pause av leverandøren. Serveren prøver igjen senere.', 'Background collection failed or the provider paused requests. The server will retry later.'),
  })[v]
  return <section class="public-signals trends-panel" aria-labelledby={`${id}-title`} aria-busy={loading() || autoLoading()} data-testid="trends-panel">
    <header><div><span>Google Trends · pytrends</span><h2 id={`${id}-title`}>{i18n.tr('Søkeinteresse', 'Search interest')}</h2></div><span class="trends-panel__badge">{i18n.tr('Kun utvikling · uoffisiell', 'Development only · unofficial')}</span></header>
    <p class="trends-panel__intro">{i18n.tr('Undersøk etterspørsel og sesongvariasjon for markedet rundt nettstedet. Dette er ikke nettstedets egne søk eller absolutte søkevolum.', 'Research market demand and seasonality around this website. These are not your site’s own searches or absolute search volumes.')}</p>
    <div class="trends-panel__automation"><div><p>{i18n.tr('Basert på nettstedets Search Console-søkeord', 'Based on your site’s Search Console queries')}</p><p>{i18n.tr('Automatisk oppdatering · daglig', 'Automatic refresh · daily')}</p></div>
    <button type="button" class="trends-panel__refresh" disabled={autoLoading()} onClick={refreshAutomatic}>{i18n.tr('Oppdater lagret rapport', 'Refresh saved report')}</button>
    </div>
    <Show when={autoLoading()}><p role="status">{i18n.tr('Leser lagret rapport…', 'Reading saved report…')}</p></Show>
    <Show when={autoError()}><p role="alert">{autoError()}</p></Show>
    <Show when={!manualReport() ? automatic() : null}>{(value) => <div>
      <p role="status">{automaticStatus(value().status)}</p>
      <Show when={value().status === 'waiting_for_search_console'}><p><a href="/settings/integrations">{i18n.tr('Administrer kontotilkobling', 'Manage account connection')}</a></p></Show>
      <details class="trends-panel__metadata"><summary>{i18n.tr('Datagrunnlag og oppdatering', 'Evidence and refresh details')}</summary>
      <p>{i18n.tr('Opptil fem søkeord med flest visninger i den lagrede Search Console-rapporten velges automatisk. Verdensomspennende interesse siste 12 måneder oppdateres daglig på serveren, også når Synlighet er lukket.', 'Up to five queries with the most impressions in the saved Search Console report are selected automatically. Worldwide interest over the last 12 months refreshes daily on the server, even when Visibility is closed.')}</p>
      <Show when={value().seed}>{(seed) => <p>{i18n.tr('Søkeordgrunnlag:', 'Keyword evidence:')} {seed().terms.join(' · ')} · {seed().period.startDate} – {seed().period.endDate} · {i18n.tr('observert', 'observed')} {stamp(seed().observedAt)}</p>}</Show>
      <Show when={value().nextSyncAt}><p>{i18n.tr('Neste bakgrunnsoppdatering:', 'Next background update:')} {stamp(value().nextSyncAt!)}</p></Show>
      </details>
    </div>}</Show>
    <Show when={manualReport()}><button type="button" class="trends-panel__refresh" onClick={() => setReport(null)}>{i18n.tr('Vis automatisk rapport', 'Show automatic report')}</button></Show>
    <details class="trends-panel__advanced"><summary>{i18n.tr('Avansert: undersøk andre søkeord', 'Advanced: research other terms')}</summary>
    <form onSubmit={(e) => void research(e)} class="trends-panel__form">
      <label for={`${id}-terms`}>{i18n.tr('Søkeord · opptil fem, adskilt med komma', 'Search terms · up to five, separated by commas')}<input id={`${id}-terms`} value={terms()} onInput={(e) => setTerms(e.currentTarget.value)} maxlength={400} placeholder={i18n.tr('Nettside, søkemotoroptimalisering', 'Web design, search engine optimization')} disabled={loading() || !props.canManageIntegrations} /></label>
      <label for={`${id}-geo`}>{i18n.tr('Marked', 'Market')}<select id={`${id}-geo`} value={geo()} onChange={(e) => setGeo(e.currentTarget.value)} disabled={loading()}><option value="NO">{i18n.tr('Norge', 'Norway')}</option><option value="">{i18n.tr('Hele verden', 'Worldwide')}</option><option value="SE">{i18n.tr('Sverige', 'Sweden')}</option><option value="DK">{i18n.tr('Danmark', 'Denmark')}</option><option value="GB">UK</option><option value="US">USA</option></select></label>
      <label for={`${id}-period`}>{i18n.tr('Periode', 'Period')}<select id={`${id}-period`} value={timeframe()} onChange={(e) => setTimeframe(e.currentTarget.value as TrendsQuery['timeframe'])} disabled={loading()}><option value="today 3-m">{i18n.tr('3 måneder', '3 months')}</option><option value="today 12-m">{i18n.tr('12 måneder', '12 months')}</option><option value="today 5-y">{i18n.tr('5 år', '5 years')}</option></select></label>
      <button type="submit" disabled={loading() || !props.canManageIntegrations || !validTrendsQuery(query())}>{loading() ? i18n.tr('Henter…', 'Fetching…') : i18n.tr('Undersøk søkeinteresse', 'Research search interest')}</button>
    </form>
    <Show when={terms().trim() && !validTrendsQuery(query())}><p role="alert">{i18n.tr('Bruk ett til fem unike, korte søkeord, adskilt med komma.', 'Use one to five unique, short search terms, separated by commas.')}</p></Show>
    <p>{i18n.tr('Disse søkeordene sendes til Google når du trykker Undersøk. Resultater mellomlagres i 24 timer. Det manuelle søket endrer ikke den automatiske rapporten.', 'These terms are sent to Google when you press Research. Results are cached for 24 hours. Manual research does not change the automatic report.')}</p>
    <Show when={!props.canManageIntegrations}><p>{i18n.tr('En administrator kan starte undersøkelser.', 'An administrator can start research.')}</p></Show>
    <Show when={error()}><p role="alert">{error()}</p></Show>
    </details>
    <Show when={report()}>{(r) => <div class="trends-panel__result">
      <p role="status">{status(r().status)}</p>
      <p>{r().terms.join(' · ')} · {r().geo || i18n.tr('Hele verden', 'Worldwide')} · {periodLabel(r().timeframe)} · {stamp(r().observedAt)}{r().cached ? i18n.tr(' · mellomlagret', ' · cached') : ''}</p>
      <Show when={r().retryAt}><p>{i18n.tr('Tidligste nye forsøk:', 'Earliest next request:')} {stamp(r().retryAt!)}</p></Show>
      <Show when={r().status === 'available'}>
        <dl class="trends-panel__metrics">
          <div><dt>{i18n.tr('Søkeord i målingen', 'Measured terms')}</dt><dd>{r().terms.length}</dd></div>
          <div><dt>{i18n.tr('Tidsvindu', 'Time window')}</dt><dd>{periodLabel(r().timeframe)}</dd></div>
          <div><dt>{i18n.tr('Marked', 'Market')}</dt><dd>{r().geo || i18n.tr('Hele verden', 'Worldwide')}</dd></div>
        </dl>
        <div class="trends-panel__legend"><For each={r().terms}>{(term, index) => <span style={{color: colors[index()]}}>{term}</span>}</For></div>
        <svg class="trends-panel__chart" viewBox="0 0 800 190" role="img" aria-label={i18n.tr('Relativ søkeinteresse fra 0 til 100 over tid', 'Relative search interest from 0 to 100 over time')}>
          <For each={[0, 50, 100]}>{(v) => <><line x1="32" x2="795" y1={170 - v * 1.5} y2={170 - v * 1.5} stroke="currentColor" opacity="0.12"/><text x="2" y={174 - v * 1.5} fill="currentColor" font-size="11">{v}</text></>}</For>
          <For each={r().terms}>{(_, index) => <polyline fill="none" stroke={colors[index()]} stroke-width="2" points={r().rows.map((row, n) => `${32 + n * 760 / Math.max(1, r().rows.length - 1)},${170 - row.values[index()]! * 1.5}`).join(' ')} />}</For>
        </svg>
        <div class="trends-panel__dates"><span>{r().rows[0]?.date}</span><span>{r().rows.at(-1)?.date}</span></div>
        <details><summary>{i18n.tr('Vis måledata', 'Show measurements')} ({r().rows.length})</summary><div class="public-signals__table"><table><caption>{i18n.tr('Relativ indeks · 0–100 innen denne forespørselen', 'Relative index · 0–100 within this request')}</caption><thead><tr><th>{i18n.tr('Dato', 'Date')}</th><For each={r().terms}>{(term) => <th>{term}</th>}</For><th>{i18n.tr('Foreløpig', 'Partial')}</th></tr></thead><tbody><For each={r().rows}>{(row) => <tr><td>{row.date}</td><For each={row.values}>{(v) => <td>{v}</td>}</For><td>{row.partial ? i18n.tr('Ja', 'Yes') : '—'}</td></tr>}</For></tbody></table></div></details>
        <p>{i18n.tr('100 er topp interesse innen samme forespørsel. 0 kan bety for lite datagrunnlag. Separate forespørsler kan ikke sammenlignes som absolutte volum.', '100 is peak interest within this request. 0 may mean insufficient data. Separate requests cannot be compared as absolute volumes.')}</p>
      </Show>
    </div>}</Show>
  </section>
}
