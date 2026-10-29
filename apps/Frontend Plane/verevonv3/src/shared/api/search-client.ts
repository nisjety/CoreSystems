import { requestJson } from './http'
import { readSseStream } from './sse'

export type SearchSuggestion = {
  collection?: string
  source?: string
  text: string
}

export type WebSearchResult = {
  displayUrl?: string
  highlights?: string[]
  hostname?: string
  score?: number
  snippet?: string
  title?: string
  url: string
}

export type SearchTopic = 'finance' | 'general' | 'news'
export type SearchTimeRange = 'day' | 'month' | 'week' | 'year'

/**
 * Exa-style web search filters forwarded to the gateway in camelCase. All
 * fields are optional; absent/empty filters are omitted from the request body
 * so an unfiltered query stays a plain `{ query, limit }` call. `topic: null`
 * and `timeRange: null` mean "no filter" (the default broad search).
 */
export type WebSearchFilters = {
  exactMatch?: boolean
  excludeDomains?: string[]
  includeDomains?: string[]
  timeRange?: SearchTimeRange | null
  topic?: SearchTopic | null
}

export const emptyWebSearchFilters: WebSearchFilters = {
  exactMatch: false,
  excludeDomains: [],
  includeDomains: [],
  timeRange: null,
  topic: null,
}

export type WebSearchCitation = {
  snippet?: string
  title?: string
  url: string
}

export type WebSearchPayload = {
  answer?: string | null
  citations?: WebSearchCitation[]
  description?: string | null
  excerpt?: string | null
  mode?: 'fetch' | 'search'
  results?: WebSearchResult[]
  title?: string | null
  url?: string | null
}

export type PreviewResult = {
  highlights?: string[]
  hostname: string
  score?: number
  snippet?: string
  title: string
  url: string
}

export type ImageHit = {
  imageUrl: string
  thumbnailUrl: string
  title: string | null
  url: string
}

export function loadSearchSuggestions(query: string, signal?: AbortSignal): Promise<{ suggestions?: SearchSuggestion[] }> {
  return requestJson<{ suggestions?: SearchSuggestion[] }>(
    `/api/v1/search/suggestions?q=${encodeURIComponent(query)}&scope=queries&limit=6`,
    { signal },
  )
}

export function runWebSearch(
  input: {
    filters?: WebSearchFilters
    includeAnswer?: boolean
    limit?: number
    query: string
  },
  signal?: AbortSignal,
): Promise<WebSearchPayload> {
  const filters = input.filters ?? {}
  const body: Record<string, unknown> = {
    includeAnswer: input.includeAnswer ?? false,
    limit: input.limit ?? 10,
    query: input.query,
  }
  // Only attach filters that are set — keeps an unfiltered query a plain
  // body and lets the gateway cache filtered vs unfiltered queries apart.
  if (filters.topic) body.topic = filters.topic
  if (filters.timeRange) body.timeRange = filters.timeRange
  if (filters.exactMatch) body.exactMatch = true
  if (filters.includeDomains?.length) body.includeDomains = filters.includeDomains
  if (filters.excludeDomains?.length) body.excludeDomains = filters.excludeDomains
  return requestJson<WebSearchPayload>('/api/v1/search/web', {
    method: 'POST',
    body: JSON.stringify(body),
    signal,
  })
}

export type SimilarSearchPayload = {
  mode?: 'similar'
  results?: WebSearchResult[]
}

/**
 * Exa-style "find similar" — neighbours of a seed URL or free-text passage.
 * Provide at least one of `url`/`text`. Returns the same result shape as web
 * search so the UI can reuse its result renderers.
 */
export function findSimilar(
  input: { limit?: number; text?: string; url?: string },
  signal?: AbortSignal,
): Promise<SimilarSearchPayload> {
  const body: Record<string, unknown> = { limit: input.limit ?? 10 }
  if (input.url?.trim()) body.url = input.url.trim()
  if (input.text?.trim()) body.text = input.text.trim()
  return requestJson<SimilarSearchPayload>('/api/v1/search/similar', {
    method: 'POST',
    body: JSON.stringify(body),
    signal,
  })
}

export type EntityFact = { label: string; value: string }
/** Google-style knowledge panel for a query's primary entity (text-only). */
export type EntityPanel = {
  name: string
  kind?: string
  summary?: string
  facts: EntityFact[]
}
export type QuerySuggestions = {
  correctedQuery: string | null
  relatedQueries: string[]
  entity: EntityPanel | null
}

function normalizeEntity(raw: unknown): EntityPanel | null {
  if (!raw || typeof raw !== 'object') return null
  const record = raw as Record<string, unknown>
  const name = typeof record.name === 'string' ? record.name.trim() : ''
  if (!name) return null
  const facts: EntityFact[] = Array.isArray(record.facts)
    ? record.facts
        .filter((f): f is Record<string, unknown> => Boolean(f) && typeof f === 'object')
        .map((f) => ({
          label: typeof f.label === 'string' ? f.label.trim() : '',
          value: typeof f.value === 'string' ? f.value.trim() : '',
        }))
        .filter((f) => f.label.length > 0 && f.value.length > 0)
    : []
  const kind = typeof record.kind === 'string' && record.kind.trim().length > 0 ? record.kind : undefined
  const summary =
    typeof record.summary === 'string' && record.summary.trim().length > 0 ? record.summary : undefined
  return { name, kind, summary, facts }
}

/**
 * Exa/Google-style "did you mean" (spell-correct) + related searches + a
 * knowledge panel for the query's primary entity, fetched in parallel with the
 * main search. Degrade-safe: returns empty on any failure so the searchbar
 * never breaks on suggestions.
 */
export function loadQuerySuggestions(query: string, signal?: AbortSignal): Promise<QuerySuggestions> {
  return requestJson<{ correctedQuery?: string | null; relatedQueries?: unknown; entity?: unknown }>(
    '/api/v1/search/suggest',
    { method: 'POST', body: JSON.stringify({ query }), signal },
  )
    .then((payload) => ({
      correctedQuery:
        typeof payload.correctedQuery === 'string' && payload.correctedQuery.trim().length > 0
          ? payload.correctedQuery
          : null,
      relatedQueries: Array.isArray(payload.relatedQueries)
        ? payload.relatedQueries.filter((value): value is string => typeof value === 'string')
        : [],
      entity: normalizeEntity(payload.entity),
    }))
    .catch(() => ({ correctedQuery: null, relatedQueries: [], entity: null }))
}

export function loadSearchImages(query: string, signal?: AbortSignal): Promise<{ images?: ImageHit[] }> {
  return requestJson<{ images?: ImageHit[] }>('/api/v1/search/images', {
    method: 'POST',
    body: JSON.stringify({ query, limit: 18 }),
    signal,
  })
}

export function buildPreviewResults(payload: WebSearchPayload): PreviewResult[] {
  if ((payload.mode === 'fetch' || payload.url) && payload.url) {
    return [
      {
        hostname: safeHostname(payload.url),
        snippet: payload.description ?? payload.excerpt ?? undefined,
        title: payload.title?.trim() || payload.url,
        url: payload.url,
      },
    ]
  }

  const sourceResults = Array.isArray(payload.results) ? payload.results : []
  return sourceResults
    .filter((result) => typeof result.url === 'string' && result.url.trim().length > 0)
    .slice(0, 8)
    .map((result) => ({
      highlights: result.highlights,
      hostname: result.hostname ?? result.displayUrl ?? safeHostname(result.url),
      score: result.score,
      snippet: result.snippet,
      title: result.title?.trim() || safeHostname(result.url),
      url: result.url,
    }))
}

export function safeHostname(value: string): string {
  try {
    return new URL(value, window.location.origin).hostname.replace(/^www\./, '')
  } catch {
    return value
  }
}

export type VideoHit = {
  author: string | null
  embedUrl: string | null
  length: string | null
  thumbnailUrl: string | null
  title: string | null
  url: string
}

export function loadSearchVideos(query: string, signal?: AbortSignal): Promise<{ videos?: VideoHit[] }> {
  return requestJson<{ videos?: VideoHit[] }>('/api/v1/search/videos', {
    method: 'POST',
    body: JSON.stringify({ query, limit: 18 }),
    signal,
  })
}

export type SearchAnswerHandlers = {
  onCitations?: (citations: WebSearchCitation[]) => void
  onDelta?: (text: string) => void
  onDone?: () => void
  onError?: (message: string) => void
}

/**
 * Stream the grounded AI answer for a query from quarry-edge's AnswerPipeline
 * (the Tavily-replacement synthesizer), proxied via the gateway as SSE. Frames:
 * `event: citations` (sources), `event: delta` (token text), `event: done`,
 * `event: error`. Some pipelines emit unnamed data frames carrying delta text.
 */
export async function streamSearchAnswer(
  query: string,
  handlers: SearchAnswerHandlers,
  signal?: AbortSignal,
): Promise<void> {
  await readSseStream(
    '/api/v1/search/answer/stream',
    { method: 'POST', body: JSON.stringify({ query }), signal },
    (event) => {
      switch (event.event) {
        case 'citations':
          handlers.onCitations?.(parseAnswerCitations(event.data))
          break
        case 'delta':
        case 'message':
        case undefined:
          handlers.onDelta?.(extractAnswerDelta(event.data))
          break
        case 'done':
          handlers.onDone?.()
          break
        case 'error':
          handlers.onError?.(extractAnswerError(event.data))
          break
      }
    },
    (err) => handlers.onError?.(err instanceof Error ? err.message : 'Svaret kunne ikke fullføres.'),
    () => handlers.onDone?.(),
  )
}

function extractAnswerDelta(data: string): string {
  if (!data) return ''
  try {
    const parsed = JSON.parse(data) as Record<string, unknown>
    for (const key of ['text', 'delta', 'token', 'content', 'answer']) {
      const value = parsed[key]
      if (typeof value === 'string') return value
    }
    return ''
  } catch {
    return data
  }
}

function parseAnswerCitations(data: string): WebSearchCitation[] {
  if (!data) return []
  try {
    const parsed = JSON.parse(data) as unknown
    const list = Array.isArray(parsed)
      ? parsed
      : Array.isArray((parsed as { citations?: unknown }).citations)
        ? (parsed as { citations: unknown[] }).citations
        : []
    return list
      .filter((item): item is Record<string, unknown> => Boolean(item) && typeof item === 'object')
      .map((item) => ({
        snippet: typeof item.snippet === 'string' ? item.snippet : undefined,
        title: typeof item.title === 'string' ? item.title : undefined,
        url: typeof item.url === 'string' ? item.url : '',
      }))
      .filter((citation) => citation.url.length > 0)
  } catch {
    return []
  }
}

function extractAnswerError(data: string): string {
  if (!data) return 'Svaret kunne ikke fullføres.'
  try {
    const parsed = JSON.parse(data) as Record<string, unknown>
    if (typeof parsed.message === 'string') return parsed.message
    if (typeof parsed.error === 'string') return parsed.error
    return 'Svaret kunne ikke fullføres.'
  } catch {
    return data
  }
}
