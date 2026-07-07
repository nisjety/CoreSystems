import {
  AlertCircle,
  Check,
  Code2,
  ExternalLink,
  Globe2,
  Loader2,
  Maximize2,
  MessageSquare,
  ShieldCheck,
  X,
} from 'lucide-solid'
import { createMemo, createSignal, For, Match, Show, Switch, untrack, type JSX } from 'solid-js'
import type {
  BrowserAction,
  BrowserActionSuggestionResponse,
  BrowserProfileRestoreProbe,
} from '@/shared/api/browser-client'
import type { BrowserLoopState } from './browser-loop'
import { BrowserChrome } from './BrowserChrome'
import { browserSessionFromPreview, type BrowserStepRationale } from './browser-session'
import { hostnameOf, type ScrapeBlock, type ScrapePreview } from './knowledge-preview'

// Inline markdown → safe JSX. We only resolve the tokens that are reliable to
// detect in scraped content — links and inline images, resolved against the
// scraped page URL — and clean residual emphasis/markers from the text in
// between. Everything is built as real elements (never innerHTML), so an
// untrusted scraped string can't inject markup.
const INLINE_LINK_RE = /(!?)\[([^\]]*)\]\(([^)\s]+)[^)]*\)/g

function cleanInlineText(value: string): string {
  return value
    .replace(/\*\*|__|[*_`]/g, '')
    .replace(/(^|\s)#{1,6}\s+/g, '$1')
    .replace(/[-=_~]{3,}/g, ' ')
    .replace(/[ \t]{2,}/g, ' ')
}

function hideBrokenImage(event: Event) {
  const image = event.currentTarget as HTMLImageElement | null
  if (image) image.style.display = 'none'
}

function resolveInlineUrl(value: string, baseUrl: string): string | null {
  try {
    const url = new URL(value, baseUrl)
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.toString() : null
  } catch {
    return null
  }
}

function blockNodeName(block: ScrapeBlock): string {
  const raw = block.raw.trim()
  if (/^#{1,6}\s+/.test(raw)) return 'heading'
  if (/^!\[/.test(raw)) return 'image'
  if (/^>\s+/.test(raw)) return 'quote'
  if (/^\s*(?:[-*+]|\d+\.)\s+/.test(raw)) return 'listitem'
  if (block.heading) return 'heading'
  return 'paragraph'
}

function renderInline(input: string, baseUrl: string): JSX.Element {
  const nodes: JSX.Element[] = []
  let lastIndex = 0
  let match: RegExpExecArray | null
  INLINE_LINK_RE.lastIndex = 0

  const pushText = (slice: string) => {
    const text = cleanInlineText(slice)
    if (text) nodes.push(text)
  }

  while ((match = INLINE_LINK_RE.exec(input)) !== null) {
    pushText(input.slice(lastIndex, match.index))
    const isImage = match[1] === '!'
    const label = match[2] ?? ''
    const url = resolveInlineUrl(match[3] ?? '', baseUrl)
    if (isImage && url) {
      nodes.push(
        <img
          class="knowledge-scrape-page__inline-img"
          src={url}
          alt={label}
          loading="lazy"
          onError={hideBrokenImage}
        />,
      )
    } else if (url) {
      const text = cleanInlineText(label).trim()
      nodes.push(
        <a href={url} target="_blank" rel="noopener noreferrer">
          {text || hostnameOf(url)}
        </a>,
      )
    } else {
      pushText(label)
    }
    lastIndex = match.index + match[0].length
  }
  pushText(input.slice(lastIndex))

  return <>{nodes}</>
}

/** Render one scraped block as the element it represents — heading, standalone
 * image, blockquote, list item, or paragraph — so the left column reads like a
 * page rather than a flat text dump. */
function BlockBody(props: { baseUrl: string; block: ScrapeBlock }) {
  const raw = () => props.block.raw.trim()
  const imageMatch = () => /^!\[([^\]]*)\]\(([^)\s]+)[^)]*\)\s*$/.exec(raw())
  const headingMatch = () => /^(#{1,6})\s+(.*)$/s.exec(raw())
  const quoteMatch = () => /^>\s+(.*)$/s.exec(raw())
  const listMatch = () => /^\s*(?:[-*+]|\d+\.)\s+(.*)$/s.exec(raw())

  return (
    <Switch fallback={<p class="knowledge-scrape-page__p">{renderInline(raw(), props.baseUrl)}</p>}>
      <Match when={imageMatch()} keyed>
        {(match) => {
          const src = () => resolveInlineUrl(match[2] ?? '', props.baseUrl)
          return (
            <Show
              when={src()}
              keyed
              fallback={<p class="knowledge-scrape-page__p">{cleanInlineText(match[1] ?? '')}</p>}
            >
              {(url) => (
                <figure class="knowledge-scrape-page__figure">
                  <img src={url} alt={match[1] ?? ''} loading="lazy" onError={hideBrokenImage} />
                  <Show when={match[1]}>
                    <figcaption>{match[1]}</figcaption>
                  </Show>
                </figure>
              )}
            </Show>
          )
        }}
      </Match>
      <Match when={headingMatch()} keyed>
        {(match) => (
          <p
            class="knowledge-scrape-page__heading"
            data-level={Math.min((match[1]?.length ?? 0), 4)}
            role="heading"
            aria-level={Math.min((match[1]?.length ?? 0), 6)}
          >
            {renderInline(match[2] ?? '', props.baseUrl)}
          </p>
        )}
      </Match>
      <Match when={quoteMatch()} keyed>
        {(match) => <blockquote class="knowledge-scrape-page__quote">{renderInline(match[1] ?? '', props.baseUrl)}</blockquote>}
      </Match>
      <Match when={listMatch()} keyed>
        {(match) => <p class="knowledge-scrape-page__li">{renderInline(match[1] ?? '', props.baseUrl)}</p>}
      </Match>
    </Switch>
  )
}

/** A hoverable / selectable region in the rendered page. Hovering or toggling
 * here is mirrored in the block list and vice-versa via the shared state owned
 * by ScrapePreviewPanel. */
function ScrapeRegion(props: {
  baseUrl: string
  block: ScrapeBlock
  selected: boolean
  hovered: boolean
  onToggle: () => void
  onEnter: () => void
  onLeave: () => void
}) {
  return (
    <div
      class="knowledge-scrape-region"
      classList={{
        'knowledge-scrape-region--selected': props.selected,
        'knowledge-scrape-region--deselected': !props.selected,
        'knowledge-scrape-region--hovered': props.hovered,
      }}
      role="button"
      tabindex="0"
      aria-pressed={props.selected}
      title={props.selected ? 'Klikk for å utelate denne seksjonen' : 'Klikk for å inkludere denne seksjonen'}
      onClick={() => props.onToggle()}
      onMouseEnter={() => props.onEnter()}
      onMouseLeave={() => props.onLeave()}
      onKeyDown={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault()
          props.onToggle()
        }
      }}
    >
      <span class="knowledge-scrape-region__marker" aria-hidden="true">
        <Show when={props.selected}>
          <Check class="size-3" />
        </Show>
      </span>
      <div class="knowledge-scrape-region__content">
        <BlockBody baseUrl={props.baseUrl} block={props.block} />
      </div>
    </div>
  )
}

function BrowserSessionSurface(props: {
  browserBusy?: boolean
  browserLoop?: BrowserLoopState
  browserRationales?: BrowserStepRationale[]
  hovered: number | null
  onBrowserAction?: (action: BrowserAction) => void
  onBrowserAutoRun?: (goal: string) => Promise<BrowserActionSuggestionResponse | null>
  onBrowserLoopPause?: () => void
  onBrowserLoopResume?: () => void
  onBrowserLoopStop?: () => void
  onBrowserSuggestAction?: (goal: string) => Promise<BrowserActionSuggestionResponse | null>
  onEnter: (index: number) => void
  onLeave: (index: number) => void
  onSelectAll: () => void
  onSelectNone: () => void
  onToggle: (index: number) => void
  preview: ScrapePreview
  profileProbe?: BrowserProfileRestoreProbe | null
  selected: Set<number>
  selectedChars: number
  selectedCount: number
  total: number
}) {
  const session = createMemo(() => browserSessionFromPreview(props.preview))
  const allSelected = () => props.total > 0 && props.selectedCount === props.total
  const isLive = () => session().renderMode === 'chromium'

  return (
    <div
      class="knowledge-browser-frame"
      classList={{
        'knowledge-browser-frame--live': isLive(),
        'knowledge-browser-frame--fallback': !isLive(),
      }}
    >
      <div class="knowledge-browser-frame__topbar">
        <div class="knowledge-browser-frame__tabs">
          <span class="knowledge-browser-traffic" aria-hidden="true">
            <i />
            <i />
            <i />
          </span>
          <span class="knowledge-browser-frame__tab">Sammendrag</span>
          <span class="knowledge-browser-frame__tab knowledge-browser-frame__tab--active">
            <Globe2 class="size-3.5" /> Browser
          </span>
          <span class="knowledge-browser-frame__plus">+</span>
        </div>
        <span class="knowledge-browser-commenting">
          <MessageSquare class="size-3.5" /> Annotering
        </span>
      </div>

      <BrowserChrome
        browserBusy={props.browserBusy}
        browserLoop={props.browserLoop}
        browserRationales={props.browserRationales}
        onBrowserAction={props.onBrowserAction}
        onBrowserAutoRun={props.onBrowserAutoRun}
        onBrowserLoopPause={props.onBrowserLoopPause}
        onBrowserLoopResume={props.onBrowserLoopResume}
        onBrowserLoopStop={props.onBrowserLoopStop}
        onBrowserSuggestAction={props.onBrowserSuggestAction}
        profileProbe={props.profileProbe}
        session={session()}
      >
        <div class="knowledge-browser-canvas">
          <div class="knowledge-browser-canvas__meta">
            <span><Maximize2 class="size-3.5" /> {session().viewport.width} × {session().viewport.height}</span>
            <span><ShieldCheck class="size-3.5" /> {session().sourceLabel}</span>
            <Show when={session().degradedReason}>
              {(reason) => (
                <span class="knowledge-browser-canvas__meta-warning" title={reason()}>
                  <AlertCircle class="size-3.5" /> {reason()}
                </span>
              )}
            </Show>
          </div>

          <For each={session().commentAnchors}>
            {(anchor) => (
              <span
                class="knowledge-browser-comment-anchor"
                style={{
                  left: `${anchor.x * 100}%`,
                  top: `${anchor.y * 100}%`,
                }}
                aria-label={`Kommentar ${anchor.label}`}
              >
                {anchor.label}
              </span>
            )}
          </For>

          <div class="knowledge-scrape-page" aria-label="Gjengitt side i nettleservisning">
            <header class="knowledge-scrape-page__browser-head">
              <span>{hostnameOf(props.preview.url)}</span>
              <strong>{props.preview.title}</strong>
              <Show when={props.preview.description}>
                <p>{props.preview.description}</p>
              </Show>
            </header>
            <For each={props.preview.blocks}>
              {(block, index) => (
                <ScrapeRegion
                  baseUrl={props.preview.url}
                  block={block}
                  selected={props.selected.has(index())}
                  hovered={props.hovered === index()}
                  onToggle={() => props.onToggle(index())}
                  onEnter={() => props.onEnter(index())}
                  onLeave={() => props.onLeave(index())}
                />
              )}
            </For>
          </div>
        </div>
      </BrowserChrome>

      <div class="knowledge-browser-frame__footer">
        <div class="knowledge-browser-frame__selection">
          <span><Code2 class="size-3.5" /> {props.selectedCount}/{props.total} seksjoner valgt</span>
          <span>{props.selectedChars.toLocaleString('nb-NO')} tegn</span>
        </div>
        <div class="knowledge-scrape-preview__select-actions" aria-label="Seksjonsvalg">
          <button type="button" onClick={() => props.onSelectAll()} disabled={allSelected()}>Velg alle</button>
          <button type="button" onClick={() => props.onSelectNone()} disabled={props.selectedCount === 0}>Fjern alle</button>
        </div>
      </div>

      <details class="knowledge-browser-selection-drawer">
        <summary>
          <span>Presist seksjonsvalg</span>
          <strong>{props.selectedCount}/{props.total}</strong>
        </summary>
        <div class="knowledge-scrape-list" aria-label="DOM-seksjoner">
          <For each={props.preview.blocks}>
            {(block, index) => {
              const isSelected = () => props.selected.has(index())
              return (
                <button
                  type="button"
                  class="knowledge-scrape-block"
                  classList={{
                    'knowledge-scrape-block--heading': block.heading,
                    'knowledge-scrape-block--selected': isSelected(),
                    'knowledge-scrape-block--deselected': !isSelected(),
                    'knowledge-scrape-block--hovered': props.hovered === index(),
                  }}
                  aria-pressed={isSelected()}
                  onClick={() => props.onToggle(index())}
                  onMouseEnter={() => props.onEnter(index())}
                  onMouseLeave={() => props.onLeave(index())}
                >
                  <span class="knowledge-scrape-block__node" aria-hidden="true">
                    {blockNodeName(block)}
                  </span>
                  <span class="knowledge-scrape-block__text">{block.text}</span>
                  <span class="knowledge-scrape-block__check" aria-hidden="true">
                    <Show when={isSelected()}>
                      <Check class="size-3" />
                    </Show>
                  </span>
                </button>
              )
            }}
          </For>
        </div>
      </details>
    </div>
  )
}

export function ScrapePreviewPanel(props: {
  adding: boolean
  browserBusy?: boolean
  browserLoop?: BrowserLoopState
  browserRationales?: BrowserStepRationale[]
  onBrowserAction?: (action: BrowserAction) => void
  onBrowserAutoRun?: (goal: string) => Promise<BrowserActionSuggestionResponse | null>
  onBrowserLoopPause?: () => void
  onBrowserLoopResume?: () => void
  onBrowserLoopStop?: () => void
  onBrowserSuggestAction?: (goal: string) => Promise<BrowserActionSuggestionResponse | null>
  onAdd: (selectedMarkdown: string, allSelected: boolean) => void
  onDiscard: () => void
  preview: ScrapePreview
  profileProbe?: BrowserProfileRestoreProbe | null
}) {
  // Mounted fresh per scrape (parent <Show keyed>), so default-select every block.
  const total = createMemo(() => props.preview.blocks.length)
  const [selected, setSelected] = createSignal<Set<number>>(
    untrack(() => new Set(props.preview.blocks.map((_, index) => index))),
  )
  const [hovered, setHovered] = createSignal<number | null>(null)
  const selectedCount = () => selected().size
  const allSelected = () => total() > 0 && selectedCount() === total()
  const selectedChars = createMemo(() => {
    const selectedSet = selected()
    return props.preview.blocks.reduce((sum, block, index) => (selectedSet.has(index) ? sum + block.raw.length : sum), 0)
  })
  const emptyMessage = createMemo(() => props.preview.source === 'artifact'
    ? 'Siden ble hentet, men forhåndsvisningen kunne ikke lese tekstinnholdet fra Quarry-artefakten ennå — bruk Crawl for hele nettstedet, eller prøv igjen.'
    : 'Ingen tekst ble hentet fra siden — prøv en annen lenke, eller bruk Crawl for et helt nettsted.')

  const toggle = (index: number) => {
    setSelected((current) => {
      const next = new Set(current)
      if (next.has(index)) next.delete(index)
      else next.add(index)
      return next
    })
  }
  const enter = (index: number) => setHovered(index)
  const leave = (index: number) => setHovered((current) => (current === index ? null : current))
  const selectAll = () => setSelected(new Set(props.preview.blocks.map((_, index) => index)))
  const selectNone = () => setSelected(new Set<number>())

  const handleAdd = () => {
    if (selectedCount() === 0) return
    const markdown = props.preview.blocks
      .filter((_, index) => selected().has(index))
      .map((block) => block.raw)
      .join('\n\n')
    props.onAdd(markdown, allSelected())
  }

  return (
    <section class="velion-fade-up knowledge-scrape-preview" aria-label="Forhåndsvisning av skrapet side">
      <div class="knowledge-scrape-preview__head">
        <div class="knowledge-scrape-preview__heading">
          <span class="knowledge-scrape-preview__tag">Nettleserøkt · ikke lagt til ennå</span>
          <p class="knowledge-scrape-preview__title">{props.preview.title}</p>
          <a
            href={props.preview.url}
            target="_blank"
            rel="noopener noreferrer"
            class="knowledge-scrape-preview__src"
          >
            {hostnameOf(props.preview.url)}
            <ExternalLink class="size-3" aria-hidden="true" />
          </a>
        </div>
        <span class="knowledge-scrape-preview__stat">
          {selectedCount()}/{total()} valgt · {selectedChars().toLocaleString('nb-NO')} tegn
        </span>
      </div>

      <Show when={props.preview.description}>
        <p class="knowledge-scrape-preview__desc">{props.preview.description}</p>
      </Show>

      <Show
        when={total() > 0}
        fallback={<p class="knowledge-scrape-preview__empty">{emptyMessage()}</p>}
      >
        <BrowserSessionSurface
          browserBusy={props.browserBusy}
          browserLoop={props.browserLoop}
          browserRationales={props.browserRationales}
          hovered={hovered()}
          onBrowserAction={props.onBrowserAction}
          onBrowserAutoRun={props.onBrowserAutoRun}
          onBrowserLoopPause={props.onBrowserLoopPause}
          onBrowserLoopResume={props.onBrowserLoopResume}
          onBrowserLoopStop={props.onBrowserLoopStop}
          onBrowserSuggestAction={props.onBrowserSuggestAction}
          onEnter={enter}
          onLeave={leave}
          onSelectAll={selectAll}
          onSelectNone={selectNone}
          onToggle={toggle}
          preview={props.preview}
          profileProbe={props.profileProbe}
          selected={selected()}
          selectedChars={selectedChars()}
          selectedCount={selectedCount()}
          total={total()}
        />
      </Show>

      <div class="knowledge-scrape-preview__actions">
        <button type="button" class="knowledge-scrape-preview__discard" onClick={() => props.onDiscard()} disabled={props.adding}>
          <X class="size-4" /> Forkast
        </button>
        <button
          type="button"
          class="knowledge-scrape-preview__add"
          onClick={handleAdd}
          disabled={props.adding || selectedCount() === 0}
        >
          <Show when={!props.adding} fallback={<Loader2 class="size-4 dashboard-xsearch-spin" />}>
            <Check class="size-4" />
          </Show>
          {allSelected() ? 'Legg til hele siden' : `Legg til ${selectedCount()} valgte`}
        </button>
      </div>
    </section>
  )
}
