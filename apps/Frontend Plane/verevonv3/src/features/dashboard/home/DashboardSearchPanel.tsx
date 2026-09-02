
import { useNavigate } from '@solidjs/router'
import { useQueryClient } from '@tanstack/solid-query'
import { ArrowRight, CirclePlus, ExternalLink, Loader2, Search } from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, Match, onCleanup, Show, Switch, untrack } from 'solid-js'
import { FOUR_HOURS_MS } from '@/app/providers/QueryProvider'
import { writePendingChatLaunch } from '@/features/chat/lib/pending-chat-launch'
import { searchNavbar, type NavbarSearchResult } from '@/shared/api/navbar-client'
import {
  buildPreviewResults,
  emptyWebSearchFilters,
  findSimilar,
  loadSearchImages,
  loadQuerySuggestions,
  type EntityPanel,
  loadSearchSuggestions,
  loadSearchVideos,
  runWebSearch,
  safeHostname,
  streamSearchAnswer,
  type ImageHit,
  type PreviewResult,
  type SearchSuggestion,
  type VideoHit,
  type WebSearchCitation,
  type WebSearchFilters,
  type WebSearchPayload,
} from '@/shared/api/search-client'
import { searchResultTabs as resultTabs, type ImagesStatus, type SearchResultTab } from '@/features/dashboard/home/dashboard-home-types'
import {
  ImageResultsPanel,
  MapGuidePanel,
  SearchInsightRail,
  SearchResultCard,
  ShoppingResultsPanel,
  SimilarResultsPanel,
  SimilarSeedForm,
  VerevonResultsSection,
  VideoResultsPanel,
} from '@/features/dashboard/home/DashboardSearchResults'
import { SearchFilterBar } from '@/features/dashboard/home/DashboardSearchFilters'
import {
  buildExpandedResults,
  filtersSignature,
  mergeCitations,
  webResultsToPreviews,
  type SearchSourceItem,
} from '@/features/dashboard/home/dashboard-search-utils'
import { useI18n } from '@/shared/i18n'
import { readClientJson, writeClientJson } from '@/shared/session/client-storage'

// Search session state lives at module scope so the active query survives the
// panel unmounting on a tab switch — combined with the 4h web-search cache, the
// last query is restored (and its preview re-renders instantly from cache) when
// you return to the Søk tab.
const [query, setQuery] = createSignal('')
const searchSessionKey = 'verevon.dashboard.search.session'
// Client-side pagination: over-fetch one page-worth more than we show, reveal in
// SEARCH_PAGE_SIZE increments via "Vis flere". Avoids backend offset paging
// through the router's cross-provider merge/dedup (which has no stable offset).
const SEARCH_PAGE_SIZE = 10
const SEARCH_FETCH_LIMIT = 30

type SearchSnapshot = {
  activeResultTab: SearchResultTab
  expanded: boolean
  filters: WebSearchFilters
  previewResults: PreviewResult[]
  query: string
  savedAt: number
  verevonResults: NavbarSearchResult[]
  webAnswer: string | null
  webCitations: WebSearchCitation[]
  webResults: PreviewResult[]
}

export function SearchPanel(props: {
  expanded: boolean
  onExpandedChange: (expanded: boolean) => void
  onPreviewActiveChange: (active: boolean) => void
  previewCollapsed?: boolean
}) {
  const i18n = useI18n()
  const navigate = useNavigate()
  const restored = readSearchSnapshot()
  let suggestionsTimer: number | undefined
  let previewTimer: number | undefined
  let suggestionsController: AbortController | undefined
  let searchController: AbortController | undefined
  let imagesController: AbortController | undefined
  let answerController: AbortController | undefined
  let videosController: AbortController | undefined
  let verevonController: AbortController | undefined
  let similarController: AbortController | undefined
  let querySuggestController: AbortController | undefined
  const [suggestions, setSuggestions] = createSignal<SearchSuggestion[]>([])
  const [previewResults, setPreviewResults] = createSignal<PreviewResult[]>(restored?.previewResults ?? [])
  const [previewLoading, setPreviewLoading] = createSignal(false)
  const [previewError, setPreviewError] = createSignal<string | null>(null)
  const [webAnswer, setWebAnswer] = createSignal<string | null>(restored?.webAnswer ?? null)
  const [webError, setWebError] = createSignal<string | null>(null)
  const [webLoading, setWebLoading] = createSignal(false)
  const [webResults, setWebResults] = createSignal<PreviewResult[]>(restored?.webResults ?? [])
  const [webCitations, setWebCitations] = createSignal<WebSearchCitation[]>(restored?.webCitations ?? [])
  const [activeResultTab, setActiveResultTab] = createSignal<SearchResultTab>(restored?.activeResultTab ?? 'Info')
  const [followUpQuery, setFollowUpQuery] = createSignal('')
  const [images, setImages] = createSignal<ImageHit[]>([])
  const [imagesStatus, setImagesStatus] = createSignal<ImagesStatus>('idle')
  const [imagesError, setImagesError] = createSignal<string | null>(null)
  const [imagesQuery, setImagesQuery] = createSignal('')
  const [videos, setVideos] = createSignal<VideoHit[]>([])
  const [videosStatus, setVideosStatus] = createSignal<ImagesStatus>('idle')
  const [videosError, setVideosError] = createSignal<string | null>(null)
  const [videosQuery, setVideosQuery] = createSignal('')
  const [verevonResults, setVerevonResults] = createSignal<NavbarSearchResult[]>(restored?.verevonResults ?? [])
  const [verevonLoading, setVerevonLoading] = createSignal(false)
  const [answerStreaming, setAnswerStreaming] = createSignal(false)
  const [filters, setFilters] = createSignal<WebSearchFilters>(coerceFilters(restored?.filters))
  const [similarLabel, setSimilarLabel] = createSignal<string | null>(null)
  const [similarResults, setSimilarResults] = createSignal<PreviewResult[]>([])
  const [similarStatus, setSimilarStatus] = createSignal<ImagesStatus>('idle')
  const [similarError, setSimilarError] = createSignal<string | null>(null)
  const [correctedQuery, setCorrectedQuery] = createSignal<string | null>(null)
  const [relatedQueries, setRelatedQueries] = createSignal<string[]>([])
  const [entity, setEntity] = createSignal<EntityPanel | null>(null)
  // How many of the fetched web results are revealed; "Vis flere" reveals more
  // (client-side pagination over the over-fetched set — see fetchWebSearch).
  const [visibleCount, setVisibleCount] = createSignal(SEARCH_PAGE_SIZE)
  const activeQuery = () => query().trim()
  const queryClient = useQueryClient()
  // Web search cached 4h by query AND active filters: the typeahead preview, the
  // Enter-key expanded search, and any repeat of the same query+filters share ONE
  // deduped fetch and return instantly from cache on repeats (data lives in the
  // client, surviving unmount). The filter signature in the key keeps a filtered
  // and unfiltered query from colliding on one cache entry.
  // Filters are passed explicitly (never read off the signal inside a deferred
  // callback) so the reactive read always happens in a tracked scope.
  const webQueryKey = (q: string, withFilters: WebSearchFilters) =>
    ['search-web', q, filtersSignature(withFilters)] as const
  const fetchWebSearch = (q: string, withFilters: WebSearchFilters) =>
    queryClient.fetchQuery({
      queryKey: webQueryKey(q, withFilters),
      queryFn: ({ signal }) =>
        runWebSearch({ filters: withFilters, includeAnswer: false, limit: SEARCH_FETCH_LIMIT, query: q }, signal),
      staleTime: FOUR_HOURS_MS,
    })

  createEffect(
    () => undefined,
    () => {
      if (!restored?.query) return
      setQuery(restored.query)
      props.onExpandedChange(restored.expanded)
      props.onPreviewActiveChange(!restored.expanded && restored.previewResults.length > 0)
    },
  )

  createEffect(
    () => ({
      activeResultTab: activeResultTab(),
      expanded: props.expanded,
      filters: filters(),
      previewResults: previewResults(),
      query: query(),
      verevonResults: verevonResults(),
      webAnswer: webAnswer(),
      webCitations: webCitations(),
      webResults: webResults(),
    }),
    (state) => {
      const snapshot: SearchSnapshot = {
        ...state,
        savedAt: Date.now(),
      }
      if (
        snapshot.query.trim()
        || snapshot.previewResults.length > 0
        || snapshot.webResults.length > 0
        || snapshot.webAnswer
      ) {
        writeClientJson(searchSessionKey, snapshot)
      }
    },
  )
  const sourceItems = createMemo(() => {
    const map = new Map<string, SearchSourceItem>()
    for (const item of [...webCitations(), ...webResults()]) {
      if (!item.url || map.has(item.url)) continue
      map.set(item.url, {
        hostname: safeHostname(item.url),
        title: item.title ?? safeHostname(item.url),
        url: item.url,
      })
    }
    return Array.from(map.values())
  })
  const showPreview = () => !props.previewCollapsed && activeQuery().length >= 3 && !props.expanded && (previewLoading() || previewError() || previewResults().length > 0)
  const isDropdownOpen = () => activeQuery().length >= 2 && suggestions().length > 0 && !props.expanded

  const updateQuery = (value: string) => {
    setQuery(value)
  }

  const submit = async (event: SubmitEvent) => {
    event.preventDefault()
    const trimmed = activeQuery()
    if (trimmed.length < 3) return
    props.onExpandedChange(true)
    props.onPreviewActiveChange(false)
    await runExpandedSearch(trimmed)
  }

  const runExpandedSearch = async (nextQuery: string, options?: { refreshSidecars?: boolean }) => {
    // The grounded AI answer, internal "i Verevon" matches, images, and videos do
    // not depend on the web filters, so a filter-only re-run skips them and just
    // re-fetches the filtered web result list.
    const refreshSidecars = options?.refreshSidecars ?? true
    const currentFilters = filters()
    searchController?.abort()
    const controller = new AbortController()
    searchController = controller
    const cached = queryClient.getQueryData<WebSearchPayload>(webQueryKey(nextQuery, currentFilters))
    setWebError(null)
    setWebLoading(!cached)
    setVisibleCount(SEARCH_PAGE_SIZE)
    setWebResults(cached ? buildExpandedResults(cached) : [])
    setWebCitations(
      cached && Array.isArray(cached.citations)
        ? cached.citations.filter((citation) => typeof citation.url === 'string' && citation.url.trim().length > 0)
        : [],
    )

    if (refreshSidecars) {
      setWebAnswer(null)
      setImages([])
      setImagesError(null)
      setImagesQuery('')
      setImagesStatus('idle')
      setVideos([])
      setVideosError(null)
      setVideosQuery('')
      setVideosStatus('idle')
      // Neighbours from a prior query no longer apply to a fresh search.
      similarController?.abort()
      setSimilarLabel(null)
      setSimilarResults([])
      setSimilarError(null)
      setSimilarStatus('idle')
      setCorrectedQuery(null)
      setRelatedQueries([])
      setEntity(null)

      // The grounded AI answer streams token-by-token in parallel, the internal
      // "i Verevon" arm queries the knowledge graph, and did-you-mean/related
      // suggestions are fetched alongside — all independent of the web result
      // list below, so none of them block it.
      streamAnswer(nextQuery)
      fetchVerevon(nextQuery)
      fetchQuerySuggestions(nextQuery)
    }

    try {
      const payload = await fetchWebSearch(nextQuery, currentFilters)
      if (controller.signal.aborted) return
      const results = buildExpandedResults(payload)
      setWebResults(results)
      setWebCitations(
        Array.isArray(payload.citations)
          ? payload.citations.filter((citation) => typeof citation.url === 'string' && citation.url.trim().length > 0)
          : [],
      )
    } catch (reason) {
      if (controller.signal.aborted) return
      setWebError(reason instanceof Error ? reason.message : 'Søket kunne ikke fullføres.')
    } finally {
      if (!controller.signal.aborted) setWebLoading(false)
    }
  }

  const streamAnswer = (nextQuery: string) => {
    answerController?.abort()
    const controller = new AbortController()
    answerController = controller
    setAnswerStreaming(true)
    setWebAnswer(null)
    void streamSearchAnswer(
      nextQuery,
      {
        onDelta: (text) => {
          if (controller.signal.aborted || !text) return
          setWebAnswer((current) => (current ?? '') + text)
        },
        onCitations: (citations) => {
          if (controller.signal.aborted) return
          setWebCitations((current) => mergeCitations(current, citations))
        },
        onDone: () => {
          if (!controller.signal.aborted) setAnswerStreaming(false)
        },
        onError: () => {
          if (!controller.signal.aborted) setAnswerStreaming(false)
        },
      },
      controller.signal,
    )
  }

  // Did-you-mean + related searches (Google-style), fetched in parallel so they
  // never delay results. Degrade-safe: empties on any failure.
  const fetchQuerySuggestions = (nextQuery: string) => {
    querySuggestController?.abort()
    const controller = new AbortController()
    querySuggestController = controller
    setCorrectedQuery(null)
    setRelatedQueries([])
    setEntity(null)
    loadQuerySuggestions(nextQuery, controller.signal)
      .then((payload) => {
        if (controller.signal.aborted) return
        setCorrectedQuery(payload.correctedQuery)
        setRelatedQueries(payload.relatedQueries)
        setEntity(payload.entity)
      })
      .catch(() => {
        if (!controller.signal.aborted) {
          setCorrectedQuery(null)
          setRelatedQueries([])
          setEntity(null)
        }
      })
  }

  const fetchVerevon = (nextQuery: string) => {
    verevonController?.abort()
    const controller = new AbortController()
    verevonController = controller
    setVerevonLoading(true)
    setVerevonResults([])
    searchNavbar(nextQuery, controller.signal)
      .then((payload) => {
        if (controller.signal.aborted) return
        setVerevonResults(Array.isArray(payload.results) ? payload.results.slice(0, 6) : [])
      })
      .catch(() => {
        if (!controller.signal.aborted) setVerevonResults([])
      })
      .finally(() => {
        if (!controller.signal.aborted) setVerevonLoading(false)
      })
  }

  const fetchVideos = (rawQuery: string) => {
    const trimmed = rawQuery.trim()
    if (!trimmed) return
    videosController?.abort()
    const controller = new AbortController()
    videosController = controller
    setVideos([])
    setVideosError(null)
    setVideosQuery(trimmed)
    setVideosStatus('loading')
    loadSearchVideos(trimmed, controller.signal)
      .then((payload) => {
        if (controller.signal.aborted) return
        setVideos(Array.isArray(payload.videos) ? payload.videos : [])
        setVideosStatus('loaded')
      })
      .catch((reason) => {
        if (controller.signal.aborted) return
        setVideosError(reason instanceof Error ? reason.message : 'Videosøk kunne ikke fullføres.')
        setVideosStatus('error')
      })
  }

  const fetchImages = (rawQuery: string) => {
    const trimmed = rawQuery.trim()
    if (!trimmed) return
    imagesController?.abort()
    const controller = new AbortController()
    imagesController = controller
    setImages([])
    setImagesError(null)
    setImagesQuery(trimmed)
    setImagesStatus('loading')
    loadSearchImages(trimmed, controller.signal)
      .then((payload) => {
        setImages(Array.isArray(payload.images) ? payload.images : [])
        setImagesStatus('loaded')
      })
      .catch((reason) => {
        if (controller.signal.aborted) return
        setImagesError(reason instanceof Error ? reason.message : i18n.tr('Bildesøk kunne ikke fullføres.', 'Image search could not be completed.'))
        setImagesStatus('error')
      })
  }

  // Changing a filter re-runs the web search in place (when expanded with a live
  // query) without re-streaming the AI answer or re-querying internal matches.
  const applyFilters = (next: WebSearchFilters) => {
    setFilters(next)
    if (props.expanded && activeQuery().length >= 3) {
      void runExpandedSearch(activeQuery(), { refreshSidecars: false })
    }
  }

  const runFindSimilar = (seed: { label: string; text?: string; url?: string }) => {
    similarController?.abort()
    const controller = new AbortController()
    similarController = controller
    setSimilarLabel(seed.label)
    setSimilarResults([])
    setSimilarError(null)
    setSimilarStatus('loading')
    findSimilar({ text: seed.text, url: seed.url }, controller.signal)
      .then((payload) => {
        if (controller.signal.aborted) return
        setSimilarResults(webResultsToPreviews(Array.isArray(payload.results) ? payload.results : []))
        setSimilarStatus('loaded')
      })
      .catch((reason) => {
        if (controller.signal.aborted) return
        setSimilarError(reason instanceof Error ? reason.message : 'Kunne ikke finne lignende sider.')
        setSimilarStatus('error')
      })
  }

  const findSimilarToSeed = (value: string) => {
    const trimmed = value.trim()
    if (!trimmed) return
    const isUrl = !/\s/.test(trimmed) && (/^https?:\/\//.test(trimmed) || /^[\w-]+(\.[\w-]+)+/.test(trimmed))
    runFindSimilar(isUrl ? { label: trimmed, url: trimmed } : { label: trimmed, text: trimmed })
  }

  const closeSimilar = () => {
    similarController?.abort()
    setSimilarLabel(null)
    setSimilarResults([])
    setSimilarError(null)
    setSimilarStatus('idle')
  }

  const submitFollowUp = (event: SubmitEvent) => {
    event.preventDefault()
    const next = followUpQuery().trim()
    if (!next) return
    setFollowUpQuery('')
    if (webResults().length > 0 || webAnswer() || verevonResults().length > 0) {
      void launchChatWithSearchContext(next)
      return
    }
    setQuery(next)
    props.onExpandedChange(true)
    void runExpandedSearch(next)
  }

  const launchChatWithSearchContext = async (question?: string) => {
    const prompt = buildSearchChatPrompt({
      answer: webAnswer(),
      query: activeQuery(),
      question: question?.trim(),
      sources: sourceItems().slice(0, 8),
      verevon: verevonResults().slice(0, 6),
    })
    await writePendingChatLaunch({
      text: prompt,
      // 'reason' dropped with the tool itself: it never reached the model,
      // so this launch behaves exactly as before.
      tools: ['search', 'research'],
    })
    navigateToChat(navigate)
  }

  // Lazy-load image results once the expanded panel is open and a query exists.
  createEffect(
    () => ({ trimmed: activeQuery(), expanded: props.expanded }),
    ({ trimmed, expanded }) => {
      if (!expanded || trimmed.length < 3) return
      untrack(() => {
        if (imagesStatus() === 'idle' || imagesQuery() !== trimmed) {
          fetchImages(trimmed)
        }
      })
    },
  )

  // Lazy-load real video results (SearXNG) the first time the Videos tab opens.
  createEffect(
    () => ({ trimmed: activeQuery(), onVideosTab: activeResultTab() === 'Videos', expanded: props.expanded }),
    ({ trimmed, onVideosTab, expanded }) => {
      if (!expanded || trimmed.length < 3 || !onVideosTab) return
      untrack(() => {
        if (videosStatus() === 'idle' || videosQuery() !== trimmed) {
          fetchVideos(trimmed)
        }
      })
    },
  )

  createEffect(
    () => activeQuery(),
    (trimmed) => {
      window.clearTimeout(suggestionsTimer)
      suggestionsController?.abort()

      if (trimmed.length < 2) {
        setSuggestions([])
        return
      }

      const controller = new AbortController()
      suggestionsController = controller
      suggestionsTimer = window.setTimeout(() => {
        loadSearchSuggestions(trimmed, controller.signal)
          .then((payload) => {
            const nextSuggestions = (payload.suggestions ?? [])
              .filter((suggestion) => suggestion.text.trim().length > 0)
              .slice(0, 6)
            setSuggestions(nextSuggestions)
          })
          .catch(() => {
            if (!controller.signal.aborted) setSuggestions([])
          })
      }, 120)
    },
  )

  createEffect(
    // Read filters synchronously in the tracked compute scope; the deferred
    // fetch below uses this captured value, never the signal.
    () => ({ trimmed: activeQuery(), expanded: props.expanded, currentFilters: filters() }),
    ({ trimmed, expanded, currentFilters }) => {
      window.clearTimeout(previewTimer)

      if (trimmed.length < 3 || expanded) {
        setPreviewLoading(false)
        setPreviewError(null)
        setPreviewResults([])
        props.onPreviewActiveChange(false)
        return
      }

      props.onPreviewActiveChange(true)
      searchController?.abort()
      const controller = new AbortController()
      searchController = controller
      const cached = queryClient.getQueryData<WebSearchPayload>(webQueryKey(trimmed, currentFilters))
      if (cached) {
        setPreviewLoading(false)
        setPreviewError(null)
        setPreviewResults(buildPreviewResults(cached).slice(0, 6))
        return
      }
      previewTimer = window.setTimeout(() => {
        setPreviewLoading(true)
        setPreviewError(null)
        fetchWebSearch(trimmed, currentFilters)
          .then((payload) => {
            if (controller.signal.aborted) return
            setPreviewResults(buildPreviewResults(payload).slice(0, 6))
          })
          .catch((reason) => {
            if (controller.signal.aborted) return
            setPreviewResults([])
            setPreviewError(reason instanceof Error ? reason.message : 'Søket kunne ikke fullføres.')
          })
          .finally(() => {
            if (!controller.signal.aborted) setPreviewLoading(false)
          })
      }, 220)
    },
  )

  onCleanup(() => {
    window.clearTimeout(suggestionsTimer)
    window.clearTimeout(previewTimer)
    suggestionsController?.abort()
    searchController?.abort()
    imagesController?.abort()
    answerController?.abort()
    videosController?.abort()
    verevonController?.abort()
    similarController?.abort()
    querySuggestController?.abort()
  })

  return (
    <Show
      when={!props.expanded}
      fallback={
        <div class="verevon-panel-in verevon-search-expanded-shell">
          <div class="dashboard-xsearch-header">
            <form onSubmit={submit}>
              <div class="dashboard-xsearch-input-row">
                <Search class="size-4 shrink-0 text-[#9A9188]" aria-hidden="true" />
                <input
                  value={query()}
                  onInput={(event) => updateQuery(event.currentTarget.value)}
                  class="dashboard-xsearch-input"
                  placeholder={i18n.tr('Skriv et nytt søk ...', 'Type a new search ...')}
                  autocomplete="off"
                />
                <Show when={webLoading()}>
                  <Loader2 class="size-4 shrink-0 dashboard-xsearch-spin" aria-hidden="true" />
                </Show>
              </div>
            </form>

            <div class="dashboard-xsearch-toolbar">
              <div class="dashboard-xsearch-tabs">
                <For each={resultTabs}>
                  {(tab) => (
                    <button
                      type="button"
                      onClick={() => setActiveResultTab(tab)}
                      class={{ 'dashboard-xsearch-tab--active': activeResultTab() === tab }}
                      aria-pressed={activeResultTab() === tab ? 'true' : 'false'}
                    >
                      {searchResultTabLabel(tab, i18n)}
                    </button>
                  )}
                </For>
              </div>

              <div class="dashboard-xsearch-actions">
                <button type="button" onClick={() => void runExpandedSearch(activeQuery())} disabled={webLoading()}>
                  {i18n.tr('Oppdater', 'Refresh')}
                </button>
                <button type="button" onClick={() => setActiveResultTab('Info')}>
                  {i18n.tr('Kilder', 'Sources')} {sourceItems().length ? sourceItems().length : ''}
                </button>
                <button
                  type="button"
                  onClick={() => void launchChatWithSearchContext()}
                  disabled={!activeQuery()}
                >
                  {i18n.tr('Spør Verevon', 'Ask Verevon')}
                </button>
                <button type="button" onClick={() => props.onExpandedChange(false)}>{i18n.tr('Kompakt', 'Compact')}</button>
              </div>
            </div>

            <SearchFilterBar filters={filters()} onChange={applyFilters} />
          </div>

          <div class="dashboard-xsearch-body">
            <div class="dashboard-xsearch-main">
              <Switch>
                <Match when={webLoading() && webResults().length === 0}>
                  <div class="dashboard-xsearch-status">
                    <Loader2 class="size-4 dashboard-xsearch-spin" aria-hidden="true" />
                    {i18n.tr('Tenker ...', 'Thinking ...')}
                  </div>
                </Match>
                <Match when={Boolean(webError()) && webResults().length === 0}>
                  <div class="dashboard-xsearch-error">{webError()}</div>
                </Match>
                <Match when={activeResultTab() === 'Images'}>
                  <ImageResultsPanel
                    images={images()}
                    imagesError={imagesError()}
                    imagesStatus={imagesStatus()}
                    query={activeQuery()}
                  />
                </Match>
                <Match when={activeResultTab() === 'Videos'}>
                  <VideoResultsPanel
                    error={videosError()}
                    query={activeQuery()}
                    status={videosStatus()}
                    videos={videos()}
                  />
                </Match>
                <Match when={activeResultTab() === 'Map'}>
                  <MapGuidePanel query={activeQuery()} results={webResults()} />
                </Match>
                <Match when={activeResultTab() === 'Shopping'}>
                  <ShoppingResultsPanel query={activeQuery()} results={webResults()} />
                </Match>
                <Match when={true}>
                  <div class="dashboard-xsearch-info">
                    <Show when={webLoading()}>
                      <div class="dashboard-xsearch-status">
                        <Loader2 class="size-4 dashboard-xsearch-spin" aria-hidden="true" />
                        Laster flere treff…
                      </div>
                    </Show>
                    <Show when={answerStreaming() || webAnswer()}>
                      <section class="dashboard-xsearch-summary">
                        <p class="dashboard-xsearch-eyebrow">
                          {i18n.tr('Verevon-sammendrag', 'Verevon summary')}
                          <Show when={answerStreaming()}>
                            <Loader2 class="ml-2 inline size-3 dashboard-xsearch-spin" aria-hidden="true" />
                          </Show>
                        </p>
                        <Show
                          when={webAnswer()}
                          fallback={<p class="dashboard-xsearch-summary-text">{i18n.tr('Verevon samler kilder og skriver et svar ...', 'Verevon is gathering sources and writing an answer ...')}</p>}
                        >
                          <p class="dashboard-xsearch-summary-text">{webAnswer()}</p>
                        </Show>
                      </section>
                    </Show>
                    <Show when={entity()}>
                      {(panel) => (
                        <section class="dashboard-xentity">
                          <div class="dashboard-xentity__head">
                            <h3 class="dashboard-xentity__name">{panel().name}</h3>
                            <Show when={panel().kind}>
                              <span class="dashboard-xentity__kind">{panel().kind}</span>
                            </Show>
                          </div>
                          <Show when={panel().summary}>
                            <p class="dashboard-xentity__summary">{panel().summary}</p>
                          </Show>
                          <Show when={panel().facts.length > 0}>
                            <dl class="dashboard-xentity__facts">
                              <For each={panel().facts}>
                                {(fact) => (
                                  <div>
                                    <dt>{fact.label}</dt>
                                    <dd>{fact.value}</dd>
                                  </div>
                                )}
                              </For>
                            </dl>
                          </Show>
                        </section>
                      )}
                    </Show>
                    <VerevonResultsSection loading={verevonLoading()} results={verevonResults()} />
                    <Show when={correctedQuery() || relatedQueries().length > 0}>
                      <section class="dashboard-xsuggest">
                        <Show when={correctedQuery()}>
                          {(corrected) => (
                            <p class="dashboard-xsuggest__correction">
                              {i18n.tr('Mente du', 'Did you mean')}{' '}
                              <button
                                type="button"
                                onClick={() => {
                                  setQuery(corrected())
                                  void runExpandedSearch(corrected())
                                }}
                              >
                                {corrected()}
                              </button>
                              ?
                            </p>
                          )}
                        </Show>
                        <Show when={relatedQueries().length > 0}>
                          <div class="dashboard-xsuggest__related">
                            <span class="dashboard-xsearch-eyebrow">{i18n.tr('Relaterte søk', 'Related searches')}</span>
                            <div class="dashboard-xsuggest__chips">
                              <For each={relatedQueries()}>
                                {(related) => (
                                  <button
                                    type="button"
                                    class="dashboard-xsuggest__chip"
                                    onClick={() => {
                                      setQuery(related)
                                      void runExpandedSearch(related)
                                    }}
                                  >
                                    {related}
                                  </button>
                                )}
                              </For>
                            </div>
                          </div>
                        </Show>
                      </section>
                    </Show>
                    <SimilarSeedForm onSubmit={findSimilarToSeed} />
                    <Show when={similarLabel()}>
                      {(label) => (
                        <SimilarResultsPanel
                          error={similarError()}
                          label={label()}
                          onClose={closeSimilar}
                          onOpen={(next) => window.open(next.url, '_blank', 'noopener,noreferrer')}
                          query={activeQuery()}
                          results={similarResults()}
                          status={similarStatus()}
                        />
                      )}
                    </Show>
                    <Show
                      when={webResults().length > 0}
                      fallback={
                        <div class="dashboard-xsearch-empty">{i18n.tr('Endre søket over, eller skriv en oppfølging under.', 'Change the search above, or write a follow-up below.')}</div>
                      }
                    >
                      <For each={webResults().slice(0, visibleCount())}>
                        {(result, index) => (
                          <SearchResultCard
                            result={result}
                            index={index()}
                            image={images()[index() % Math.max(images().length, 1)]}
                            onOpen={(next) => window.open(next.url, '_blank', 'noopener,noreferrer')}
                            onFindSimilar={(next) => runFindSimilar({ label: next.title, url: next.url })}
                          />
                        )}
                      </For>
                      <Show when={webResults().length > visibleCount()}>
                        <button
                          type="button"
                          class="dashboard-xsearch-more"
                          onClick={() => setVisibleCount((count) => count + SEARCH_PAGE_SIZE)}
                        >
                          {i18n.tr('Vis flere', 'Show more')} ({webResults().length - visibleCount()})
                        </button>
                      </Show>
                    </Show>
                  </div>
                </Match>
              </Switch>
            </div>

            <aside class="dashboard-xsearch-rail">
              <SearchInsightRail
                activeTab={activeResultTab()}
                answer={webAnswer() ?? ''}
                images={images()}
                query={activeQuery()}
                sourceItems={sourceItems().slice(0, 5)}
              />
            </aside>
          </div>

          <form onSubmit={submitFollowUp} class="dashboard-xsearch-followup">
            <CirclePlus class="size-4 shrink-0 text-[#9A9188]" aria-hidden="true" />
            <input
              value={followUpQuery()}
              onInput={(event) => setFollowUpQuery(event.currentTarget.value)}
              placeholder={i18n.tr('Spør om oppfølging ...', 'Ask a follow-up ...')}
              autocomplete="off"
            />
            <button
              type="submit"
              disabled={!followUpQuery().trim()}
              aria-label={i18n.tr('Send oppfølging', 'Send follow-up')}
              title={i18n.tr('Send oppfølging', 'Send follow-up')}
            >
              <ArrowRight class="size-4" />
            </button>
          </form>
        </div>
      }
    >
      <div class="verevon-panel-in relative transition-all duration-500 ease-out">
        <form onSubmit={submit} class="relative z-[1] w-full">
          <label class="sr-only" for="dashboard-search">{i18n.tr('Søk i selskapets kunnskap', 'Search company knowledge')}</label>
          <div class="dashboard-search-row">
            <button
              type="button"
              aria-label={i18n.tr('Send søkekontekst til chat', 'Send search context to chat')}
              class="verevon-glass-input dashboard-search-context-button"
              disabled={!activeQuery()}
              onClick={() => void launchChatWithSearchContext()}
              title={i18n.tr('Send søkekontekst til chat', 'Send search context to chat')}
            >
              <CirclePlus class="size-4" />
            </button>
            <div class="verevon-glass-input dashboard-search-input-wrap">
              <Search class="mr-2 size-4 shrink-0 text-[#9A9188]" />
              <input
                id="dashboard-search"
                role="combobox"
                aria-label={i18n.tr('Søk i selskapets kunnskap', 'Search company knowledge')}
                aria-expanded={isDropdownOpen() ? 'true' : 'false'}
                autocomplete="off"
                value={query()}
                onInput={(event) => updateQuery(event.currentTarget.value)}
                class="dashboard-search-input"
                placeholder={i18n.tr('Søk eller spør ...', 'Search or ask ...')}
              />
              <button
                type="submit"
                aria-label={i18n.tr('Søk', 'Search')}
                disabled={!activeQuery()}
                class="dashboard-search-submit"
                title={i18n.tr('Søk', 'Search')}
              >
                <ArrowRight class="size-4" />
              </button>
            </div>
          </div>

          <Show when={isDropdownOpen()}>
            <div class="verevon-glass verevon-fade-up dashboard-search-suggestions">
              <p>{i18n.tr('Forslag', 'Suggestions')}</p>
              <For each={suggestions()}>
                {(suggestion) => (
                  <button type="button" onMouseDown={() => updateQuery(suggestion.text)}>
                    <Search class="size-4 shrink-0 text-[#7E776F]" aria-hidden="true" />
                    <span>{suggestion.text}</span>
                    <small>{suggestion.collection || suggestion.source}</small>
                  </button>
                )}
              </For>
            </div>
          </Show>

          <Show when={showPreview()}>
            <div class="verevon-fade-up dashboard-search-preview-results">
              <div>
                <Show
                  when={!previewLoading()}
                  fallback={<div class="dashboard-search-preview-status">{i18n.tr('Søker ...', 'Searching ...')}</div>}
                >
                  <Show
                    when={!previewError()}
                    fallback={<div class="dashboard-search-preview-status">{previewError()}</div>}
                  >
                    <div class="dashboard-search-preview-results__list">
                      <For each={previewResults()}>
                        {(result) => (
                          <button
                            type="button"
                            class="dashboard-search-preview-result"
                            onClick={() => {
                              setActiveResultTab('Info')
                              props.onExpandedChange(true)
                              props.onPreviewActiveChange(false)
                              void runExpandedSearch(activeQuery())
                            }}
                          >
                            <span>
                              <span>{result.hostname}</span>
                              <ExternalLink class="size-3 shrink-0" aria-hidden="true" />
                            </span>
                            <strong>{result.title}</strong>
                            <small>{result.snippet}</small>
                          </button>
                        )}
                      </For>
                    </div>
                  </Show>
                </Show>
                <div class="dashboard-search-preview-results__footer">
                  <p>{i18n.tr('Viser', 'Showing')} {Math.min(previewResults().length, 3)} {i18n.tr('av', 'of')} {previewResults().length} {i18n.tr('forhåndstreff.', 'preview results.')}</p>
                  <button
                    type="button"
                    onClick={() => {
                      props.onExpandedChange(true)
                      props.onPreviewActiveChange(false)
                      void runExpandedSearch(activeQuery())
                    }}
                  >
                    {i18n.tr('Se mer', 'See more')}
                  </button>
                </div>
              </div>
            </div>
          </Show>
        </form>
      </div>
    </Show>
  )
}

function readSearchSnapshot(): SearchSnapshot | null {
  const snapshot = readClientJson(searchSessionKey, isSearchSnapshot)
  if (!snapshot) return null
  const maxAgeMs = FOUR_HOURS_MS
  return Date.now() - snapshot.savedAt <= maxAgeMs ? snapshot : null
}

function isSearchSnapshot(value: unknown): value is SearchSnapshot {
  if (!value || typeof value !== 'object') return false
  const record = value as Record<string, unknown>
  return typeof record.query === 'string'
    && typeof record.expanded === 'boolean'
    && typeof record.savedAt === 'number'
    && isResultTab(record.activeResultTab)
    && Array.isArray(record.previewResults)
    && Array.isArray(record.webResults)
    && Array.isArray(record.webCitations)
    && Array.isArray(record.verevonResults)
    && (record.webAnswer === null || typeof record.webAnswer === 'string')
}

function isResultTab(value: unknown): value is SearchResultTab {
  return value === 'Info' || value === 'Videos' || value === 'Map' || value === 'Images' || value === 'Shopping'
}

function searchResultTabLabel(tab: SearchResultTab, i18n: ReturnType<typeof useI18n>): string {
  if (tab === 'Videos') return i18n.tr('Videoer', 'Videos')
  if (tab === 'Map') return i18n.tr('Kart', 'Map')
  if (tab === 'Images') return i18n.tr('Bilder', 'Images')
  if (tab === 'Shopping') return i18n.tr('Shopping', 'Shopping')
  return i18n.tr('Info', 'Info')
}

/**
 * Coerce a possibly-stale/partial persisted filter object into a valid
 * {@link WebSearchFilters} — older snapshots predate the filter field, and we
 * never want a malformed value reaching the filter signature or request body.
 */
function coerceFilters(value: WebSearchFilters | undefined): WebSearchFilters {
  if (!value || typeof value !== 'object') return { ...emptyWebSearchFilters }
  const topic =
    value.topic === 'news' || value.topic === 'finance' || value.topic === 'general' ? value.topic : null
  const timeRange =
    value.timeRange === 'day' || value.timeRange === 'week' || value.timeRange === 'month' || value.timeRange === 'year'
      ? value.timeRange
      : null
  const toDomains = (input: unknown): string[] =>
    Array.isArray(input) ? input.filter((d): d is string => typeof d === 'string' && d.trim().length > 0) : []
  return {
    exactMatch: value.exactMatch === true,
    excludeDomains: toDomains(value.excludeDomains),
    includeDomains: toDomains(value.includeDomains),
    timeRange,
    topic,
  }
}

function buildSearchChatPrompt(input: {
  answer: string | null
  query: string
  question?: string
  sources: SearchSourceItem[]
  verevon: NavbarSearchResult[]
}): string {
  const lines = [
    input.question
      ? `Answer this follow-up using the search context below: ${input.question}`
      : `Continue from this search and help me decide the next best action: ${input.query}`,
    '',
    `Original search: ${input.query}`,
  ]

  if (input.answer?.trim()) {
    lines.push('', 'Current search answer:', input.answer.trim())
  }

  if (input.sources.length > 0) {
    lines.push('', 'Web sources:')
    for (const source of input.sources) {
      lines.push(`- ${source.title} (${source.hostname}) ${source.url}`)
    }
  }

  if (input.verevon.length > 0) {
    lines.push('', 'Verevon internal matches:')
    for (const result of input.verevon) {
      lines.push(`- ${result.label}${result.excerpt ? `: ${result.excerpt}` : ''}`)
    }
  }

  lines.push('', 'Use citations when possible. If the internal Verevon matches are relevant, ground the answer in them before using public web sources.')
  return lines.join('\n')
}

function navigateToChat(navigate: ReturnType<typeof useNavigate>) {
  const doc = document as Document & {
    startViewTransition?: (callback: () => void) => { finished: Promise<void> }
  }

  if (!prefersReducedMotion() && typeof doc.startViewTransition === 'function') {
    doc.startViewTransition(() => navigate('/chat'))
    return
  }

  navigate('/chat')
}

function prefersReducedMotion() {
  return typeof window.matchMedia === 'function' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
}
