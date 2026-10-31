import { For, Show } from 'solid-js'
import type { ChatSearchHit } from '@/shared/api/chat-client'
import { useI18n } from '@/shared/i18n'

/** A search runs once the query has this many characters. */
export const SEARCH_MIN_CHARS = 2
/** Quiet time after the last keystroke before the server is asked. */
export const SEARCH_DEBOUNCE_MS = 250

export type ChatSearchStatus = 'idle' | 'loading' | 'ready' | 'unavailable'

const MARK_START = ''
const MARK_END = ''

export function searchListboxId(idPrefix: string): string {
  return `${idPrefix}-listbox`
}

export function searchOptionId(idPrefix: string, index: number): string {
  return `${idPrefix}-option-${index}`
}

/**
 * Splits a snippet into plain and matched runs. The markers are consumed here,
 * so a stray marker in content can never mark text the server did not match.
 */
export function snippetRuns(snippet: string): Array<{ text: string; matched: boolean }> {
  const runs: Array<{ text: string; matched: boolean }> = []
  let matched = false
  for (const piece of snippet.split(/([])/)) {
    if (piece === MARK_START) matched = true
    else if (piece === MARK_END) matched = false
    else if (piece) runs.push({ text: piece, matched })
  }
  return runs
}

/**
 * The active option after an arrow, Home or End key over `count` hits, or null
 * when the key does not move the selection. ArrowDown from nothing selected
 * starts at the first hit; ArrowUp from nothing starts at the last.
 */
export function nextSearchIndex(current: number, key: string, count: number): number | null {
  if (count === 0) return null
  switch (key) {
    case 'ArrowDown': return current < 0 || current >= count - 1 ? 0 : current + 1
    case 'ArrowUp': return current <= 0 ? count - 1 : current - 1
    case 'Home': return 0
    case 'End': return count - 1
    default: return null
  }
}

/**
 * Server hits for the sidebar search, above the local filter. The input keeps
 * focus and drives the selection (combobox pattern), so this list is never a
 * tab stop; it only announces what the search found.
 */
export function ChatSearchResults(props: {
  idPrefix: string
  hits: ChatSearchHit[]
  status: ChatSearchStatus
  activeIndex: number
  onOpen: (hit: ChatSearchHit) => void
  onActiveChange: (index: number) => void
}) {
  const i18n = useI18n()
  const rows = () => props.hits.map((hit, index) => ({ hit, index }))
  const announcement = () => {
    if (props.status === 'loading') return i18n.tr('Søker …', 'Searching …')
    if (props.status === 'unavailable') {
      return i18n.tr('Søk er ikke tilgjengelig. Listen viser bare treff i samtalelisten.', 'Search is unavailable. The list shows matches from the chat list only.')
    }
    if (props.status === 'ready') {
      return props.hits.length
        ? i18n.tr(`${props.hits.length} treff i samtalene dine`, `${props.hits.length} matches in your chats`)
        : i18n.tr('Ingen treff i samtalene dine.', 'No matches in your chats.')
    }
    return ''
  }

  return (
    <div class="chat-search-results">
      <p class="chat-search-results__status" aria-live="polite">{announcement()}</p>
      <Show when={props.status === 'ready' && props.hits.length > 0}>
        <ul id={searchListboxId(props.idPrefix)} class="chat-search-results__list" role="listbox" aria-label={i18n.tr('Treff i samtalene dine', 'Matches in your chats')}>
          <For each={rows()}>{(row) => (
            <li id={searchOptionId(props.idPrefix, row.index)} role="option" class="chat-search-results__option"
              aria-selected={row.index === props.activeIndex ? 'true' : 'false'}
              onMouseDown={(event) => event.preventDefault()}
              onMouseEnter={() => props.onActiveChange(row.index)}
              onClick={() => props.onOpen(row.hit)}>
              <span class="chat-search-results__title">
                {row.hit.threadTitle || i18n.tr('Samtale uten tittel', 'Untitled chat')}
                <Show when={row.hit.archived}><small>{i18n.tr(' · Arkivert', ' · Archived')}</small></Show>
              </span>
              <span class="chat-search-results__snippet">
                <For each={snippetRuns(row.hit.snippet)}>{(run) => (
                  run.matched ? <mark>{run.text}</mark> : <span>{run.text}</span>
                )}</For>
              </span>
            </li>
          )}</For>
        </ul>
      </Show>
    </div>
  )
}
