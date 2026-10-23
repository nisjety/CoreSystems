import { isLocalRetentionAllowed } from '@/features/chat/lib/chat-retention'
import { loadSearchSuggestions, type SearchSuggestion } from '@/shared/api/search-client'
import { readOmniboxHistory, type OmniboxHistoryEntry } from './omnibox-history'

// The dropdown rows under the omnibox field: remembered pages, query suggestions,
// and the two explicit rows ("Search the web", "Ask Verevon"). The field itself
// is the primary row; it is rendered by OmniboxField, not here.
//
// Zero data retention is absolute for this module: under `zero`, no history is
// read, no suggestion is requested, and nothing is stored. The check is made at
// every step, never once at creation, so a posture change takes effect at once.

export type OmniboxRow =
  | { kind: 'history'; id: string; url: string; title: string }
  | { kind: 'suggestion'; id: string; text: string }
  | { kind: 'search'; id: string; query: string }
  | { kind: 'ask'; id: string; text: string }

export type OmniboxSuggestionOptions = {
  /** Wait before asking for query suggestions, in milliseconds. */
  debounceMs?: number
  /** The history key of the signed-in identity, or null until both user and workspace are known. */
  historyKey: () => string | null
  /** Shows the "Search the web" row (the omnibox search flag is on). */
  searchEnabled: boolean
  /** Shows the "Ask Verevon" row (a hand-off to chat is wired). */
  askEnabled: boolean
  /** Whether this workspace is zero-data-retention right now. */
  zdr: () => boolean
  /** Fetches query suggestions. Defaults to the gateway suggestions call. */
  fetchSuggestions?: (query: string, signal: AbortSignal) => Promise<SearchSuggestion[]>
  /** Receives the rows after each change, the local rows first and the rest when they arrive. */
  onRows: (rows: OmniboxRow[]) => void
}

/** At least this many characters before any suggestion or history row is offered. */
export const OMNIBOX_MIN_QUERY_CHARS = 2
/** Rows from the remembered pages at most. */
export const OMNIBOX_HISTORY_ROWS = 5
/** Rows from the query suggestions at most. */
export const OMNIBOX_SUGGESTION_ROWS = 4

const DEFAULT_DEBOUNCE_MS = 150

const defaultFetch = (query: string, signal: AbortSignal) =>
  loadSearchSuggestions(query, signal).then((payload) => payload.suggestions ?? [])

/** The remembered pages whose address or title contains the text (case-insensitive), newest first. */
export function matchingHistory(entries: readonly OmniboxHistoryEntry[], text: string, limit = OMNIBOX_HISTORY_ROWS): OmniboxHistoryEntry[] {
  const needle = text.trim().toLowerCase()
  if (needle.length < OMNIBOX_MIN_QUERY_CHARS) return []
  return entries
    .filter((entry) => entry.url.toLowerCase().includes(needle) || entry.title.toLowerCase().includes(needle))
    .slice(0, limit)
}

/**
 * Creates the dropdown controller. Call `update` with the field's text on every
 * change; rows arrive through `onRows`. `dispose` stops the pending request and timer.
 */
export function createOmniboxSuggestions(options: OmniboxSuggestionOptions): {
  update: (input: string) => void
  dispose: () => void
} {
  const debounceMs = options.debounceMs ?? DEFAULT_DEBOUNCE_MS
  const fetchSuggestions = options.fetchSuggestions ?? defaultFetch
  let timer: ReturnType<typeof setTimeout> | undefined
  let controller: AbortController | undefined
  let latest = ''

  const cancel = () => {
    if (timer !== undefined) clearTimeout(timer)
    timer = undefined
    controller?.abort()
    controller = undefined
  }

  const explicitRows = (text: string): OmniboxRow[] => {
    const rows: OmniboxRow[] = []
    if (options.searchEnabled) rows.push({ kind: 'search', id: 'search', query: text })
    if (options.askEnabled) rows.push({ kind: 'ask', id: 'ask', text })
    return rows
  }

  const suggestionRows = (suggestions: SearchSuggestion[]): OmniboxRow[] =>
    suggestions
      .map((suggestion) => suggestion.text.trim())
      .filter((text) => text.length > 0)
      .slice(0, OMNIBOX_SUGGESTION_ROWS)
      .map((text, index) => ({ kind: 'suggestion' as const, id: `suggestion-${index}`, text }))

  const historyRows = (text: string): OmniboxRow[] => {
    // Under ZDR the record is not even read.
    if (options.zdr()) return []
    return matchingHistory(readOmniboxHistory(options.historyKey()), text).map((entry) => ({
      kind: 'history' as const,
      id: `history:${entry.url}`,
      url: entry.url,
      title: entry.title,
    }))
  }

  const update = (input: string) => {
    cancel()
    const text = input.trim()
    latest = text
    if (text.length < OMNIBOX_MIN_QUERY_CHARS) {
      options.onRows([])
      return
    }
    const local = historyRows(text)
    options.onRows([...local, ...explicitRows(text)])
    // With search off there is no suggestion request at all.
    if (options.zdr() || !options.searchEnabled) return

    timer = setTimeout(() => {
      timer = undefined
      // Checked again when the timer fires: the posture may have changed meanwhile.
      if (options.zdr() || !isLocalRetentionAllowed()) return
      const request = new AbortController()
      controller = request
      fetchSuggestions(text, request.signal)
        .then((suggestions) => {
          if (request.signal.aborted || latest !== text) return
          if (options.zdr()) return
          options.onRows([...local, ...suggestionRows(suggestions), ...explicitRows(text)])
        })
        .catch(() => {
          // Suggestions are not critical: a failure leaves the rows already shown.
        })
    }, debounceMs)
  }

  return { update, dispose: cancel }
}
