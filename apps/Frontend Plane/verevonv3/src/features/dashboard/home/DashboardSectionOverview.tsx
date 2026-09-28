import { createEffect, createSignal, For, Show } from 'solid-js'
import { ArrowUpRight, RefreshCw } from '@/shared/icons'
import { getSession } from '@/shared/session/session-store'
import { useI18n } from '@/shared/i18n'
import { homeOverviewLabel } from '@/shared/actions/home-overview'
import { homeSectionContent, loadHomeSection, type HomeSectionData, type HomeWorkSection } from './home-section-data'

export function DashboardSectionOverview(props: { section: HomeWorkSection }) {
  const i18n = useI18n()
  const session = getSession()
  const [data, setData] = createSignal<HomeSectionData | null>(null)
  const [loading, setLoading] = createSignal(true)
  const [failed, setFailed] = createSignal(false)
  const [refresh, setRefresh] = createSignal(0)
  const content = () => homeSectionContent(props.section, i18n.tr)
  const label = () => homeOverviewLabel(props.section, i18n.tr)
  const hasWorkspace = () => Boolean(session.user?.id && session.activeOrg?.id)

  createEffect(
    () => ({ section: props.section, org: session.activeOrg?.id, user: session.user?.id, refresh: refresh(), locale: i18n.locale() }),
    ({ section, org, user }) => {
      let cancelled = false
      setData(null)
      setFailed(false)
      setLoading(Boolean(org && user))
      if (org && user) void loadHomeSection(section, org, i18n.tr)
        .then((result) => { if (!cancelled) setData(result) })
        .catch(() => { if (!cancelled) setFailed(true) })
        .finally(() => { if (!cancelled) setLoading(false) })
      return () => { cancelled = true }
    },
  )
  const dateLabel = (value: string) => {
    const date = new Date(value)
    return Number.isNaN(date.getTime()) ? '' : date.toLocaleDateString(i18n.locale() === 'no' ? 'nb-NO' : 'en-GB', { day: 'numeric', month: 'short' })
  }

  return (
    <section class="home-section-grid" aria-label={i18n.tr(`${label()} oversikt`, `${label()} overview`)} aria-busy={loading() ? 'true' : 'false'}>
      <section class="home-overview-card home-section-work" aria-labelledby="home-section-work-title">
        <header class="home-overview-card__header">
          <div><span class="home-eyebrow">{i18n.tr('Arbeidsområdet ditt', 'Your workspace')}</span><h2 id="home-section-work-title">{content().title}</h2></div>
          <button type="button" class="home-quiet-button" disabled={loading() || !hasWorkspace()} aria-label={i18n.tr('Oppdater oversikt', 'Refresh overview')} onClick={() => setRefresh((value) => value + 1)}><RefreshCw size={16} /></button>
        </header>
        <Show when={hasWorkspace()} fallback={<div class="home-section-state" role="status">{i18n.tr('Velg et arbeidsområde for å se oversikten.', 'Select a workspace to see this overview.')}</div>}>
          <Show when={!loading()} fallback={<div class="home-section-state" role="status">{i18n.tr('Henter oversikten …', 'Loading your overview…')}</div>}>
            <Show when={!failed()} fallback={<div class="home-section-state" role="status"><strong>{i18n.tr('Oversikten er ikke tilgjengelig akkurat nå.', 'This overview is unavailable right now.')}</strong><p>{i18n.tr('Prøv igjen, eller åpne arbeidsområdet direkte.', 'Try again, or open the workspace directly.')}</p><button type="button" class="home-quiet-button" onClick={() => setRefresh((value) => value + 1)}><RefreshCw size={14} />{i18n.tr('Prøv igjen', 'Try again')}</button></div>}>
              <Show when={data()?.items.length} fallback={<div class="home-section-state"><strong>{content().empty}</strong><p>{i18n.tr('Arbeid du har tilgang til, vises her når det er tilgjengelig.', 'Work you have access to will appear here when it is available.')}</p></div>}>
                <div class="home-section-list"><For each={data()?.items.slice(0, 6)}>{(item) => (
                  <div class="home-section-row">
                    <div class="home-section-row__copy"><Show when={item.href} fallback={<strong>{item.title}</strong>}><a link href={item.href}><strong>{item.title}</strong></a></Show><Show when={item.detail}><p>{item.detail}</p></Show></div>
                    <div class="home-section-row__meta"><Show when={item.value}><strong>{item.value}</strong></Show><Show when={item.status}><span>{item.status?.replaceAll('_', ' ')}</span></Show><Show when={item.updatedAt && dateLabel(item.updatedAt)}><time datetime={item.updatedAt}>{dateLabel(item.updatedAt!)}</time></Show></div>
                    <Show when={item.href}><a link href={item.href} aria-label={i18n.tr(`Åpne ${item.title}`, `Open ${item.title}`)}><ArrowUpRight size={15} /></a></Show>
                  </div>
                )}</For></div>
              </Show>
            </Show>
          </Show>
        </Show>
        <footer class="home-section-footer"><a link href={content().href}>{i18n.tr(`Åpne ${label()}`, `Open ${label()}`)}<ArrowUpRight size={15} /></a><Show when={(data()?.items.length ?? 0) > 6}><span>{i18n.tr(`Viser 6 av ${data()?.items.length}`, `Showing 6 of ${data()?.items.length}`)}</span></Show></footer>
      </section>
      <aside class="home-section-aside" aria-label={i18n.tr('Oppsummering og snarveier', 'Summary and shortcuts')}>
        <Show when={data()}>{(result) => <section class="home-overview-card home-section-summary"><span class="home-eyebrow">{i18n.tr('Et overblikk', 'At a glance')}</span><dl><For each={result().metrics}>{(metric) => <div><dt>{metric.label}</dt><dd>{metric.value}</dd></div>}</For></dl><Show when={result().note}><p>{result().note}</p></Show></section>}</Show>
        <section class="home-overview-card home-destinations"><span class="home-eyebrow">{i18n.tr('Fortsett arbeidet', 'Continue your work')}</span><h2>{i18n.tr(`Utforsk ${label()}`, `Explore ${label()}`)}</h2><nav aria-label={i18n.tr(`${label()} snarveier`, `${label()} shortcuts`)}><For each={content().links}>{(link) => <a link href={link.href}><span><strong>{link.label}</strong></span><ArrowUpRight size={15} /></a>}</For></nav></section>
      </aside>
    </section>
  )
}
