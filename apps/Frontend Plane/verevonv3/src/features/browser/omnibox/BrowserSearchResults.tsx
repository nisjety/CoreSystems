import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js'
import { cleanWebHistoryText, cleanWebHistoryUrl, webHistoryHost } from '@/features/dashboard/home/web-history'
import { runWebSearch, type WebSearchResult } from '@/shared/api/search-client'
import './browser-omnibox.css'

// Verevon search results in the browser frame stage, shown when the omnibox
// searches free text. A result opens through the same navigation as the address
// bar. Nothing here is stored: results live in this component for as long as it
// is shown.

/** Most results shown. */
export const SEARCH_RESULTS_MAX = 10

export type SafeSearchResult = {
  url: string
  title: string
  snippet: string
  host: string
}

/**
 * The results that may be shown and opened: http(s) links only, credentials
 * removed from the link, text trimmed to one line each. Anything else is dropped.
 */
export function safeSearchResults(results: readonly WebSearchResult[] | undefined): SafeSearchResult[] {
  const kept: SafeSearchResult[] = []
  for (const result of results ?? []) {
    const url = cleanWebHistoryUrl(result.url)
    if (!url) continue
    const host = webHistoryHost(url)
    kept.push({
      url,
      title: cleanWebHistoryText(result.title ?? '') || host,
      snippet: cleanWebHistoryText(result.snippet ?? ''),
      host,
    })
    if (kept.length >= SEARCH_RESULTS_MAX) break
  }
  return kept
}

type SearchState =
  | { status: 'loading' }
  | { status: 'ready'; results: SafeSearchResult[] }
  | { status: 'empty' }
  | { status: 'error' }

export function BrowserSearchResults(props: {
  query: string
  /** Zero-data-retention: the search runs, but nothing about it is kept here. */
  zdr: boolean
  /** The host this session is limited to, when a result on another host was refused. */
  limitedToHost?: string | null
  /** The host of the result that was refused, named in the notice when known. */
  refusedHost?: string | null
  /** Opens a result: the same navigation the address bar performs. */
  onOpen: (url: string) => void
  onClose: () => void
}) {
  const [state, setState] = createSignal<SearchState>({ status: 'loading' })
  let inflight: AbortController | undefined
  onCleanup(() => inflight?.abort())

  createEffect(
    () => props.query,
    (query) => {
      inflight?.abort()
      const request = new AbortController()
      inflight = request
      setState({ status: 'loading' })
      runWebSearch({ query, limit: SEARCH_RESULTS_MAX }, request.signal)
        .then((payload) => {
          if (request.signal.aborted) return
          const results = safeSearchResults(payload.results)
          setState(results.length > 0 ? { status: 'ready', results } : { status: 'empty' })
        })
        .catch(() => {
          if (!request.signal.aborted) setState({ status: 'error' })
        })
    },
  )

  const readyResults = () => {
    const current = state()
    return current.status === 'ready' ? current.results : []
  }

  return (
    <section class="browser-search-results" aria-label={`Søkeresultater for ${props.query}`}>
      <header class="browser-search-results__head">
        <span class="browser-search-results__query">
          Søk: <strong>{props.query}</strong>
        </span>
        <button type="button" class="browser-search-results__close" aria-label="Lukk søkeresultater" onClick={props.onClose}>
          Lukk
        </button>
      </header>
      <Show when={props.zdr}>
        <p class="browser-search-results__note" role="note">
          Zero data retention er på: søket lagres ikke lokalt, og det vises ingen forslag.
        </p>
      </Show>
      <Show when={props.limitedToHost}>
        {(host) => (
          <p class="browser-search-results__note" role="alert" data-reason="outside_scope">
            Denne økten er begrenset til {host()}
            <Show when={props.refusedHost && props.refusedHost !== host()}>
              {' '}og kan ikke åpne {props.refusedHost}
            </Show>
            . Åpne resultatet i en ny økt, eller ta over nettleseren selv.
          </p>
        )}
      </Show>
      <Show when={state().status === 'loading'}>
        <p class="browser-search-results__note" role="status">
          Søker …
        </p>
      </Show>
      <Show when={state().status === 'error'}>
        <p class="browser-search-results__note" role="alert">
          Søket feilet. Prøv igjen.
        </p>
      </Show>
      <Show when={state().status === 'empty'}>
        <p class="browser-search-results__note">Ingen treff for «{props.query}».</p>
      </Show>
      <Show when={state().status === 'ready'}>
        <ol class="browser-search-results__list">
          <For each={readyResults()}>
            {(result) => (
              <li class="browser-search-results__item">
                <button type="button" class="browser-search-results__title" onClick={() => props.onOpen(result.url)}>
                  {result.title}
                </button>
                <span class="browser-search-results__host">{result.host}</span>
                <Show when={result.snippet}>
                  <p class="browser-search-results__snippet">{result.snippet}</p>
                </Show>
              </li>
            )}
          </For>
        </ol>
      </Show>
    </section>
  )
}
