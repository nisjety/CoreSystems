import { ChevronLeft, ChevronRight, Search } from 'lucide-solid'
import { For, Show, createEffect, createMemo, createSignal, createUniqueId, onCleanup } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import type { PulseQuarter } from '../../../shared/api/marketing/pulse-client.ts'
import { loadSearch, type Opportunity, type OpportunityType, type SearchRow, type SearchSnapshot } from '../../../shared/api/marketing/search-client.ts'
import { formatPulseDate } from '../../../shared/api/marketing/pulse-timeline.ts'
import { saveMarketingOpportunity } from '../../../shared/api/marketing/marketing-work-client.ts'
import './search-intelligence.css'

type SearchTab = 'queries' | 'pages' | 'opportunities'
const TABS: SearchTab[] = ['queries', 'pages', 'opportunities']
const PAGE_SIZE = 10

/** Search evidence is read-only. Suggestions are not automatically saved or acted on. */
export function SearchIntelligence(props: { site: string; quarter: PulseQuarter; websiteId?: string; revision?: number; refreshKey?: number; savedFindings?: string[]; onTaskSaved?: () => void; onSnapshot?: (snapshot: SearchSnapshot | null) => void; view?: SearchTab; onViewChange?: (view: SearchTab) => void }) {
  const i18n = useI18n()
  const id = createUniqueId()
  const [snapshot, setSnapshot] = createSignal<SearchSnapshot | null>(null)
  const [loading, setLoading] = createSignal(true)
  const [failed, setFailed] = createSignal(false)
  const [retry, setRetry] = createSignal(0)
  const [tab, setTab] = createSignal<SearchTab>('queries')
  const [filter, setFilter] = createSignal('')
  const [page, setPage] = createSignal(0)
  const [savingKey, setSavingKey] = createSignal<string | null>(null)
  const [savedThisSession, setSavedThisSession] = createSignal<ReadonlySet<string>>(new Set())
  const [saveError, setSaveError] = createSignal('')
  let saveController: AbortController | undefined

  createEffect(() => {
    const site = props.site
    const quarter = props.quarter
    const websiteId = props.websiteId
    const onSnapshot = props.onSnapshot
    void props.revision
    void props.refreshKey
    retry()
    const controller = new AbortController()
    setSnapshot(null)
    onSnapshot?.(null)
    setFailed(false)
    setLoading(true)
    setPage(0)
    setFilter('')
    saveController?.abort(); setSavingKey(null); setSaveError('')
    setSavedThisSession(new Set<string>())
    void loadSearch(quarter, site, controller.signal, websiteId)
      .then((value) => { if (!controller.signal.aborted) { setSnapshot(value); onSnapshot?.(value) } })
      .catch(() => { if (!controller.signal.aborted) setFailed(true) })
      .finally(() => { if (!controller.signal.aborted) setLoading(false) })
    onCleanup(() => { controller.abort(); saveController?.abort() })
  })

  createEffect(() => { if (props.view) { setTab(props.view); setPage(0); setFilter('') } })

  const connected = () => snapshot()?.status === 'connected'
  const rows = createMemo(() => snapshot()?.[tab()] ?? [])
  const filtered = createMemo(() => {
    const needle = filter().trim().toLocaleLowerCase(localeDateTime(i18n.locale()))
    return rows().filter((row) => row.key.toLocaleLowerCase(localeDateTime(i18n.locale())).includes(needle))
  })
  const pages = () => Math.max(1, Math.ceil(filtered().length / PAGE_SIZE))
  const visible = () => filtered().slice(page() * PAGE_SIZE, (page() + 1) * PAGE_SIZE)
  const count = (value: number) => new Intl.NumberFormat(localeDateTime(i18n.locale())).format(value)
  const position = (value: number) => new Intl.NumberFormat(localeDateTime(i18n.locale()), { minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(value)
  const ctr = (value: number) => new Intl.NumberFormat(localeDateTime(i18n.locale()), { style: 'percent', maximumFractionDigits: 2 }).format(value)
  const tabLabel = (value: SearchTab) => value === 'queries' ? i18n.tr('Søkeord', 'Queries') : value === 'pages' ? i18n.tr('Sider', 'Pages') : i18n.tr('Muligheter', 'Opportunities')
  const period = () => {
    const range = snapshot()?.period
    return range ? `${formatPulseDate(range.startDate, localeDateTime(i18n.locale()), true)} – ${formatPulseDate(range.endDate, localeDateTime(i18n.locale()), true)}` : '—'
  }
  const opportunityLabel = (type: OpportunityType) => ({
    snippet_review: i18n.tr('Vurder søkeutdraget', 'Review search snippet'),
    title_meta: i18n.tr('Vurder tittel og metabeskrivelse', 'Review title and meta description'),
    title_test: i18n.tr('Test en tydeligere tittel', 'Test a clearer title'),
    content_expansion: i18n.tr('Vurder å utvide innholdet', 'Consider expanding content'),
  })[type]
  const reason = () => {
    if (snapshot()?.reason === 'snapshot_pending' || snapshot()?.reason === 'snapshot_expired') return i18n.tr('Lagret søkerapport mangler eller er utløpt. Bruk innhentingspanelet for å hente nye data.', 'The saved search snapshot is missing or expired. Use the collection panel to collect new data.')
    if (snapshot()?.reason === 'authorization_required') return i18n.tr('Search Console-tilgangen må kontrolleres i Integration Core.', 'Search Console authorization needs attention in Integration Core.')
    if (snapshot()?.reason === 'property_unverified') return i18n.tr('Nettstedet er ikke verifisert i Search Console-kontoen.', 'This site is not verified in the Search Console account.')
    if (snapshot()?.reason === 'not_configured') return i18n.tr('Google Search Console er ikke koblet til ennå.', 'Google Search Console is not connected yet.')
    return i18n.tr('Søkeinnsikt er midlertidig utilgjengelig.', 'Search intelligence is temporarily unavailable.')
  }
  function changeTab(value: SearchTab) { setTab(value); props.onViewChange?.(value); setPage(0); setFilter('') }
  function tabKey(event: KeyboardEvent, current: SearchTab) {
    const offset = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0
    if (offset === 0 && event.key !== 'Home' && event.key !== 'End') return
    event.preventDefault()
    const next = event.key === 'Home' ? TABS[0]! : event.key === 'End' ? TABS.at(-1)! : TABS[(TABS.indexOf(current) + offset + TABS.length) % TABS.length]!
    changeTab(next)
    document.getElementById(`${id}-${next}`)?.focus()
  }
  const opportunity = (row: SearchRow) => opportunityLabel((row as Opportunity).type)
  const findingKey = (row: SearchRow) => `${row.key}:${(row as Opportunity).type}`
  const isSaved = (row: SearchRow) => (props.savedFindings ?? []).includes(findingKey(row)) || savedThisSession().has(findingKey(row))
  async function save(row: SearchRow) {
    const websiteId = props.websiteId; const revision = props.revision
    if (!websiteId || !revision || savingKey()) return
    const contextKey = `${websiteId}|${props.site}|${revision}|${props.quarter}`
    const key = findingKey(row)
    const controller = new AbortController(); saveController = controller
    setSavingKey(key); setSaveError('')
    try {
      await saveMarketingOpportunity({ websiteId, site: props.site, websiteRevision: revision, quarter: props.quarter, query: row.key }, controller.signal)
      if (controller.signal.aborted || `${props.websiteId}|${props.site}|${props.revision}|${props.quarter}` !== contextKey) return
      setSavedThisSession((current) => new Set(current).add(key))
      props.onTaskSaved?.()
    } catch { if (!controller.signal.aborted && `${props.websiteId}|${props.site}|${props.revision}|${props.quarter}` === contextKey) setSaveError(i18n.tr('Forslaget kunne ikke lagres. Kontroller tilkoblingen og prøv igjen.', 'Could not save this suggestion. Check the connection and try again.')) }
    finally { if (saveController === controller) setSavingKey(null) }
  }

  return (
    <section class="search-intelligence" aria-labelledby={`${id}-title`} data-testid="search-intelligence">
      <header class="search-intelligence__header">
        <div>
          <span class="search-intelligence__eyebrow">Google Search Console</span>
          <h2 id={`${id}-title`}>{i18n.tr('Søkeinnsikt', 'Search intelligence')}</h2>
          <p data-testid="search-period">{period()} · {props.quarter}</p>
          <Show when={snapshot()?.reportingProperty}>{(property)=><p>{i18n.tr('Rapportkilde','Reporting property')}: <code>{property()}</code></p>}</Show>
        </div>
        <span class="search-intelligence__readonly">{i18n.tr('Kun lesing', 'Read only')}</span>
      </header>

      <Show when={!loading()} fallback={<p role="status" class="search-intelligence__state">{i18n.tr('Laster søkeinnsikt…', 'Loading search intelligence…')}</p>}>
        <Show when={!failed()} fallback={<div role="alert" class="search-intelligence__state"><p>{i18n.tr('Søkeinnsikt kunne ikke lastes. Øvrige Synlighet-data påvirkes ikke.', 'Search intelligence could not be loaded. Other Visibility data is unaffected.')}</p><button type="button" onClick={() => setRetry((value) => value + 1)}>{i18n.tr('Prøv igjen', 'Try again')}</button></div>}>
          <Show when={connected()} fallback={<p role="status" class="search-intelligence__state">{reason()}</p>}>
            <div class="search-intelligence__toolbar">
              <div role="tablist" aria-label={i18n.tr('Søkeinnsiktvisning', 'Search intelligence view')}>
                <For each={TABS}>{(value) => <button id={`${id}-${value}`} role="tab" type="button" aria-selected={tab() === value} aria-controls={`${id}-panel`} tabIndex={tab() === value ? 0 : -1} onClick={() => changeTab(value)} onKeyDown={(event) => tabKey(event, value)}>{tabLabel(value)}<span>{count(snapshot()?.[value].length ?? 0)}</span></button>}</For>
              </div>
              <label class="search-intelligence__filter"><Search size={14} aria-hidden="true" /><span class="search-intelligence__sr-only">{i18n.tr('Filtrer søkeinnsikt', 'Filter search intelligence')}</span><input type="search" placeholder={i18n.tr('Filtrer rader…', 'Filter rows…')} value={filter()} onInput={(event) => { setFilter(event.currentTarget.value); setPage(0) }} /></label>
            </div>
            <div id={`${id}-panel`} role="tabpanel" aria-labelledby={`${id}-${tab()}`}>
              <Show when={tab() === 'opportunities'}><p class="search-intelligence__note">{i18n.tr('Regelbaserte forslag fra søkeordene nedenfor – ikke lagrede oppgaver eller garantert vekst. Minst 100 visninger, CTR under 5 % og posisjon 1–20.', 'Rule-based suggestions from the returned queries – not saved tasks or guaranteed growth. At least 100 impressions, CTR below 5%, and position 1–20.')} <a href="/engagement/visibility/groups">{i18n.tr('Flere muligheter: nær første side, lav CTR, tekniske funn og interne lenker → SEO-grupper', 'More opportunities: near page one, low CTR, technical findings and internal links → SEO groups')}</a></p></Show>
              <Show when={visible().length > 0} fallback={<p role="status" class="search-intelligence__state">{filter() ? i18n.tr('Ingen treff i radene som er hentet.', 'No matching rows in the fetched results.') : tab() === 'queries' ? i18n.tr('Ingen søkeord rapportert for perioden.', 'No query rows reported for this period.') : tab() === 'pages' ? i18n.tr('Ingen sider rapportert for perioden.', 'No page rows reported for this period.') : i18n.tr('Ingen søkeord oppfyller reglene i dette utvalget.', 'No queries meet the rules in this sample.')}</p>}>
                <div class="search-intelligence__table-scroll"><table>
                  <caption class="search-intelligence__sr-only">{tabLabel(tab())} · {period()}</caption>
                  <thead><tr><th scope="col">{tab() === 'pages' ? i18n.tr('Nettadresse', 'Page URL') : i18n.tr('Søkeord', 'Query')}</th><Show when={tab() === 'opportunities'}><th scope="col">{i18n.tr('Forslag', 'Suggestion')}</th></Show><th scope="col">{i18n.tr('Klikk', 'Clicks')}</th><th scope="col">{i18n.tr('Visninger', 'Impressions')}</th><th scope="col">CTR</th><th scope="col">{i18n.tr('Snittposisjon', 'Avg. position')}</th><Show when={tab() === 'opportunities' && props.websiteId && props.revision}><th scope="col">{i18n.tr('Arbeid', 'Work')}</th></Show></tr></thead>
                  <tbody><For each={visible()}>{(row) => <tr><td class="search-intelligence__key">{row.key}</td><Show when={tab() === 'opportunities'}><td class="search-intelligence__suggestion">{opportunity(row)}</td></Show><td>{count(row.clicks)}</td><td>{count(row.impressions)}</td><td>{ctr(row.ctr)}</td><td>{position(row.position)}</td><Show when={tab() === 'opportunities' && props.websiteId && props.revision}><td class="search-intelligence__work-action"><button type="button" disabled={Boolean(savingKey()) || isSaved(row)} onClick={() => void save(row)}>{isSaved(row) ? i18n.tr('Lagret', 'Saved') : savingKey() === findingKey(row) ? i18n.tr('Lagrer…', 'Saving…') : i18n.tr('Legg i oppgavelisten', 'Add to backlog')}</button></td></Show></tr>}</For></tbody>
                </table></div>
              </Show>
              <footer class="search-intelligence__pagination"><span>{count(filtered().length)} {i18n.tr('rader', 'rows')} · {i18n.tr('Side', 'Page')} {page() + 1} / {pages()}</span><div><button type="button" aria-label={i18n.tr('Forrige side', 'Previous page')} disabled={page() === 0} onClick={() => setPage((value) => Math.max(0, value - 1))}><ChevronLeft size={15} /></button><button type="button" aria-label={i18n.tr('Neste side', 'Next page')} disabled={page() + 1 >= pages()} onClick={() => setPage((value) => Math.min(pages() - 1, value + 1))}><ChevronRight size={15} /></button></div></footer>
            </div>
            <Show when={saveError()}><p class="search-intelligence__state" role="alert">{saveError()}</p></Show>
            <p class="search-intelligence__note search-intelligence__caveat">{i18n.tr('Opptil 100 topprader per visning fra Google for perioden. Anonyme søk og radgrenser betyr at summen kan avvike fra Synlighet-totalene. Google kan ha rapporteringsforsinkelse.', 'Up to 100 top rows per view from Google for this period. Anonymized queries and row limits mean these rows may not sum to Visibility totals. Google reporting may be delayed.')}
              <Show when={snapshot()?.queriesAtLimit || snapshot()?.pagesAtLimit}><span> {i18n.tr('Radgrensen er nådd; flere rader kan finnes.', 'The row limit was reached; more rows may exist.')}</span></Show>
            </p>
          </Show>
        </Show>
      </Show>
    </section>
  )
}
