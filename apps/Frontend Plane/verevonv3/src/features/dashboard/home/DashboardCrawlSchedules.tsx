import { createMemo, For, Show } from 'solid-js'
import { ArrowUpRight, CalendarClock, RefreshCw } from '@/shared/icons'
import { listIngestionSchedules, type ScheduleItem } from '@/shared/api/ingestions-client'
import { useI18n } from '@/shared/i18n'
import { crawlDateLabel, crawlKindLabel, crawlMetaLine, crawlStatusLabel, ingestionsHref, upcomingCrawlSchedules, useCrawlList } from './dashboard-crawl'

/** Hent ut overview: the recurring schedules, soonest next run first. Read-only;
 * pausing, triggering and retiring stay on the ingestions page. */
export function DashboardCrawlSchedules() {
  const i18n = useI18n()
  const { items, loading, error, retry } = useCrawlList<ScheduleItem>(listIngestionSchedules)
  const visible = createMemo(() => upcomingCrawlSchedules(items()))
  return (
    <section class="home-overview-card home-recent home-recent--schedules" aria-labelledby="home-crawl-schedules-title" aria-busy={loading() ? 'true' : 'false'}>
      <header class="home-overview-card__header">
        <div><span class="home-eyebrow">{i18n.tr('Gjentakende', 'Recurring')}</span><h2 id="home-crawl-schedules-title">{i18n.tr('Planlagte innhentinger', 'Scheduled ingestions')}</h2></div>
      </header>
      <Show when={!loading()} fallback={<div class="home-recent__empty" role="status">{i18n.tr('Henter tidsplanene dine …', 'Loading your schedules…')}</div>}>
        <Show when={!error()} fallback={<div class="home-recent__empty" role="status"><p>{i18n.tr('Tidsplanene kunne ikke lastes.', 'Schedules could not be loaded.')}</p><button type="button" class="home-quiet-button" onClick={retry}><RefreshCw size={14} />{i18n.tr('Prøv igjen', 'Try again')}</button></div>}>
          <Show when={visible().length} fallback={<div class="home-recent__empty"><CalendarClock size={24} /><p>{i18n.tr('Ingen planlagte innhentinger ennå.', 'No scheduled ingestions yet.')}</p><span>{i18n.tr('Tidsplaner opprettes under Innhenting.', 'Schedules are created under Ingestions.')}</span></div>}>
            <div class="home-recent__list"><For each={visible()}>{(schedule) => (
              <a link href={ingestionsHref} class="home-recent__row">
                <span class="home-recent__icon"><CalendarClock size={18} /></span>
                <span class="home-recent__copy"><strong>{schedule.name || schedule.target || crawlKindLabel(schedule.kind, i18n.tr)}</strong><small>{crawlMetaLine(crawlKindLabel(schedule.kind, i18n.tr), crawlStatusLabel(schedule.status, i18n.tr))}</small></span>
                <Show when={crawlDateLabel(schedule.nextRunAt, i18n.locale())}>{(label) => <time datetime={schedule.nextRunAt ?? undefined}title={i18n.tr('Neste kjøring', 'Next run')}>{label()}</time>}</Show><ArrowUpRight size={14} />
              </a>
            )}</For></div>
          </Show>
        </Show>
      </Show>
      <footer><a link href={ingestionsHref}>{i18n.tr('Alle tidsplaner', 'All schedules')}<ArrowUpRight size={14} /></a><span>{i18n.tr('Neste kjøring først', 'Next run first')}</span></footer>
    </section>
  )
}
