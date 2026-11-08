import { ExternalLink, X } from '@/shared/icons'
import { createEffect, createSignal, Show } from 'solid-js'
import {
  citationPassage,
  fetchCitedChunk,
  type CitationLocator,
  type CitedChunk,
} from '@/shared/api/chat-client'
import { useI18n } from '@/shared/i18n'
import { isSafeLinkHref } from '../lib/safe-href'
import type { Citation } from './chat-types'

/**
 * The passage a citation points at (EP-2-S4). Read from the citation itself, so a live citation and a stored one
 * (whose keys arrive as the gateway wrote them) give the same answer. `null` when the server sent neither half,
 * which is every citation while the gateway's cited-passage switch is off: the viewer then never opens.
 */
export type CitedPassage = { quote?: string; locator?: CitationLocator }

export function citedPassageOf(citation: Citation): CitedPassage | null {
  const passage = citationPassage(citation as unknown as Record<string, unknown>)
  return passage.quote || passage.locator ? passage : null
}

/** Words of the quote a text-fragment link highlights: enough to find the passage, few enough to survive edits. */
const TEXT_FRAGMENT_WORDS = 8

/** Longest chunk text the viewer renders. Data Plane returns the whole document, so the cap is applied here. */
const MAX_CHUNK_CHARS = 6_000

/**
 * The live page opened at the quote, as a text-fragment link (D8: no read path into Quarry's stored copy). The
 * fragment's own separators (`-`, `,`, `&`) are escaped so a quote can never split the directive. `null` when the
 * page address is not one the chat may link to, or when the quote has no words; the result is checked again, so a
 * crafted address can never become a link through the fragment.
 */
export function textFragmentHref(url: string, quote: string | undefined): string | null {
  if (!isSafeLinkHref(url)) return null
  const words = (quote ?? '').split(/\s+/).filter(Boolean).slice(0, TEXT_FRAGMENT_WORDS)
  if (words.length === 0) return url.trim()
  const encoded = encodeURIComponent(words.join(' ')).replace(/-/g, '%2D')
  const base = url.trim()
  const href = `${base}${base.includes('#') ? '' : '#'}:~:text=${encoded}`
  return isSafeLinkHref(href) ? href : null
}

type SourceKind = 'knowledge' | 'web' | 'connector'

function sourceKind(citation: Citation, passage: CitedPassage): SourceKind {
  if (citation.origin === 'connector') return 'connector'
  if (citation.origin === 'knowledge' || passage.locator) return 'knowledge'
  return 'web'
}

/**
 * Side sheet that shows the passage a cited source contributed (EP-2-S4), opened from a citation chip or a Sources
 * row (action `chat.open_cited_passage`). Knowledge: the quote, and "Vis mer" reads the chunk again through the
 * BFF's Data Plane chunk read, where Data Plane decides what this viewer may see. Web: the quote and the live page
 * opened at it. Connector: the quote only, since the gateway never sends a connected account's body.
 */
export function CitedPassageViewer(props: { citation: Citation; onClose: () => void }) {
  const i18n = useI18n()
  const passage = () => citedPassageOf(props.citation) ?? {}
  const kind = () => sourceKind(props.citation, passage())
  const [chunk, setChunk] = createSignal<CitedChunk | null>(null)
  const [loading, setLoading] = createSignal(false)
  const [failed, setFailed] = createSignal(false)

  const loadMore = async () => {
    const locator = passage().locator
    if (!locator || loading()) return
    setLoading(true)
    setFailed(false)
    try {
      setChunk(await fetchCitedChunk(locator))
    } catch {
      setFailed(true)
    } finally {
      setLoading(false)
    }
  }

  // Solid v2 has no onMount: a two-phase effect with a constant compute runs once after mount.
  createEffect(() => undefined, () => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') props.onClose()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  })

  const pageHref = () => (kind() === 'web' ? textFragmentHref(props.citation.url, passage().quote) : null)
  const chunkText = (value: CitedChunk) => (value.status === 'ok' ? value.text.slice(0, MAX_CHUNK_CHARS) : '')

  return (
    <aside
      class="verevon-chat-cited-passage"
      role="dialog"
      aria-modal="false"
      aria-labelledby="verevon-chat-cited-passage-title"
    >
      <header class="verevon-chat-cited-passage__header">
        <h2 id="verevon-chat-cited-passage-title">{props.citation.title || props.citation.url}</h2>
        <button type="button" onClick={() => props.onClose()} aria-label={i18n.tr('Lukk', 'Close')}>
          <X size={16} />
        </button>
      </header>
      <Show
        when={passage().quote}
        fallback={<p class="verevon-chat-cited-passage__empty">{i18n.tr('Kilden sendte ingen passasje.', 'The source sent no passage.')}</p>}
      >
        {(quote) => <blockquote class="verevon-chat-cited-passage__quote">{quote()}</blockquote>}
      </Show>
      <Show when={kind() === 'knowledge' && passage().locator}>
        <Show
          when={chunk()}
          fallback={(
            <button type="button" class="verevon-chat-cited-passage__more" disabled={loading()} onClick={() => void loadMore()}>
              {loading() ? i18n.tr('Henter …', 'Loading …') : i18n.tr('Vis mer', 'Show more')}
            </button>
          )}
        >
          {(value) => (
            <Show
              when={value().status === 'ok'}
              fallback={(
                <p class="verevon-chat-cited-passage__denied" role="status">
                  {value().status === 'denied'
                    ? i18n.tr('Du har ikke tilgang til dette dokumentet', 'You do not have access to this document')
                    : i18n.tr('Fant ikke dette avsnittet i dokumentet lenger', 'This passage is no longer in the document')}
                </p>
              )}
            >
              <p class="verevon-chat-cited-passage__chunk">{chunkText(value())}</p>
            </Show>
          )}
        </Show>
        <Show when={failed()}>
          <p class="verevon-chat-cited-passage__denied" role="status">
            {i18n.tr('Kunne ikke hente dokumentet nå', 'Could not load the document right now')}
          </p>
        </Show>
      </Show>
      <Show when={pageHref()}>
        {(href) => (
          <a class="verevon-chat-cited-passage__open" href={href()} target="_blank" rel="noopener noreferrer">
            {i18n.tr('Åpne på siden', 'Open on the page')} <ExternalLink size={14} />
          </a>
        )}
      </Show>
    </aside>
  )
}
