import { createSignal, onCleanup } from 'solid-js'
import { streamCrawlPreview, type CrawlSnippet, type SseHandlers } from '@/features/onboarding/lib/api'

type CrawlPreviewInput = {
  brief?: string
  maxPages?: number
  orgId?: string
  url: string
}

export const CRAWL_SNIPPET_LIMIT = 12

/** URL identity for a snippet: fragment and trailing slash ignored, case-folded.
 * Seed and live emit the same page under different ids, so the URL is the key. */
export function crawlSnippetKey(snippet: Pick<CrawlSnippet, 'id' | 'url'>): string {
  const raw = (snippet.url || snippet.id || '').split('#')[0] ?? ''
  return raw.trim().replace(/\/+$/, '').toLowerCase()
}

/** 0 = bare host card (no text) … 3 = specific title + excerpt. Mirrors the
 * gateway's `SnippetLedger` scoring so both ends agree on "richer". */
export function snippetQuality(snippet: CrawlSnippet): number {
  const hasExcerpt = Boolean(snippet.excerpt && snippet.excerpt.trim())
  const hasSpecificTitle = snippet.titleSource === 'html' || snippet.titleSource === 'model'
  return (hasExcerpt ? 2 : 0) + (hasSpecificTitle ? 1 : 0)
}

export function isRicherCrawlSnippet(candidate: CrawlSnippet, current: CrawlSnippet): boolean {
  return snippetQuality(candidate) > snippetQuality(current)
}

/**
 * Merge one incoming snippet into the list by page URL: a first sighting is
 * appended; a richer snippet for a page already listed UPDATES that card in
 * place (quarry emits the text-less `page_fetched` card first and the
 * `page_extracted` card with title/excerpt moments later); a poorer duplicate
 * is ignored. The list is capped at `limit`, dropping the oldest.
 */
export function mergeCrawlSnippet(
  current: readonly CrawlSnippet[],
  incoming: CrawlSnippet,
  limit = CRAWL_SNIPPET_LIMIT,
): CrawlSnippet[] {
  const key = crawlSnippetKey(incoming)
  const index = current.findIndex((snippet) => crawlSnippetKey(snippet) === key)
  if (index < 0) return [...current, incoming].slice(-limit)
  const existing = current[index]
  if (!existing || !isRicherCrawlSnippet(incoming, existing)) return [...current]
  const next = [...current]
  next[index] = { ...existing, ...incoming, id: existing.id }
  return next
}

/** Collapse an append-only list to one card per page, richest wins, first-seen order. */
export function dedupeCrawlSnippets(snippets: readonly CrawlSnippet[]): CrawlSnippet[] {
  const byKey = new Map<string, CrawlSnippet>()
  for (const snippet of snippets) {
    const key = crawlSnippetKey(snippet)
    const existing = byKey.get(key)
    if (!existing) {
      byKey.set(key, snippet)
    } else if (isRicherCrawlSnippet(snippet, existing)) {
      byKey.set(key, { ...existing, ...snippet, id: existing.id })
    }
  }
  return [...byKey.values()]
}

export function createCrawlPreviewStream() {
  const [previewing, setPreviewing] = createSignal(false)
  let controller: AbortController | undefined

  const abort = () => {
    controller?.abort()
    controller = undefined
    setPreviewing(false)
  }

  const start = async (input: CrawlPreviewInput, handlers: SseHandlers) => {
    abort()
    const currentController = new AbortController()
    controller = currentController
    setPreviewing(true)

    // Suppress pure duplicates within one stream (same page, nothing new to
    // show) before they reach the caller; richer re-sends for a page pass
    // through so the caller can update its card via `mergeCrawlSnippet`.
    const best = new Map<string, CrawlSnippet>()
    const dedupedHandlers: SseHandlers = {
      ...handlers,
      onSnippet: handlers.onSnippet
        ? (payload) => {
            const key = crawlSnippetKey(payload)
            const seen = best.get(key)
            if (seen && !isRicherCrawlSnippet(payload, seen)) return
            best.set(key, payload)
            handlers.onSnippet?.(payload)
          }
        : undefined,
    }

    try {
      await streamCrawlPreview(input, dedupedHandlers, currentController.signal)
    } catch (reason) {
      if (currentController.signal.aborted) return
      throw reason
    } finally {
      if (controller === currentController) {
        controller = undefined
        setPreviewing(false)
      }
    }
  }

  onCleanup(abort)

  return {
    abort,
    previewing,
    start,
  }
}
