import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import { ArrowUpRight, Globe, Search, Trash2 } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { runSearch } from './dashboard-search-session'
import { clearWebHistory, useWebHistory, webHistoryCardRows, webHistoryHost } from './web-history'

/** Nett overview: the searches this browser ran and the pages it opened from their
 * results, newest first. It is the recent-work card for the Nett tab, in the same
 * markup, and shows only what web-history holds, never anything from the server. */
export function DashboardWebHistory() {
  const i18n = useI18n()
  const history = useWebHistory()
  const rows = createMemo(() => webHistoryCardRows(history()))
  // "Tøm" removes the list, and the button that was pressed goes with it, so focus
  // moves to the card's heading (not the top of the page) and a status line says
  // what happened. The line is cleared when something is remembered again, so the
  // next clearing is announced too.
  let heading: HTMLHeadingElement | undefined
  const [cleared, setCleared] = createSignal(false)
  createEffect(() => history().length, (count) => { if (count) setCleared(false) })
  const clear = () => {
    clearWebHistory()
    setCleared(true)
    heading?.focus()
  }
  // A search runs again through the panel's own submit flow. With no panel to
  // take it (the query is then only put in the box), the search box gets focus.
  // The row goes away when the results open; a keyboard press (a click with no
  // pointer, detail 0) has focus to keep, so the results page's search box takes it.
  const searchAgain = (text: string, event: MouseEvent) => {
    if (!runSearch(text, { focus: event.detail === 0 })) document.getElementById('dashboard-search')?.focus()
  }
  const dateLabel = (value: string) => {
    const date = new Date(value)
    return Number.isNaN(date.getTime()) ? '' : date.toLocaleDateString(i18n.locale() === 'no' ? 'nb-NO' : 'en-GB', { day: 'numeric', month: 'short' })
  }
  return (
    <section class="home-overview-card home-recent" aria-labelledby="home-web-history-title">
      <header class="home-overview-card__header">
        <div><span class="home-eyebrow">{i18n.tr('Nett', 'Web')}</span><h2 id="home-web-history-title" tabindex="-1" ref={(element) => { heading = element }}>{i18n.tr('Fortsett der du slapp', 'Pick up where you left off')}</h2></div>
        <Show when={history().length}>
          <button type="button" class="home-quiet-button" onClick={clear} title={i18n.tr('Fjern søkene og sidene som er lagret her', 'Remove the searches and pages saved here')}><Trash2 size={14} />{i18n.tr('Tøm', 'Clear')}</button>
        </Show>
      </header>
      <Show when={rows().length} fallback={<div class="home-recent__empty"><Search size={24} /><p>{i18n.tr('Ingen søk ennå.', 'No searches yet.')}</p><span>{i18n.tr('Søkene dine og sidene du åpner fra resultatene, dukker opp her.', 'Your searches and the pages you open from results will appear here.')}</span></div>}>
        <div class="home-recent__list"><For each={rows()}>{(entry) => (
          entry.kind === 'query' ? (
            <button type="button" class="home-recent__row" title={i18n.tr('Søk på nytt', 'Search again')} onClick={(event) => searchAgain(entry.text, event)}>
              <span class="home-recent__icon"><Search size={18} /></span>
              <span class="home-recent__copy"><strong>{entry.text}</strong><small>{i18n.tr('Søk på nettet', 'Web search')}</small></span>
              <time datetime={entry.at}>{dateLabel(entry.at)}</time><ArrowUpRight size={14} />
            </button>
          ) : (
            <a href={entry.url} target="_blank" rel="noopener noreferrer" class="home-recent__row" title={i18n.tr('Åpne siden i ny fane', 'Open the page in a new tab')}>
              <span class="home-recent__icon"><Globe size={18} /></span>
              <span class="home-recent__copy"><strong>{entry.text}</strong><small>{webHistoryHost(entry.url)}</small></span>
              <time datetime={entry.at}>{dateLabel(entry.at)}</time><ArrowUpRight size={14} />
            </a>
          )
        )}</For></div>
      </Show>
      <span class="sr-only" role="status">{cleared() ? i18n.tr('Søkene og sidene er fjernet.', 'The searches and pages were removed.') : ''}</span>
      <footer><span>{i18n.tr('Lagres bare i denne nettleseren', 'Saved in this browser only')}</span><span>{i18n.tr('Nyeste først', 'Newest first')}</span></footer>
    </section>
  )
}
