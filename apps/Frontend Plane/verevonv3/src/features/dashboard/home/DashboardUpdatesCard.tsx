import { createEffect, createMemo, createSignal, For, Match, Show, Switch } from 'solid-js'
import { ArrowUpRight, Maximize2, Newspaper, RefreshCw, X } from '@/shared/icons'
import { homeUpdatesActionSchema, type HomeUpdatesFeed } from '@/shared/actions/home-updates'
import { loadAquatiqUpdates, type AquatiqUpdates } from '@/shared/api/home-updates-client'
import { formatDerivedTrafficMetadata, formatTrafficObservation, loadDashboardInformationSnapshot, loadNews, loadTraffic, newsCategoryOptions, type InformationNewsPayload, type InformationTrafficPayload } from '@/shared/api/information-client'
import { getSession } from '@/shared/session/session-store'
import { useI18n } from '@/shared/i18n'
import { dashboardCards, type DashboardCard } from './dashboard-cards'
import { useHomeLayout } from './home-layout-state'
import './dashboard-updates.css'

type FeedData = { kind: 'news'; payload: InformationNewsPayload } | { kind: 'traffic'; payload: InformationTrafficPayload; nearby: boolean } | { kind: 'aquatiq'; payload: AquatiqUpdates }

export function DashboardUpdatesCard(props: { onPrompt: (card: DashboardCard) => void }) {
  const i18n = useI18n()
  const session = getSession()
  // The card remounts with the Home tab, so the feed and topic are remembered
  // per browser and account instead of resetting to News / All each time.
  const [layout, updateLayout] = useHomeLayout()
  const feed = () => layout().feed
  const category = () => layout().category
  const [site, setSite] = createSignal('all')
  const [revision, setRevision] = createSignal(0)
  const [data, setData] = createSignal<FeedData | null>(null)
  const [loading, setLoading] = createSignal(true)
  const [error, setError] = createSignal(false)
  const [expanded, setExpanded] = createSignal(false)
  let dialog: HTMLDialogElement | undefined
  let expandButton: HTMLButtonElement | undefined
  const feeds: HomeUpdatesFeed[] = ['news', 'traffic', 'aquatiq']
  const label = (value: HomeUpdatesFeed) => value === 'news' ? i18n.tr('Nyheter', 'News') : value === 'traffic' ? i18n.tr('Trafikk', 'Traffic') : 'Aquatiq'
  const title = () => feed() === 'news' ? i18n.tr('Norske nyheter', 'Norwegian news') : feed() === 'traffic' ? i18n.tr('Trafikk i området', 'Traffic nearby') : i18n.tr('Fra Aquatiq', 'From Aquatiq')
  const subtitle = () => feed() === 'news' ? i18n.tr('Siste fra norske redaksjoner', 'Latest from Norwegian publishers') : feed() === 'traffic' ? 'Statens vegvesen' : i18n.tr('Synkroniserte SharePoint-sider', 'Synced SharePoint pages')
  const selectFeed = (value: HomeUpdatesFeed) => {
    const action = homeUpdatesActionSchema.parse({ type: 'home.updates.select', feed: value })
    updateLayout({ feed: action.feed })
  }
  createEffect(
    () => ({ feed: feed(), category: category(), revision: revision(), org: session.activeOrg?.id, user: session.user?.id }),
    (selection) => {
      const controller = new AbortController()
      setData(null)
      setError(false)
      setLoading(true)
      const load = async (): Promise<FeedData> => {
        if (selection.feed === 'aquatiq') {
          if (!selection.org || !selection.user) throw new Error('No workspace')
          return { kind: 'aquatiq', payload: await loadAquatiqUpdates(selection.org, controller.signal) }
        }
        if (selection.feed === 'news') {
          if (selection.category !== 'all' || selection.revision > 0) return { kind: 'news', payload: await loadNews(selection.category) }
          const snapshot = await loadDashboardInformationSnapshot()
          if (!snapshot.news || snapshot.newsError) throw new Error('News unavailable')
          return { kind: 'news', payload: snapshot.news }
        }
        const traffic = await loadTraffic()
        return { kind: 'traffic', payload: traffic.data, nearby: traffic.usingLiveLocation }
      }
      void load().then((result) => { if (!controller.signal.aborted) setData(result) })
        .catch(() => { if (!controller.signal.aborted) setError(true) })
        .finally(() => { if (!controller.signal.aborted) setLoading(false) })
      return () => controller.abort()
    },
  )
  createEffect(() => ({ org: session.activeOrg?.id, user: session.user?.id }), () => { setSite('all') })
  createEffect(expanded, (open) => {
    if (open) { if (dialog && !dialog.open) dialog.showModal() }
    else if (dialog?.open) { dialog.close(); expandButton?.focus() }
  })
  const sites = createMemo(() => {
    const current = data()
    return current?.kind === 'aquatiq' ? [...new Set(current.payload.pages.map((page) => page.site))] : []
  })
  const date = (value?: string) => {
    if (!value || Number.isNaN(Date.parse(value))) return ''
    return new Date(value).toLocaleDateString(i18n.locale() === 'no' ? 'nb-NO' : 'en-GB', { day: 'numeric', month: 'short' })
  }
  const Switcher = () => <div class="home-updates__switcher" role="group" aria-label={i18n.tr('Velg oppdateringer', 'Choose updates')}><For each={feeds}>{(value) => <button type="button" aria-pressed={feed() === value ? 'true' : 'false'} onClick={() => selectFeed(value)}>{label(value)}</button>}</For></div>
  const Refresh = () => <button type="button" class="home-updates__icon" disabled={loading()} aria-label={i18n.tr('Oppdater', 'Refresh updates')} onClick={() => setRevision((value) => value + 1)}><RefreshCw size={15} /></button>
  const Filters = () => <>
    <Show when={feed() === 'news'}><select aria-label={i18n.tr('Filtrer nyheter', 'Filter news')} value={category()} onChange={(event) => updateLayout({ category: event.currentTarget.value })}><For each={newsCategoryOptions}>{(option) => <option value={option.value}>{option.value === 'all' ? i18n.tr('Alle emner', 'All topics') : option.value === 'General' ? i18n.tr('Generelt', 'General') : option.value === 'Business' ? i18n.tr('Næring', 'Business') : i18n.tr('Teknologi', 'Technology')}</option>}</For></select></Show>
    <Show when={feed() === 'aquatiq' && sites().length > 1}><select aria-label={i18n.tr('SharePoint-nettsted', 'SharePoint site')} value={site()} onChange={(event) => setSite(event.currentTarget.value)}><option value="all">{i18n.tr('Alle nettsteder', 'All sites')}</option><For each={sites()}>{(value) => <option value={value}>{new URL(value).pathname.replace('/sites/', '') || new URL(value).hostname}</option>}</For></select></Show>
  </>
  const Feed = (view: { full?: boolean }) => <div class="home-updates__feed" aria-busy={loading() ? 'true' : 'false'}>
    <Show when={loading()}><div class="home-updates__state" role="status">{i18n.tr('Henter oppdateringer …', 'Loading updates…')}</div></Show>
    <Show when={error()}><div class="home-updates__state" role="status"><strong>{i18n.tr('Kunne ikke hente oppdateringer', 'Couldn’t load updates')}</strong><p>{feed() === 'aquatiq' ? i18n.tr('SharePoint-sidene er ikke tilgjengelige akkurat nå.', 'SharePoint pages aren’t available right now.') : i18n.tr('Prøv igjen om et øyeblikk.', 'Please try again in a moment.')}</p><button type="button" class="home-quiet-button" onClick={() => setRevision((value) => value + 1)}>{i18n.tr('Prøv igjen', 'Try again')}</button></div></Show>
    <Show when={data()}>{(current) => <Switch>
      <Match when={current().kind === 'news'}>{() => {
        const payload = () => { const result = current(); return result.kind === 'news' ? result.payload : null }
        return <Show when={payload()?.articles.length} fallback={<div class="home-updates__state">{i18n.tr('Ingen nyheter i dette emnet ennå.', 'No news in this topic yet.')}</div>}><For each={payload()?.articles.slice(0, view.full ? undefined : 3)}>{(article) => <a class="home-updates__row" href={article.link} target="_blank" rel="noopener noreferrer"><span><strong>{article.title}</strong><small>{article.source}<Show when={article.publishDate}> · {date(article.publishDate)}</Show></small><Show when={view.full && article.description}><p>{article.description}</p></Show></span><ArrowUpRight size={14} /></a>}</For></Show>
      }}</Match>
      <Match when={current().kind === 'traffic'}>{() => {
        const payload = () => { const result = current(); return result.kind === 'traffic' ? result : null }
        const locale = () => i18n.locale() === 'no' ? 'nb-NO' : 'en-US'
        return <><p class="home-updates__source">{payload()?.nearby ? i18n.tr('Nær din posisjon', 'Near your location') : i18n.tr('Oslo-området · standard plassering', 'Oslo area · default location')}</p><Show when={payload()?.payload.data.length} fallback={<div class="home-updates__state">{i18n.tr('Ingen trafikkmålinger tilgjengelig.', 'No traffic observations available.')}</div>}><For each={payload()?.payload.data.slice(0, view.full ? undefined : 2)}>{(station) => <div class="home-updates__row home-updates__traffic"><span><strong>{station.name}</strong><small>{formatDerivedTrafficMetadata(station, locale())}</small></span><span class="home-updates__observation"><strong>{formatTrafficObservation(station.averageSpeed, locale())}</strong><small>{formatTrafficObservation(station.trafficVolume, locale())}</small></span></div>}</For></Show></>
      }}</Match>
      <Match when={current().kind === 'aquatiq'}>{() => {
        const payload = () => { const result = current(); return result.kind === 'aquatiq' ? result.payload : null }
        const pages = () => payload()?.pages.filter((page) => site() === 'all' || page.site === site()) ?? []
        return <><Show when={pages().length} fallback={<div class="home-updates__state"><Newspaper size={22} /><strong>{i18n.tr('Ingen SharePoint-sider her ennå', 'No SharePoint pages here yet')}</strong><p>{i18n.tr('Sider som er synkronisert og delt med deg, vises her.', 'Pages synced and shared with you will appear here.')}</p><a link href="/knowledge">{i18n.tr('Åpne kunnskapsbasen', 'Open knowledge base')}<ArrowUpRight size={13} /></a></div>}><For each={pages().slice(0, view.full ? undefined : 3)}>{(page) => <a class="home-updates__row" href={page.url} target="_blank" rel="noopener noreferrer"><span><strong>{page.title}</strong><small>SharePoint<Show when={page.updatedAt}> · {i18n.tr('Oppdatert', 'Updated')} {date(page.updatedAt)}</Show></small></span><ArrowUpRight size={14} /></a>}</For></Show><Show when={view.full && payload()?.hasMore}><p class="home-updates__source">{i18n.tr('Viser nylig synkroniserte sider. Finn flere i kunnskapsbasen.', 'Showing recently synced pages. Find more in the knowledge base.')}</p></Show></>
      }}</Match>
    </Switch>}</Show>
  </div>
  const ask = () => {
    const card = dashboardCards.find((item) => item.id === feed())
    if (card) { setExpanded(false); props.onPrompt(card) }
  }
  return <section class="home-overview-card home-updates" aria-labelledby="home-updates-title">
    <header class="home-updates__header"><div><span class="home-eyebrow">{i18n.tr('Hold deg oppdatert', 'Stay up to date')}</span><h2 id="home-updates-title">{title()}</h2></div><Switcher /></header>
    <div class="home-updates__toolbar"><span>{subtitle()}</span><Refresh /></div>
    <Feed />
    <footer><Show when={feed() !== 'aquatiq'} fallback={<a link href="/knowledge">{i18n.tr('Kunnskapsbase', 'Knowledge base')}<ArrowUpRight size={13} /></a>}><button type="button" onClick={ask}><Newspaper size={13} />{i18n.tr('Spør Verevon', 'Ask Verevon')}</button></Show><button ref={expandButton} type="button" onClick={() => setExpanded(true)} aria-haspopup="dialog"><span>{i18n.tr('Åpne oppdateringer', 'Open updates')}</span><Maximize2 size={14} /></button></footer>
    <dialog ref={dialog} class="home-updates-dialog" aria-labelledby="home-updates-dialog-title" onCancel={() => setExpanded(false)} onClose={() => setExpanded(false)}>
      <Show when={expanded()}><header class="home-updates-dialog__header"><div><span class="home-eyebrow">{i18n.tr('Dine oppdateringer', 'Your updates')}</span><h2 id="home-updates-dialog-title">{i18n.tr('I og rundt arbeidsdagen', 'In and around your workday')}</h2></div><button type="button" class="home-updates__icon" aria-label={i18n.tr('Lukk oppdateringer', 'Close updates')} onClick={() => setExpanded(false)}><X size={19} /></button></header><div class="home-updates-dialog__controls"><Switcher /><Refresh /></div><div class="home-updates-dialog__heading"><div><h3>{title()}</h3><p>{subtitle()}</p></div><Filters /></div><Feed full /></Show>
    </dialog>
  </section>
}
