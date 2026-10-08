import { createMemo, For, Show } from 'solid-js'
import { ArrowUpRight, RefreshCw, ScanSearch } from '@/shared/icons'
import { listIngestionRuns, type RunItem } from '@/shared/api/ingestions-client'
import { useI18n } from '@/shared/i18n'
import { crawlDateLabel, crawlKindLabel, crawlMetaLine, crawlStatusLabel, ingestionsHref, latestCrawlRuns, useCrawlList } from './dashboard-crawl'

/** Hent ut overview: the latest durable runs from the ingestions API. Read-only,
 * and it shows what the API returns, with no counts or rates worked out here. */
export function DashboardCrawlRuns() {
  const i18n = useI18n()
  const { items, loading, error, retry } = useCrawlList<RunItem>(listIngestionRuns)
  const visible = createMemo(() => latestCrawlRuns(items()))
  return (
    <section class="home-overview-card home-recent" aria-labelledby="home-crawl-runs-title" aria-busy={loading() ? 'true' : 'false'}>
      <header class="home-overview-card__header">
        <div><span class="home-eyebrow">{i18n.tr('Hent ut', 'Extract')}</span><h2 id="home-crawl-runs-title">{i18n.tr('Siste innhentinger', 'Latest ingestions')}</h2></div>
      </header>
      <Show when={!loading()} fallback={<div class="home-recent__empty" role="status">{i18n.tr('Henter innhentingene dine …', 'Loading your ingestions…')}</div>}>
        <Show when={!error()} fallback={<div class="home-recent__empty" role="status"><p>{i18n.tr('Innhentingene kunne ikke lastes.', 'Ingestions could not be loaded.')}</p><button type="button" class="home-quiet-button" onClick={retry}><RefreshCw size={14} />{i18n.tr('Prøv igjen', 'Try again')}</button></div>}>
          <Show when={visible().length} fallback={<div class="home-recent__empty"><ScanSearch size={24} /><p>{i18n.tr('Ingen innhentinger ennå.', 'No ingestions yet.')}</p><span>{i18n.tr('Kjøringer fra Innhenting vises her.', 'Runs from Ingestions appear here.')}</span></div>}>
            <div class="home-recent__list"><For each={visible()}>{(run) => (
              <a link href={ingestionsHref} class="home-recent__row">
                <span class="home-recent__icon"><ScanSearch size={18} /></span>
                <span class="home-recent__copy"><strong>{run.target || crawlKindLabel(run.kind, i18n.tr)}</strong><small>{crawlMetaLine(crawlKindLabel(run.kind, i18n.tr), crawlStatusLabel(run.status, i18n.tr))}</small></span>
                <time datetime={run.createdAt} title={i18n.tr('Opprettet', 'Created')}>{crawlDateLabel(run.createdAt, i18n.locale())}</time><ArrowUpRight size={14} />
              </a>
            )}</For></div>
          </Show>
        </Show>
      </Show>
      <footer><a link href={ingestionsHref}>{i18n.tr('Alle kjøringer', 'All runs')}<ArrowUpRight size={14} /></a><span>{i18n.tr('Nyeste først', 'Newest first')}</span></footer>
    </section>
  )
}
