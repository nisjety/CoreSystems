import { Check, ExternalLink, Loader2, X } from 'lucide-solid'
import { createMemo, createSignal, For, Match, Show, Switch, untrack, type JSX } from 'solid-js'
import { hostnameOf, type ScrapeBlock, type ScrapePreview } from './knowledge-preview'

// Inline markdown → safe JSX. We only resolve the tokens that are reliable to
// detect in scraped content — links and inline images, both anchored on an
// explicit http(s) URL — and clean residual emphasis/markers from the text in
// between. Everything is built as real elements (never innerHTML), so an
// untrusted scraped string can't inject markup.
const INLINE_LINK_RE = /(!?)\[([^\]]*)\]\((https?:\/\/[^)\s]+)[^)]*\)/g

function cleanInlineText(value: string): string {
  return value
    .replace(/\*\*|__|[*_`]/g, '')
    .replace(/[-=_~]{3,}/g, ' ')
    .replace(/[ \t]{2,}/g, ' ')
}

function hideBrokenImage(event: Event) {
  const image = event.currentTarget as HTMLImageElement | null
  if (image) image.style.display = 'none'
}

function renderInline(input: string): JSX.Element {
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
    const url = match[3] ?? ''
    if (isImage) {
      nodes.push(
        <img
          class="knowledge-scrape-page__inline-img"
          src={url}
          alt={label}
          loading="lazy"
          onError={hideBrokenImage}
        />,
      )
    } else {
      const text = cleanInlineText(label).trim()
      nodes.push(
        <a href={url} target="_blank" rel="noopener noreferrer">
          {text || hostnameOf(url)}
        </a>,
      )
    }
    lastIndex = match.index + match[0].length
  }
  pushText(input.slice(lastIndex))

  return <>{nodes}</>
}

/** Render one scraped block as the element it represents — heading, standalone
 * image, blockquote, list item, or paragraph — so the left column reads like a
 * page rather than a flat text dump. */
function BlockBody(props: { block: ScrapeBlock }) {
  const raw = () => props.block.raw.trim()
  const imageMatch = () => /^!\[([^\]]*)\]\((https?:\/\/[^)\s]+)[^)]*\)\s*$/.exec(raw())
  const headingMatch = () => /^(#{1,6})\s+(.*)$/s.exec(raw())
  const quoteMatch = () => /^>\s+(.*)$/s.exec(raw())
  const listMatch = () => /^\s*(?:[-*+]|\d+\.)\s+(.*)$/s.exec(raw())

  return (
    <Switch fallback={<p class="knowledge-scrape-page__p">{renderInline(raw())}</p>}>
      <Match when={imageMatch()} keyed>
        {(match) => (
          <figure class="knowledge-scrape-page__figure">
            <img src={match[2]} alt={match[1] ?? ''} loading="lazy" onError={hideBrokenImage} />
            <Show when={match[1]}>
              <figcaption>{match[1]}</figcaption>
            </Show>
          </figure>
        )}
      </Match>
      <Match when={headingMatch()} keyed>
        {(match) => (
          <p
            class="knowledge-scrape-page__heading"
            data-level={Math.min((match[1]?.length ?? 0), 4)}
            role="heading"
            aria-level={Math.min((match[1]?.length ?? 0), 6)}
          >
            {renderInline(match[2] ?? '')}
          </p>
        )}
      </Match>
      <Match when={quoteMatch()} keyed>
        {(match) => <blockquote class="knowledge-scrape-page__quote">{renderInline(match[1] ?? '')}</blockquote>}
      </Match>
      <Match when={listMatch()} keyed>
        {(match) => <p class="knowledge-scrape-page__li">{renderInline(match[1] ?? '')}</p>}
      </Match>
    </Switch>
  )
}

/** A hoverable / selectable region in the rendered page. Hovering or toggling
 * here is mirrored in the block list and vice-versa via the shared state owned
 * by ScrapePreviewPanel. */
function ScrapeRegion(props: {
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
        <BlockBody block={props.block} />
      </div>
    </div>
  )
}

export function ScrapePreviewPanel(props: {
  adding: boolean
  onAdd: (selectedMarkdown: string, allSelected: boolean) => void
  onDiscard: () => void
  preview: ScrapePreview
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
          <span class="knowledge-scrape-preview__tag">Forhåndsvisning · ikke lagt til ennå</span>
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

      <div class="knowledge-scrape-preview__toolbar">
        <p class="knowledge-scrape-preview__hint">Hold over en seksjon for å markere den — klikk for å velge den bort.</p>
        <div class="knowledge-scrape-preview__select-actions">
          <button type="button" onClick={selectAll} disabled={allSelected()}>Velg alle</button>
          <button type="button" onClick={selectNone} disabled={selectedCount() === 0}>Fjern alle</button>
        </div>
      </div>

      <Show
        when={total() > 0}
        fallback={<p class="knowledge-scrape-preview__empty">{emptyMessage()}</p>}
      >
        <div class="knowledge-scrape-preview__split">
          <div class="knowledge-scrape-page" aria-label="Gjengitt side">
            <For each={props.preview.blocks}>
              {(block, index) => (
                <ScrapeRegion
                  block={block}
                  selected={selected().has(index())}
                  hovered={hovered() === index()}
                  onToggle={() => toggle(index())}
                  onEnter={() => enter(index())}
                  onLeave={() => leave(index())}
                />
              )}
            </For>
          </div>

          <div class="knowledge-scrape-list" aria-label="Seksjoner">
            <For each={props.preview.blocks}>
              {(block, index) => {
                const isSelected = () => selected().has(index())
                return (
                  <button
                    type="button"
                    class="knowledge-scrape-block"
                    classList={{
                      'knowledge-scrape-block--heading': block.heading,
                      'knowledge-scrape-block--selected': isSelected(),
                      'knowledge-scrape-block--deselected': !isSelected(),
                      'knowledge-scrape-block--hovered': hovered() === index(),
                    }}
                    aria-pressed={isSelected()}
                    onClick={() => toggle(index())}
                    onMouseEnter={() => enter(index())}
                    onMouseLeave={() => leave(index())}
                  >
                    <span class="knowledge-scrape-block__check" aria-hidden="true">
                      <Show when={isSelected()}>
                        <Check class="size-3" />
                      </Show>
                    </span>
                    <span class="knowledge-scrape-block__text">{block.text}</span>
                  </button>
                )
              }}
            </For>
          </div>
        </div>
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
