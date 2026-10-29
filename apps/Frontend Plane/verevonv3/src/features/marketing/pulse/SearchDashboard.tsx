import { ArrowDown, TrendingUp, Megaphone, Search } from 'lucide-solid'
import { For, createSignal, createUniqueId } from 'solid-js'
import { localeDateTime, useI18n } from '@/shared/i18n'
import type { SearchSnapshot } from '@/shared/api/marketing/search-client'
import type { TrendsReport } from '@/shared/api/marketing/trends-client'
import { SearchPositions } from './SearchPositions'
import { SearchIntelligence } from './SearchIntelligence'
import { SearchComparisonPanel } from './SearchComparisonPanel'
import { TrendsPanel } from './TrendsPanel'
import { ReportDisclosure } from './ReportDisclosure'
import './search-dashboard.css'
import './pulse-dashboard.css'

type Props = Parameters<typeof SearchIntelligence>[0] & { websiteId: string; revision: number; canManageIntegrations?: boolean }

/** One presentation of the existing evidence flows; no additional provider requests. */
export function SearchDashboard(props: Props) {
  const i18n = useI18n(), id = createUniqueId()
  const [snapshot, setSnapshot] = createSignal<SearchSnapshot | null>(null)
  const [trends, setTrends] = createSignal<TrendsReport | null>(null)
  const [view, setView] = createSignal<'queries' | 'pages' | 'opportunities'>('queries')
  let evidence: HTMLDetailsElement | undefined, trendDetails: HTMLDetailsElement | undefined, adsDetails: HTMLDetailsElement | undefined
  const available = () => snapshot()?.status === 'connected'
  const count = (value: number | undefined) => value === undefined ? '—' : new Intl.NumberFormat(localeDateTime(i18n.locale())).format(value)
  function openReport(element: HTMLDetailsElement | undefined) {
    if (!element) return
    element.open = true
    element.scrollIntoView({ block: 'start', behavior: 'instant' })
    element.querySelector('summary')?.focus({ preventScroll: true })
  }
  function openView(next: 'queries' | 'pages' | 'opportunities') {
    setView(next)
    openReport(evidence)
    queueMicrotask(() => evidence?.querySelector<HTMLButtonElement>('[role="tab"][aria-selected="true"]')?.focus({ preventScroll: true }))
  }
  return <section class="search-dashboard" aria-labelledby={`${id}-title`} data-testid="search-dashboard">
    <header class="search-dashboard__heading">
      <div><span class="search-dashboard__eyebrow">Google Search · Trends · Ads</span><h2 id={`${id}-title`}>{i18n.tr('Søk. Innsikt. Muligheter.', 'Search. Insight. Opportunity.')}</h2></div>
      <span class="search-dashboard__scope">{i18n.tr('Nettstedets søk og markedets interesse', 'Your site’s searches and market interest')}</span>
    </header>
    <div class="search-dashboard__summary-grid">
      <article class="search-dashboard__tile search-dashboard__coverage">
        <div class="search-dashboard__tile-label"><Search size={16}/>{i18n.tr('Søkeord i rapporten', 'Queries in this report')}</div>
        <strong class="search-dashboard__number" data-testid="search-query-count">{count(available() ? snapshot()?.queries.length : undefined)}</strong>
        <p>{i18n.tr('Slik finner folk nettstedet ditt.', 'How people find your website.')}</p>
        <div class="search-dashboard__query-mosaic" aria-hidden="true"><For each={available() ? snapshot()?.queries.slice(0, 4) : []}>{row => <span>{row.key}</span>}</For></div>
        <div class="search-dashboard__mini-metrics">
          <button type="button" onClick={() => openView('pages')}><strong data-testid="search-page-count">{count(available() ? snapshot()?.pages.length : undefined)}</strong><span>{i18n.tr('Se sidene', 'Explore pages')}</span></button>
          <button type="button" onClick={() => openView('opportunities')}><strong data-testid="search-opportunity-count">{count(available() ? snapshot()?.opportunities.length : undefined)}</strong><span>{i18n.tr('Vurder mulighetene', 'Review opportunities')}</span></button>
        </div>
        <div class="search-dashboard__tile-bottom"><p>{i18n.tr('Rapporterte topprader fra Google. Anonyme søk og radgrenser gjør at utvalget ikke dekker all trafikk.', 'Top rows reported by Google. Anonymous queries and row limits mean this sample does not cover all traffic.')}</p><button type="button" onClick={() => openView('queries')}>{i18n.tr('Utforsk søkeord', 'Explore queries')}<ArrowDown size={15}/></button></div>
      </article>
      <article class="search-dashboard__tile search-dashboard__trends-summary">
        <div class="search-dashboard__tile-label"><TrendingUp size={16}/>Google Trends</div>
        <strong class="search-dashboard__number" data-testid="trends-summary-count">{count(trends()?.status === 'available' ? trends()?.terms.length : undefined)}</strong>
        <p>{i18n.tr('Søkeord med målt markedsinteresse', 'Terms with measured market interest')}</p>
        <p>{trends()?.status === 'available' ? i18n.tr('Relativ interesse, ikke søkevolum.', 'Relative interest, not search volume.') : i18n.tr('Venter på en tilgjengelig rapport.', 'Waiting for an available report.')}</p>
        <div class="search-dashboard__tile-bottom"><button type="button" onClick={() => openReport(trendDetails)}>{i18n.tr('Se søkeinteresse', 'Explore interest')}<ArrowDown size={15}/></button></div>
      </article>
      <article class="search-dashboard__tile search-dashboard__ads-summary">
        <div class="search-dashboard__tile-label"><Megaphone size={16}/>Google Ads</div>
        <strong class="search-dashboard__number">—</strong><p>{i18n.tr('Rapportering kommer senere', 'Reporting is coming later')}</p>
        <p>{i18n.tr('Forbruk, klikk og konverteringer vises når annonserapportering er tilgjengelig.', 'Spend, clicks and conversions will appear when ad reporting is available.')}</p>
        <div class="search-dashboard__tile-bottom"><button type="button" onClick={() => openReport(adsDetails)}>{i18n.tr('Se annonsestatus', 'View ad status')}<ArrowDown size={15}/></button></div>
      </article>
    </div>
    <SearchPositions snapshot={snapshot()} onExplore={() => openView('queries')}/>
    <ReportDisclosure ref={element => { evidence = element }} source="Google Search Console" title={i18n.tr('Søkeinnsikt og endringer', 'Search insights and changes')} testId="search-report-disclosure">
      <SearchIntelligence {...props} view={view()} onViewChange={setView} onSnapshot={setSnapshot}/>
      <SearchComparisonPanel site={props.site} websiteId={props.websiteId} revision={props.revision}/>
    </ReportDisclosure>
    <ReportDisclosure ref={element => { trendDetails = element }} source="Google Trends" title={i18n.tr('Søkeinteresse', 'Search interest')} testId="trends-report-disclosure">
      <TrendsPanel site={props.site} websiteId={props.websiteId} revision={props.revision} refreshKey={props.refreshKey} canManageIntegrations={props.canManageIntegrations} onReport={setTrends}/>
    </ReportDisclosure>
    <ReportDisclosure ref={element => { adsDetails = element }} source="Google Ads" title={i18n.tr('Annonsering', 'Advertising')} testId="ads-report-disclosure">
      <div class="search-dashboard__ads-status"><h3>{i18n.tr('Annonserapportering er ikke tilgjengelig ennå', 'Advertising reports are not available yet')}</h3><p>{i18n.tr('Google Ads-rapportering er ikke implementert i Synlighet ennå. Ukjente kostnader og resultater vises som —, ikke null.', 'Google Ads reporting is not implemented in Visibility yet. Unknown spend and results are shown as —, not zero.')}</p><a href="/settings/integrations">{i18n.tr('Administrer integrasjoner', 'Manage integrations')}</a></div>
    </ReportDisclosure>
  </section>
}
