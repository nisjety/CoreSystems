/**
 * Inline widgets (EP-4 S1): a closed ```widget fence in a chat answer, drawn as
 * self-contained HTML or SVG in the opaque-origin sandbox frame.
 *
 * Pure helpers only. Whether a widget runs at all is the surface's call
 * (`InlineWidgetSurface` in `InlineWidget.tsx`): only a chat assistant answer is
 * live, so script in a person's room post never runs in another member's browser.
 */

import type { MarkdownBlock } from '../components/chat-types'

/** The fence language. Only this one: an ```html sample stays code, never script. */
export const WIDGET_FENCE = 'widget'

/** Over this many characters a widget stays a code block with a one-line note. */
export const MAX_WIDGET_CHARS = 100_000

/**
 * Live frames per thread view. Each `srcdoc` frame is a whole document, so a long
 * thread keeps only the newest ones running; older ones wait for a click.
 */
export const MAX_LIVE_WIDGETS = 8

/** A widget is a fence whose language is `widget` and that has closed: a fence still streaming is code. */
export function isWidgetFence(block: Pick<Extract<MarkdownBlock, { kind: 'code' }>, 'lang' | 'closed'>): boolean {
  return block.closed && block.lang.trim().toLowerCase() === WIDGET_FENCE
}

export function widgetTooLong(source: string): boolean {
  return source.length > MAX_WIDGET_CHARS
}

/**
 * The host values a widget may style itself with, as `--vv-*` custom properties.
 * The source is a host custom property or, for size and scheme, the computed
 * property itself, so the widget follows the reader's theme without reading the host.
 */
const THEME_TOKENS: ReadonlyArray<readonly [vv: string, source: string]> = [
  ['--vv-background', '--verevon-bg'],
  ['--vv-surface', '--verevon-surface'],
  ['--vv-text', '--verevon-text'],
  ['--vv-muted', '--verevon-text-muted'],
  ['--vv-accent', '--verevon-accent'],
  ['--vv-border', '--verevon-border'],
  ['--vv-radius', '--radius'],
  ['--vv-font-family', '--font-sans'],
  ['--vv-font-size', 'font-size'],
  ['--vv-color-scheme', 'color-scheme'],
]

/** A token value is a colour, length or font list; anything else could end the declaration or the element. */
const UNSAFE_THEME_CHARS = /[^#a-zA-Z0-9(),.%\s-]/g

/** Longer than any real colour or font list; bounds what a hostile stylesheet could inject. */
const MAX_THEME_VALUE_CHARS = 200

function cleanThemeValue(raw: string): string {
  return raw.replace(UNSAFE_THEME_CHARS, '').replace(/\s+/g, ' ').trim().slice(0, MAX_THEME_VALUE_CHARS)
}

/**
 * The `:root{...}` rule the frame's stylesheet starts with. `getVar` reads one
 * computed value (in the page: `getComputedStyle(document.documentElement).getPropertyValue`).
 * An empty value is left out, so the widget's own fallback applies.
 */
export function widgetThemeCss(getVar: (name: string) => string): string {
  const declarations = THEME_TOKENS.flatMap(([vv, source]) => {
    const value = cleanThemeValue(getVar(source) ?? '')
    return value ? [`${vv}:${value}`] : []
  })
  return `:root{${declarations.join(';')}}`
}

/** Long enough for a heading, short enough for the toolbar. */
const MAX_TITLE_CHARS = 80

/**
 * The widget's own name, from its first `<title>` or `aria-label`; null when it
 * gives none (the caller falls back to "Interaktiv widget").
 */
export function widgetTitle(source: string): string | null {
  const fromTitle = /<title[^>]*>([^<]*)<\/title>/i.exec(source)?.[1]
  const fromLabel = /\saria-label\s*=\s*(?:"([^"]*)"|'([^']*)')/i.exec(source)
  const raw = fromTitle?.trim() || fromLabel?.[1]?.trim() || fromLabel?.[2]?.trim() || ''
  const title = raw.replace(/\s+/g, ' ').slice(0, MAX_TITLE_CHARS)
  return title || null
}

/**
 * Whether the widget mounted as `seq` may run a live frame: it is one of the
 * `max` newest mounted. A seq not yet in `mounted` is the newest by construction.
 */
export function isWithinLiveWindow(mounted: readonly number[], seq: number, max = MAX_LIVE_WIDGETS): boolean {
  let newer = 0
  for (const other of mounted) if (other > seq) newer += 1
  return newer < max
}
