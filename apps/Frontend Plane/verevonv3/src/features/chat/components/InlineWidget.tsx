/**
 * Inline widgets (EP-4 S1): a closed ```widget fence in a chat assistant answer,
 * drawn in the opaque-origin sandbox frame with the host's theme tokens.
 *
 * The frame is `sandbox="allow-scripts"` and nothing else, the same isolation as
 * `ArtifactHtmlPreview`: `allow-same-origin` beside `allow-scripts` would let the
 * widget lift its own sandbox, and forms, popups and top navigation stay closed.
 * There is no bridge yet: nothing the frame posts is listened to.
 */

import { createContext, createMemo, createSignal, onCleanup, Show, useContext } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { useI18n } from '@/shared/i18n'
import { sandboxWidgetDocument } from '@/shared/lib/sandbox-html'
import { MAX_WIDGET_CHARS, isWithinLiveWindow, widgetThemeCss, widgetTitle, widgetTooLong } from '../lib/inline-widget'
import './inline-widget.css'

/** `VITE_CHAT_INLINE_WIDGETS`, literal `'true'` only. Off: a widget fence is an ordinary code block. */
export const inlineWidgetsEnabled = () => import.meta.env.VITE_CHAT_INLINE_WIDGETS === 'true'

export type InlineWidgetSurfaceKind = 'live' | 'inert'

/**
 * Where `ChatMarkdown` is drawing. Inert by default, so the Space room, people's
 * posts, user messages and off-screen exports keep the code block: a live widget
 * there would run one person's script in another person's browser. Only a chat
 * assistant answer provides `live`.
 */
export const InlineWidgetSurface = createContext<InlineWidgetSurfaceKind>('inert')

/** Whether a widget fence drawn here may run. */
export function useInlineWidgetsLive(): boolean {
  return useContext(InlineWidgetSurface) === 'live' && inlineWidgetsEnabled()
}

/** The sandbox token list. A test pins it; widening it is a security decision. */
export const WIDGET_SANDBOX = 'allow-scripts'

/**
 * Mount order of the widgets on screen, for the live window
 * (`MAX_LIVE_WIDGETS`). Written from a widget's own setup and cleanup.
 */
let nextWidgetSeq = 0
const [mountedWidgets, setMountedWidgets] = createSignal<readonly number[]>([], { ownedWrite: true })

function hostThemeCss(): string {
  const style = getComputedStyle(document.documentElement)
  return widgetThemeCss((name) => style.getPropertyValue(name))
}

/**
 * A closed widget fence. Over `MAX_WIDGET_CHARS` it stays the code block
 * (`fallback`) with a one-line note; otherwise the framed widget.
 */
export function InlineWidget(props: { source: string; fallback: JSX.Element }): JSX.Element {
  const i18n = useI18n()
  return (
    <Show
      when={!widgetTooLong(props.source)}
      fallback={
        <>
          {props.fallback}
          <p class="verevon-inline-widget__note" role="note">
            {i18n.tr(
              `Widgeten er lengre enn ${MAX_WIDGET_CHARS.toLocaleString('nb-NO')} tegn og vises som kode.`,
              `The widget is longer than ${MAX_WIDGET_CHARS.toLocaleString('en-US')} characters and is shown as code.`,
            )}
          </p>
        </>
      }
    >
      <InlineWidgetFrame source={props.source} fallback={props.fallback} />
    </Show>
  )
}

function InlineWidgetFrame(props: { source: string; fallback: JSX.Element }): JSX.Element {
  const i18n = useI18n()
  const seq = nextWidgetSeq++
  setMountedWidgets((list) => [...list, seq])
  onCleanup(() => setMountedWidgets((list) => list.filter((other) => other !== seq)))

  const [showCode, setShowCode] = createSignal(false)
  const [expanded, setExpanded] = createSignal(false)
  // A click on "Vis widget" keeps this one running even when newer widgets arrive.
  const [activated, setActivated] = createSignal(false)
  const live = createMemo(() => activated() || isWithinLiveWindow(mountedWidgets(), seq))

  const title = createMemo(() => widgetTitle(props.source) ?? i18n.tr('Interaktiv widget', 'Interactive widget'))
  // The theme is read once per source: the frame reloads whenever srcdoc changes.
  const documentHtml = createMemo(() => sandboxWidgetDocument(props.source, hostThemeCss()))

  return (
    <figure
      class={{ 'verevon-inline-widget': true, 'verevon-inline-widget--expanded': expanded() }}
      onKeyDown={(event) => {
        if (event.key === 'Escape' && expanded()) setExpanded(false)
      }}
    >
      <figcaption class="verevon-inline-widget__toolbar">
        <span class="verevon-inline-widget__title">{title()}</span>
        <button
          type="button"
          class="verevon-inline-widget__action"
          aria-pressed={showCode() ? 'true' : 'false'}
          onClick={() => setShowCode((open) => !open)}
        >
          {showCode() ? i18n.tr('Skjul kode', 'Hide code') : i18n.tr('Vis kode', 'Show code')}
        </button>
        <button
          type="button"
          class="verevon-inline-widget__action"
          aria-pressed={expanded() ? 'true' : 'false'}
          onClick={() => setExpanded((open) => !open)}
        >
          {expanded() ? i18n.tr('Lukk', 'Close') : i18n.tr('Åpne stort', 'Open large')}
        </button>
      </figcaption>
      <Show
        when={live()}
        fallback={
          <button type="button" class="verevon-inline-widget__activate" onClick={() => setActivated(true)}>
            {i18n.tr('Vis widget', 'Show widget')}
          </button>
        }
      >
        <iframe
          class="verevon-inline-widget__frame"
          title={title()}
          sandbox={WIDGET_SANDBOX}
          srcdoc={documentHtml()}
          referrerpolicy="no-referrer"
          loading="lazy"
        />
      </Show>
      <Show when={showCode()}>
        <div class="verevon-inline-widget__code">{props.fallback}</div>
      </Show>
    </figure>
  )
}
