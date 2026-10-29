import { For, Show, createEffect, createMemo, createSignal, createUniqueId, onCleanup } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { comparisonWindows, loadSearchComparison, type SearchComparison } from '../../../shared/api/marketing/search-comparison-client.ts'
import { formatPulseDate } from '../../../shared/api/marketing/pulse-timeline.ts'
import './search-intelligence.css'
import './search-comparison.css'

type Analysis = 'drops' | 'decay'
const PAGE_SIZE = 10

/** Explicit read-only evidence review; a decline is a candidate to investigate, not a causal diagnosis. */
export function SearchComparisonPanel(props: { site: string; websiteId: string; revision: number }) {
  const i18n = useI18n()
  const id = createUniqueId()
  const [analysis, setAnalysis] = createSignal<Analysis>('drops')
  const [attempt, setAttempt] = createSignal(0)
  const [snapshot, setSnapshot] = createSignal<SearchComparison | null>(null)
  const [loading, setLoading] = createSignal(false)
  const [failed, setFailed] = createSignal(false)
  const [showAll, setShowAll] = createSignal(false)
  const [page, setPage] = createSignal(0)

  createEffect(() => {
    const site = props.site, websiteId = props.websiteId
    void props.revision
    const mode = analysis(), run = attempt()
    const controller = new AbortController()
    setSnapshot(null); setFailed(false); setLoading(false); setPage(0); setShowAll(false)
    if (run === 0) return
    setLoading(true)
    const input = { site, websiteId, dimension: mode === 'decay' ? 'page' as const : 'query' as const, ...comparisonWindows(mode === 'decay' ? 90 : 28) }
    void loadSearchComparison(input, controller.signal)
      .then((data) => { if (!controller.signal.aborted) setSnapshot(data) })
      .catch(() => { if (!controller.signal.aborted) setFailed(true) })
      .finally(() => { if (!controller.signal.aborted) setLoading(false) })
    onCleanup(() => controller.abort())
  })

  const connected = () => snapshot()?.status === 'connected'
  const candidates = createMemo(() => (snapshot()?.rows ?? []).filter((row) => row.state === 'compared' && (row.clicksChange ?? 0) < 0 && (analysis() !== 'decay' || (row.impressionsChange ?? 0) < 0)))
  const observations = createMemo(() => showAll() ? snapshot()?.rows ?? [] : candidates())
  const pages = () => Math.max(1, Math.ceil(observations().length / PAGE_SIZE))
  const visible = () => observations().slice(page() * PAGE_SIZE, (page() + 1) * PAGE_SIZE)
  const number = (value: number | undefined) => value === undefined ? '—' : new Intl.NumberFormat(localeDateTime(i18n.locale()), { maximumFractionDigits: 1 }).format(value)
  const change = (value: number | undefined) => value === undefined ? '—' : `${value > 0 ? '+' : ''}${number(value)}`
  const range = (period: SearchComparison['currentPeriod'] | undefined) => period ? `${formatPulseDate(period.startDate, localeDateTime(i18n.locale()), true)} – ${formatPulseDate(period.endDate, localeDateTime(i18n.locale()), true)}` : '—'
  const reason = () => snapshot()?.reason === 'not_configured' ? i18n.tr('Koble til Search Console for å sammenligne perioder.', 'Connect Search Console to compare periods.')
    : snapshot()?.reason === 'property_unverified' ? i18n.tr('Dette nettstedet er ikke tilgjengelig i Search Console-kontoen.', 'This website is not accessible in the Search Console account.')
      : i18n.tr('Search Console er midlertidig utilgjengelig. Kjør analysen igjen senere.', 'Search Console is temporarily unavailable. Run the analysis again later.')

  function chooseAnalysis(value: string) {
    if (value !== 'drops' && value !== 'decay') return
    setAttempt(0); setAnalysis(value)
  }

  return (
    <section class="search-intelligence search-comparison" aria-labelledby={`${id}-title`} data-testid="search-comparison">
      <header class="search-intelligence__header">
        <div><span class="search-intelligence__eyebrow">Google Search Console</span><h2 id={`${id}-title`}>{i18n.tr('Endringer i søketrafikk', 'Search performance changes')}</h2>
          <p>{i18n.tr('Sammenlign to like lange perioder og finn hva som bør undersøkes.', 'Compare equal periods and identify what needs a closer look.')}</p></div>
        <span class="search-intelligence__readonly">{i18n.tr('Kun lesing', 'Read only')}</span>
      </header>
      <Show when={snapshot()?.reportingProperty}>{(property)=><p>{i18n.tr('Rapportkilde','Reporting property')}: <code>{property()}</code></p>}</Show>
      <div class="search-intelligence__toolbar">
        <label class="search-comparison__select">{i18n.tr('Analysetype', 'Analysis type')}
          <select aria-label={i18n.tr('Analysetype', 'Analysis type')} value={analysis()} onChange={(event) => chooseAnalysis(event.currentTarget.value)}>
            <option value="drops">{i18n.tr('Søkeord · siste 28 dager', 'Queries · last 28 days')}</option>
            <option value="decay">{i18n.tr('Innholdsnedgang · siste 90 dager', 'Content decline · last 90 days')}</option>
          </select>
        </label>
        <button class="search-comparison__run" type="button" disabled={loading()} onClick={() => setAttempt((value) => value + 1)}>{loading() ? i18n.tr('Analyserer…', 'Analyzing…') : i18n.tr('Kjør analyse', 'Run analysis')}</button>
      </div>
      <p class="search-intelligence__note">{i18n.tr('Rullerende perioder, uavhengig av kvartalet i Synlighet. Slutter tre dager tilbake for å gi Google tid til å ferdigstille data.', 'Rolling periods, independent of the Visibility quarter. Ends three days ago to allow Google to finalize data.')}</p>

      <Show when={loading()}><p role="status" class="search-intelligence__state">{i18n.tr('Henter begge perioder…', 'Loading both periods…')}</p></Show>
      <Show when={failed()}><p role="alert" class="search-intelligence__state">{i18n.tr('Sammenligningen kunne ikke lastes. Kjør analysen igjen.', 'The comparison could not be loaded. Run the analysis again.')}</p></Show>
      <Show when={snapshot() && !connected()}><p role="status" class="search-intelligence__state">{reason()}</p></Show>
      <Show when={connected()}>
        <dl class="search-comparison__periods">
          <div><dt>{i18n.tr('Denne perioden', 'Current period')}</dt><dd data-testid="comparison-current-period">{range(snapshot()?.currentPeriod)}</dd></div>
          <div><dt>{i18n.tr('Forrige periode', 'Previous period')}</dt><dd>{range(snapshot()?.previousPeriod)}</dd></div>
        </dl>
        <div class="search-comparison__summary">
          <div><span>{i18n.tr('Til gjennomgang', 'For review')}</span><strong>{number(candidates().length)}</strong></div>
          <div><span>{i18n.tr('Sammenlignbare rader', 'Comparable rows')}</span><strong>{number(snapshot()?.comparableRows)}</strong></div>
          <div><span>{i18n.tr('Kun i én periode', 'In one period only')}</span><strong>{number(snapshot()?.unmatchedRows)}</strong></div>
        </div>
        <div class="search-comparison__results-heading">
          <p>{analysis() === 'decay' ? i18n.tr('Kandidater har nedgang i både klikk og visninger. Dette fastslår ikke årsaken.', 'Candidates declined in both clicks and impressions. This does not establish the cause.') : i18n.tr('Søkeord med færre klikk i de sammenlignbare radene. Dette fastslår ikke årsaken.', 'Queries with fewer clicks among comparable rows. This does not establish the cause.')}</p>
          <label><input type="checkbox" checked={showAll()} onChange={(event) => { setShowAll(event.currentTarget.checked); setPage(0) }} />{i18n.tr('Vis alle observasjoner', 'Show all observations')}</label>
        </div>
        <Show when={visible().length > 0} fallback={<p role="status" class="search-intelligence__state">{snapshot()?.comparableRows === 0 && (snapshot()?.unmatchedRows ?? 0) > 0
          ? i18n.tr('Google rapporterte ingen rader i begge perioder. Nedgang kan derfor ikke vurderes. Vis alle observasjoner for å se dataene.', 'Google reported no rows present in both periods. Decline cannot be assessed. Show all observations to inspect the data.')
          : i18n.tr('Ingen rader i denne visningen. Prøv alle observasjoner for å se hele det returnerte utvalget.', 'No rows in this view. Show all observations to inspect the returned sample.')}</p>}>
          <div class="search-intelligence__table-scroll"><table>
            <caption class="search-intelligence__sr-only">{i18n.tr('Sammenligning av søkedata', 'Search comparison')}</caption>
            <thead><tr><th scope="col">{analysis() === 'decay' ? i18n.tr('Nettadresse', 'Page URL') : i18n.tr('Søkeord', 'Query')}</th><th scope="col">{i18n.tr('Klikk før', 'Previous clicks')}</th><th scope="col">{i18n.tr('Klikk nå', 'Current clicks')}</th><th scope="col">{i18n.tr('Endring', 'Change')}</th><th scope="col">{i18n.tr('Visningsendring', 'Impression change')}</th><th scope="col">{i18n.tr('Vurdering', 'Observation')}</th></tr></thead>
            <tbody><For each={visible()}>{(row) => <tr><td class="search-intelligence__key">{row.key}</td><td>{number(row.previous?.clicks)}</td><td>{number(row.current?.clicks)}</td><td>{change(row.clicksChange)}</td><td>{change(row.impressionsChange)}</td><td class="search-comparison__observation">{row.state === 'not_comparable' ? i18n.tr('Kan ikke sammenlignes', 'Not comparable') : row.clicksChangePercent === undefined ? i18n.tr('Ingen prosent ved nullgrunnlag', 'No percentage with zero baseline') : `${change(row.clicksChangePercent)} %`}</td></tr>}</For></tbody>
          </table></div>
        </Show>
        <footer class="search-intelligence__pagination"><span>{number(observations().length)} {i18n.tr('rader', 'rows')} · {i18n.tr('Side', 'Page')} {page() + 1} / {pages()}</span><div>
          <button type="button" disabled={page() === 0} onClick={() => setPage((value) => Math.max(0, value - 1))}>{i18n.tr('Forrige', 'Previous')}</button>
          <button type="button" disabled={page() + 1 >= pages()} onClick={() => setPage((value) => Math.min(pages() - 1, value + 1))}>{i18n.tr('Neste', 'Next')}</button>
        </div></footer>
        <p class="search-intelligence__note search-intelligence__caveat">{i18n.tr('Utvalg av opptil 100 topprader per periode. Anonyme søk og radgrenser gjør at radene ikke dekker all trafikk. Manglende rader er ukjente, ikke null.', 'Sample of up to 100 top rows per period. Anonymized queries and row limits mean rows do not cover all traffic. Missing rows are unknown, not zero.')}
          <Show when={snapshot()?.rowsAtLimit}><span> {i18n.tr('Radgrensen er nådd; flere observasjoner kan finnes.', 'The row limit was reached; more observations may exist.')}</span></Show>
        </p>
      </Show>
    </section>
  )
}
