import {
  AlertCircle,
  ArrowLeft,
  ArrowRight,
  Camera,
  Check,
  Code2,
  ExternalLink,
  Eye,
  Globe2,
  Keyboard,
  Loader2,
  LockKeyhole,
  Maximize2,
  MessageSquare,
  MousePointer2,
  Navigation,
  Network,
  RefreshCw,
  Send,
  ShieldCheck,
  Sparkles,
  Terminal,
  TextCursorInput,
  Timer,
  X,
} from 'lucide-solid'
import { createEffect, createMemo, createSignal, For, Match, Show, Switch, untrack, type JSX } from 'solid-js'
import type { BrowserAction, BrowserActionSuggestionResponse } from '@/shared/api/browser-client'
import { browserSessionFromPreview, type BrowserSessionViewModel } from './browser-session'
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

function compactBrowserUrl(value: string): string {
  try {
    const url = new URL(value)
    const path = `${url.pathname}${url.search}`.replace(/\/$/, '')
    return `${url.hostname}${path || '/'}`
  } catch {
    return value
  }
}

function compactArtifactId(value: string): string {
  if (value.length <= 22) return value
  return `${value.slice(0, 9)}...${value.slice(-8)}`
}

function consoleTone(level: string): 'error' | 'warn' | 'info' {
  const normalized = level.toLowerCase()
  if (normalized.includes('error')) return 'error'
  if (normalized.includes('warn')) return 'warn'
  return 'info'
}

function networkTone(status: number): 'error' | 'warn' | 'redirect' | 'ok' {
  if (status >= 500) return 'error'
  if (status >= 400) return 'warn'
  if (status >= 300) return 'redirect'
  return 'ok'
}

function BrowserObservationInspector(props: { session: BrowserSessionViewModel }) {
  const domNodes = () => props.session.domNodes.slice(0, 8)
  const consoleEntries = () => props.session.consoleEntries.slice(0, 5)
  const networkEntries = () => props.session.networkEntries.slice(0, 6)
  const domCount = () => props.session.nodeCount ?? props.session.domNodes.length

  return (
    <aside class="knowledge-browser-inspector" aria-label="Nettleserinspektør">
      <div class="knowledge-browser-inspector__tabs" aria-label="Observasjonspaneler">
        <span class="knowledge-browser-inspector__tab knowledge-browser-inspector__tab--active">
          <Code2 class="size-3.5" /> DOM
        </span>
        <span class="knowledge-browser-inspector__tab">
          <Terminal class="size-3.5" /> Console
        </span>
        <span class="knowledge-browser-inspector__tab">
          <Network class="size-3.5" /> Network
        </span>
        <Show when={props.session.visualObservationArtifactId}>
          <span class="knowledge-browser-inspector__tab">
            <Eye class="size-3.5" /> Vision
          </span>
        </Show>
      </div>

      <Show when={props.session.visualObservationArtifactId}>
        {(artifactId) => (
          <section class="knowledge-browser-inspector__section knowledge-browser-inspector__section--vision">
            <header>
              <span><Eye class="size-3.5" /> Visuelt</span>
              <strong>OpenCV</strong>
            </header>
            <div class="knowledge-browser-vision-artifact">
              <span>visual_observation.json</span>
              <code>{compactArtifactId(artifactId())}</code>
              <Show when={props.session.visualObservationUrl}>
                {(url) => (
                  <a href={url()} target="_blank" rel="noopener noreferrer" aria-label="Åpne visuell observasjon">
                    <ExternalLink class="size-3.5" />
                  </a>
                )}
              </Show>
            </div>
          </section>
        )}
      </Show>

      <section class="knowledge-browser-inspector__section">
        <header>
          <span><MousePointer2 class="size-3.5" /> Interaktive noder</span>
          <strong>{domCount()}</strong>
        </header>
        <div class="knowledge-browser-live-dom">
          <For
            each={domNodes()}
            fallback={<p class="knowledge-browser-inspector__empty">Ingen DOM-noder returnert ennå.</p>}
          >
            {(node) => (
              <div class="knowledge-browser-live-dom__node">
                <span>{node.kind}</span>
                <p>{node.text}</p>
                <Show when={node.selector}>
                  {(selector) => <code>{selector()}</code>}
                </Show>
              </div>
            )}
          </For>
        </div>
      </section>

      <section class="knowledge-browser-inspector__section">
        <header>
          <span><Terminal class="size-3.5" /> Console</span>
          <strong>{props.session.consoleEntries.length}</strong>
        </header>
        <div class="knowledge-browser-console-list">
          <For
            each={consoleEntries()}
            fallback={<p class="knowledge-browser-inspector__empty">Ingen console-hendelser.</p>}
          >
            {(entry) => (
              <div class={`knowledge-browser-console-row knowledge-browser-console-row--${consoleTone(entry.level)}`}>
                <span>{entry.level}</span>
                <p>{entry.text}</p>
              </div>
            )}
          </For>
        </div>
      </section>

      <section class="knowledge-browser-inspector__section">
        <header>
          <span><Network class="size-3.5" /> Network</span>
          <strong>{props.session.networkEntries.length}</strong>
        </header>
        <div class="knowledge-browser-network-list">
          <For
            each={networkEntries()}
            fallback={<p class="knowledge-browser-inspector__empty">Ingen nettverkskall returnert ennå.</p>}
          >
            {(entry) => (
              <div class={`knowledge-browser-network-row knowledge-browser-network-row--${networkTone(entry.status)}`}>
                <span>{entry.method}</span>
                <strong>{entry.status}</strong>
                <p title={entry.url}>{compactBrowserUrl(entry.url)}</p>
              </div>
            )}
          </For>
        </div>
      </section>

      <Show when={props.session.policyDenials.length > 0}>
        <section class="knowledge-browser-inspector__section knowledge-browser-inspector__section--policy">
          <header>
            <span><ShieldCheck class="size-3.5" /> Policy</span>
            <strong>{props.session.policyDenials.length}</strong>
          </header>
          <For each={props.session.policyDenials.slice(0, 4)}>
            {(denial) => (
              <p class="knowledge-browser-policy-denial">
                <AlertCircle class="size-3.5" /> {denial}
              </p>
            )}
          </For>
        </section>
      </Show>
    </aside>
  )
}

function BrowserSessionSurface(props: {
  browserBusy?: boolean
  hovered: number | null
  onBrowserAction?: (action: BrowserAction) => void
  onBrowserSuggestAction?: (goal: string) => Promise<BrowserActionSuggestionResponse | null>
  onEnter: (index: number) => void
  onLeave: (index: number) => void
  onSelectAll: () => void
  onSelectNone: () => void
  onToggle: (index: number) => void
  preview: ScrapePreview
  selected: Set<number>
  selectedChars: number
  selectedCount: number
  total: number
}) {
  const session = createMemo(() => browserSessionFromPreview(props.preview))
  const [brokenFrameUrl, setBrokenFrameUrl] = createSignal<string | null>(null)
  const [addressInput, setAddressInput] = createSignal('')
  const [lastObservedUrl, setLastObservedUrl] = createSignal('')
  const [selectorInput, setSelectorInput] = createSignal('')
  const [textInput, setTextInput] = createSignal('')
  const [keyInput, setKeyInput] = createSignal('Enter')
  const [goalInput, setGoalInput] = createSignal('Capture useful evidence from this page')
  const [lastSuggestion, setLastSuggestion] = createSignal<BrowserActionSuggestionResponse | null>(null)
  const allSelected = () => props.total > 0 && props.selectedCount === props.total
  const isLive = () => session().renderMode === 'chromium'
  const controlsDisabled = () => !isLive() || props.browserBusy || !session().sessionId
  const frameUrl = () => {
    const url = session().frameUrl
    return url && brokenFrameUrl() !== url ? url : null
  }
  const runAction = (action: BrowserAction) => {
    if (controlsDisabled()) return
    props.onBrowserAction?.(action)
  }
  const selector = () => selectorInput().trim()
  const address = () => addressInput().trim()
  const textValue = () => textInput()
  const keyValue = () => keyInput().trim() || 'Enter'
  const selectorActionDisabled = () => controlsDisabled() || selector().length === 0
  const typeActionDisabled = () => selectorActionDisabled() || textValue().length === 0
  const modelActionDisabled = () => controlsDisabled() || !props.onBrowserSuggestAction
  const navigateFromAddress = () => {
    const target = address() || session().url
    if (!target) return
    runAction({ type: 'navigate', url: target })
  }
  const suggestAction = async () => {
    if (modelActionDisabled()) return
    const suggestion = await props.onBrowserSuggestAction?.(goalInput().trim())
    setLastSuggestion(suggestion ?? null)
  }

  createEffect(() => {
    const url = session().url
    if (url && lastObservedUrl() !== url) {
      setAddressInput(url)
      setLastObservedUrl(url)
    }
  })

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

      <div class="knowledge-browser-frame__nav" aria-label="Nettleserkontroller">
        <div class="knowledge-browser-frame__controls">
          <button
            type="button"
            aria-label="Gå tilbake i nettleserøkten"
            title="Gå tilbake"
            disabled={controlsDisabled()}
            onClick={() => runAction({ type: 'back' })}
          >
            <ArrowLeft class="size-3.5" />
          </button>
          <button
            type="button"
            aria-label="Gå fremover i nettleserøkten"
            title="Gå fremover"
            disabled={controlsDisabled()}
            onClick={() => runAction({ type: 'forward' })}
          >
            <ArrowRight class="size-3.5" />
          </button>
          <button
            type="button"
            aria-label="Last nettlesersiden på nytt"
            title="Last på nytt"
            disabled={controlsDisabled()}
            onClick={() => runAction({ type: 'navigate', url: session().url })}
          >
            <RefreshCw class="size-3.5" />
          </button>
        </div>
        <label class="knowledge-browser-frame__address">
          <LockKeyhole class="size-3.5" aria-hidden="true" />
          <input
            value={addressInput()}
            disabled={controlsDisabled()}
            aria-label="Nettleseradresse"
            spellcheck={false}
            onInput={(event) => setAddressInput(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault()
                navigateFromAddress()
              }
            }}
          />
          <button
            type="button"
            aria-label="Naviger til adresse"
            title="Naviger"
            disabled={controlsDisabled() || !address()}
            onClick={navigateFromAddress}
          >
            <Send class="size-3.5" />
          </button>
        </label>
        <span class="knowledge-browser-frame__source">
          <Globe2 class="size-3.5" /> {session().host}
        </span>
      </div>

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

        <Show
          when={isLive()}
          fallback={
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
          }
        >
          <div class="knowledge-browser-live-surface" aria-label="Live nettleserobservasjon">
            <div class="knowledge-browser-live-surface__toolbar">
              <span>{session().status}</span>
              <span>{session().profileLabel}</span>
              <span>{session().domNodes.length}/{session().nodeCount ?? session().domNodes.length} DOM</span>
              <span>{session().networkEntries.length} network</span>
              <Show when={session().frameArtifactId ?? session().screenshotArtifactId}>
                {(artifactId) => <span>shot {artifactId()}</span>}
              </Show>
              <Show when={session().visualObservationArtifactId}>
                <span><Eye class="size-3.5" /> vision</span>
              </Show>
              <button
                type="button"
                aria-label="Ta skjermbilde"
                title="Skjermbilde"
                disabled={controlsDisabled()}
                onClick={() => runAction({ type: 'screenshot', full_page: false })}
              >
                <Camera class="size-3.5" />
              </button>
            </div>
            <div class="knowledge-browser-actionbar" aria-label="Nettleserhandlinger">
              <label class="knowledge-browser-actionbar__field knowledge-browser-actionbar__field--selector">
                <MousePointer2 class="size-3.5" aria-hidden="true" />
                <input
                  value={selectorInput()}
                  disabled={controlsDisabled()}
                  placeholder="CSS selector"
                  spellcheck={false}
                  onInput={(event) => setSelectorInput(event.currentTarget.value)}
                />
              </label>
              <label class="knowledge-browser-actionbar__field">
                <TextCursorInput class="size-3.5" aria-hidden="true" />
                <input
                  value={textInput()}
                  disabled={controlsDisabled()}
                  placeholder="Text"
                  onInput={(event) => setTextInput(event.currentTarget.value)}
                />
              </label>
              <label class="knowledge-browser-actionbar__field knowledge-browser-actionbar__field--key">
                <Keyboard class="size-3.5" aria-hidden="true" />
                <input
                  value={keyInput()}
                  disabled={controlsDisabled()}
                  placeholder="Key"
                  spellcheck={false}
                  onInput={(event) => setKeyInput(event.currentTarget.value)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter') {
                      event.preventDefault()
                      runAction({ type: 'press', key: keyValue() })
                    }
                  }}
                />
              </label>
              <label class="knowledge-browser-actionbar__field knowledge-browser-actionbar__field--goal">
                <Sparkles class="size-3.5" aria-hidden="true" />
                <input
                  value={goalInput()}
                  disabled={controlsDisabled()}
                  placeholder="AI goal"
                  onInput={(event) => setGoalInput(event.currentTarget.value)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter') {
                      event.preventDefault()
                      void suggestAction()
                    }
                  }}
                />
              </label>
              <div class="knowledge-browser-actionbar__buttons">
                <button
                  type="button"
                  aria-label="Kjør ett AI-foreslått nettlesersteg"
                  title="AI-steg"
                  disabled={modelActionDisabled()}
                  onClick={() => void suggestAction()}
                >
                  <Sparkles class="size-3.5" />
                </button>
                <button
                  type="button"
                  aria-label="Klikk valgt selector"
                  title="Klikk"
                  disabled={selectorActionDisabled()}
                  onClick={() => runAction({ type: 'click', selector: selector() })}
                >
                  <MousePointer2 class="size-3.5" />
                </button>
                <button
                  type="button"
                  aria-label="Skriv tekst i valgt selector"
                  title="Skriv"
                  disabled={typeActionDisabled()}
                  onClick={() => runAction({ type: 'type', selector: selector(), text: textValue() })}
                >
                  <TextCursorInput class="size-3.5" />
                </button>
                <button
                  type="button"
                  aria-label="Send tastetrykk"
                  title="Tast"
                  disabled={controlsDisabled()}
                  onClick={() => runAction({ type: 'press', key: keyValue() })}
                >
                  <Keyboard class="size-3.5" />
                </button>
                <button
                  type="button"
                  aria-label="Vent på valgt selector"
                  title="Vent på selector"
                  disabled={selectorActionDisabled()}
                  onClick={() => runAction({ type: 'wait_for', selector: selector(), timeout_ms: 5000 })}
                >
                  <Timer class="size-3.5" />
                </button>
                <button
                  type="button"
                  aria-label="Rull til valgt selector"
                  title="Rull"
                  disabled={selectorActionDisabled()}
                  onClick={() => runAction({ type: 'scroll', target: selector() })}
                >
                  <Navigation class="size-3.5" />
                </button>
              </div>
            </div>
            <Show when={lastSuggestion()?.suggestion.reason}>
              {(reason) => (
                <div class="knowledge-browser-model-suggestion">
                  <Sparkles class="size-3.5" />
                  <span>{lastSuggestion()?.suggestion.done ? 'done' : lastSuggestion()?.suggestion.action?.type ?? 'no-action'}</span>
                  <p>{reason()}</p>
                </div>
              )}
            </Show>
            <div class="knowledge-browser-live-surface__workspace">
              <div class="knowledge-browser-live-surface__page">
                <header>
                  <span>{session().host}</span>
                  <strong>{session().title}</strong>
                </header>
                <Show
                  when={frameUrl()}
                  keyed
                  fallback={
                    <div class="knowledge-browser-live-dom">
                      <For
                        each={session().domNodes}
                        fallback={<p class="knowledge-browser-inspector__empty">Ingen DOM-noder returnert ennå.</p>}
                      >
                        {(node) => (
                          <div class="knowledge-browser-live-dom__node">
                            <span>{node.kind}</span>
                            <p>{node.text}</p>
                            <Show when={node.selector}>
                              {(selector) => <code>{selector()}</code>}
                            </Show>
                          </div>
                        )}
                      </For>
                    </div>
                  }
                >
                  {(src) => (
                    <div class="knowledge-browser-screenshot" aria-label="Gjengitt Chromium-side">
                      <img
                        src={src}
                        alt={`Gjengitt nettleserside for ${session().title}`}
                        decoding="async"
                        onError={() => setBrokenFrameUrl(src)}
                      />
                    </div>
                  )}
                </Show>
              </div>
              <BrowserObservationInspector session={session()} />
            </div>
          </div>
        </Show>
      </div>

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
  onBrowserAction?: (action: BrowserAction) => void
  onBrowserSuggestAction?: (goal: string) => Promise<BrowserActionSuggestionResponse | null>
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
          hovered={hovered()}
          onBrowserAction={props.onBrowserAction}
          onBrowserSuggestAction={props.onBrowserSuggestAction}
          onEnter={enter}
          onLeave={leave}
          onSelectAll={selectAll}
          onSelectNone={selectNone}
          onToggle={toggle}
          preview={props.preview}
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
