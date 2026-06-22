
import {
  buildPreviewResults,
  safeHostname,
  type PreviewResult,
  type WebSearchCitation,
  type WebSearchFilters,
  type WebSearchPayload,
  type WebSearchResult,
} from '@/shared/api/search-client'
import type { SearchResultTab } from '@/features/dashboard/home/dashboard-home-types'
import type { Locale } from '@/shared/i18n'

export type SearchSourceItem = {
  hostname: string
  title: string
  url: string
}

export function faviconUrlForResult(url: string): string | null {
  const hostname = safeHostname(url)
  if (!hostname || hostname === url) return null
  return `https://www.google.com/s2/favicons?domain=${encodeURIComponent(hostname)}&sz=64`
}

export function searchRailTips(activeTab: SearchResultTab, locale: Locale = 'no'): string[] {
  const no = locale === 'no'
  switch (activeTab) {
    case 'Map':
      return no
        ? [
            'Bruk kartvisningen til å sammenligne sted, reisevei, åpningstider og bookingtrygghet.',
            'Velion kan gjøre stedstreff om til en kort rute eller besøksplan når kartdata finnes.',
          ]
        : [
            'Use the map view to compare location, travel context, opening hours, and booking confidence.',
            'Velion can turn place results into a short route or visit plan when map data is available.',
          ]
    case 'Images':
      return no
        ? [
            'Bilder behandles som visuelt bevis, ikke endelige svar, til Velion kan sitere kildesiden.',
            'Velion kan forklare produkt-, sted-, layout- eller skjermbildedetaljer fra bildekonteksten.',
          ]
        : [
            'Images are treated as visual evidence, not final answers, until Velion can cite the source page.',
            'Velion can explain product, venue, layout, or screenshot details from the image context.',
          ]
    case 'Videos':
      return no
        ? [
            'Videokilder bør oppsummeres med tidsstempler og siterte kildesider når de finnes.',
            'Velion kan hente ut nyttige øyeblikk før hele klippet anbefales.',
          ]
        : [
            'Video sources should be summarized with timestamps and cited source pages when available.',
            'Velion can extract the useful moments before suggesting that a user watches the full clip.',
          ]
    case 'Shopping':
      return no
        ? [
            'Shopping- og bookingtreff bør kryssjekkes mot offisielle sider og ferske artikler.',
            'Velion kan sammenligne pris, tilgjengelighet, kildetrygghet og lignende alternativer.',
          ]
        : [
            'Shopping and booking results should be cross-checked against official pages and recent articles.',
            'Velion can compare price, availability, source confidence, and similar alternatives.',
          ]
    default:
      return no
        ? [
            'Bruk kildene til å se hvor svaret kommer fra før du stoler på eller gjenbruker det.',
            'Still et oppfølgingsspørsmål for å få Velion til å snevre inn, sammenligne, crawle eller oppsummere treffene.',
          ]
        : [
            'Use sources to inspect where the answer came from before trusting or reusing it.',
            'Ask a follow-up to make Velion narrow, compare, crawl, or summarize the result set.',
          ]
  }
}

export function mergeCitations(current: WebSearchCitation[], incoming: WebSearchCitation[]): WebSearchCitation[] {
  const seen = new Set(current.map((citation) => citation.url))
  const merged = [...current]
  for (const citation of incoming) {
    if (citation.url && !seen.has(citation.url)) {
      seen.add(citation.url)
      merged.push(citation)
    }
  }
  return merged
}

export function buildExpandedResults(payload: WebSearchPayload): PreviewResult[] {
  if (isFetchPayload(payload)) return buildPreviewResults(payload)

  const byUrl = new Map<string, PreviewResult>()
  addWebResults(byUrl, Array.isArray(payload.results) ? payload.results : [])
  addCitationResults(byUrl, Array.isArray(payload.citations) ? payload.citations : [])
  // Keep the full over-fetched set so the panel can paginate ("Vis flere")
  // client-side; the panel itself controls how many are visible.
  return Array.from(byUrl.values()).slice(0, 40)
}

function isFetchPayload(payload: WebSearchPayload): boolean {
  return Boolean((payload.mode === 'fetch' || payload.url) && payload.url)
}

function addWebResults(byUrl: Map<string, PreviewResult>, results: WebSearchResult[]) {
  for (const result of results) {
    if (!result.url || byUrl.has(result.url)) continue
    byUrl.set(result.url, {
      highlights: result.highlights,
      hostname: result.hostname ?? result.displayUrl ?? safeHostname(result.url),
      score: result.score,
      snippet: result.snippet,
      title: result.title?.trim() || safeHostname(result.url),
      url: result.url,
    })
  }
}

/**
 * Map raw web-search results (e.g. the "find similar" neighbour list) into the
 * deduped `PreviewResult[]` the result renderers expect — reusing the same
 * field mapping as the main result list.
 */
export function webResultsToPreviews(results: WebSearchResult[]): PreviewResult[] {
  const byUrl = new Map<string, PreviewResult>()
  addWebResults(byUrl, results)
  return Array.from(byUrl.values()).slice(0, 12)
}

/**
 * Stable cache-key fragment for a filter set. Domains are sorted so chip order
 * never changes the key, keeping it aligned with the gateway's own filter-aware
 * cache (a filtered and unfiltered query must not collide).
 */
export function filtersSignature(filters: WebSearchFilters): string {
  return [
    filters.topic ?? '',
    filters.timeRange ?? '',
    filters.exactMatch ? '1' : '',
    [...(filters.includeDomains ?? [])].sort().join(','),
    [...(filters.excludeDomains ?? [])].sort().join(','),
  ].join('|')
}

/** True when any filter narrows the search (used to surface a reset affordance). */
export function hasActiveFilters(filters: WebSearchFilters): boolean {
  return Boolean(
    filters.topic
      || filters.timeRange
      || filters.exactMatch
      || filters.includeDomains?.length
      || filters.excludeDomains?.length,
  )
}

function addCitationResults(byUrl: Map<string, PreviewResult>, citations: WebSearchCitation[]) {
  for (const citation of citations) {
    if (!citation.url || byUrl.has(citation.url)) continue
    byUrl.set(citation.url, {
      hostname: safeHostname(citation.url),
      snippet: citation.snippet,
      title: citation.title?.trim() || safeHostname(citation.url),
      url: citation.url,
    })
  }
}
