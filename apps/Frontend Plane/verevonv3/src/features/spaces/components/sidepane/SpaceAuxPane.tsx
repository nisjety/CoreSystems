import { X } from '@/shared/icons'
import { onCleanup } from 'solid-js'
import type { JSX } from '@solidjs/web'

import { useI18n } from '@/shared/i18n'
import './space-aux-pane.css'

/**
 * The room's right-hand pane: a generalised host that a thread panel (today)
 * and later panels (private panel, browser view, files) plug into.
 *
 * It owns only what every such panel shares: a labelled landmark, a close
 * control, Escape-to-close with focus handed back to whatever opened it, and a
 * keyboard- and pointer-resizable edge. The width is the only thing stored in
 * the browser (`verevon.space.auxpane.width.v1`); no transcript content ever is.
 */

export const SPACE_AUX_PANE_WIDTH_KEY = 'verevon.space.auxpane.width.v1'
export const SPACE_AUX_PANE_MIN_WIDTH = 320
export const SPACE_AUX_PANE_MAX_WIDTH = 720
export const SPACE_AUX_PANE_DEFAULT_WIDTH = 416
/** One keyboard step on the resize separator. */
export const SPACE_AUX_PANE_STEP = 32

export function clampAuxPaneWidth(width: number): number {
  if (!Number.isFinite(width)) return SPACE_AUX_PANE_DEFAULT_WIDTH
  return Math.min(SPACE_AUX_PANE_MAX_WIDTH, Math.max(SPACE_AUX_PANE_MIN_WIDTH, Math.round(width)))
}

/** Storage can be absent, blocked or full; a width is a convenience, so every
 * failure falls back to the default rather than surfacing. */
export function readStoredAuxPaneWidth(): number {
  try {
    const raw = window.localStorage.getItem(SPACE_AUX_PANE_WIDTH_KEY)
    if (raw === null) return SPACE_AUX_PANE_DEFAULT_WIDTH
    const parsed = Number(raw)
    return Number.isFinite(parsed) ? clampAuxPaneWidth(parsed) : SPACE_AUX_PANE_DEFAULT_WIDTH
  } catch {
    return SPACE_AUX_PANE_DEFAULT_WIDTH
  }
}

function writeStoredAuxPaneWidth(width: number): void {
  try {
    window.localStorage.setItem(SPACE_AUX_PANE_WIDTH_KEY, String(width))
  } catch {
    // Not stored; the pane keeps its width for this page view.
  }
}

export interface SpaceAuxPaneProps {
  /** Accessible name of the pane landmark, e.g. the thread's title. */
  readonly label: string
  /** Pane width in px; the host owns it so the room can make space for it. */
  readonly width: number
  readonly onWidthChange: (width: number) => void
  readonly onClose: () => void
  readonly children: JSX.Element
}

export function SpaceAuxPane(props: SpaceAuxPaneProps) {
  const i18n = useI18n()
  let pane: HTMLElement | undefined
  // Whatever held focus when the pane opened: Escape and Close give it back, so
  // a keyboard reader returns to the post they came from.
  let opener: HTMLElement | null =
    typeof document !== 'undefined' && document.activeElement instanceof HTMLElement
      && document.activeElement !== document.body
      ? document.activeElement
      : null

  const setWidth = (next: number) => {
    const width = clampAuxPaneWidth(next)
    if (width === props.width) return
    props.onWidthChange(width)
    writeStoredAuxPaneWidth(width)
  }

  const close = () => {
    props.onClose()
    const target = opener
    opener = null
    if (target?.isConnected) queueMicrotask(() => target.focus())
  }

  // The pane is closed by the host removing it (the URL parameter going away),
  // which also happens on browser back. Return focus there too.
  onCleanup(() => {
    const target = opener
    if (target?.isConnected) queueMicrotask(() => target.focus())
  })

  const focusIn = (element: HTMLElement) => {
    pane = element
    // After layout: the pane is mounted before its children settle.
    queueMicrotask(() => pane?.focus())
  }

  const onPaneKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Escape' && !event.defaultPrevented) {
      event.preventDefault()
      close()
    }
  }

  // The separator is on the pane's left edge, so ArrowLeft makes it wider.
  const onSeparatorKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'ArrowLeft') {
      event.preventDefault()
      setWidth(props.width + SPACE_AUX_PANE_STEP)
    } else if (event.key === 'ArrowRight') {
      event.preventDefault()
      setWidth(props.width - SPACE_AUX_PANE_STEP)
    } else if (event.key === 'Home') {
      event.preventDefault()
      setWidth(SPACE_AUX_PANE_MIN_WIDTH)
    } else if (event.key === 'End') {
      event.preventDefault()
      setWidth(SPACE_AUX_PANE_MAX_WIDTH)
    }
  }

  const onSeparatorPointerDown = (event: PointerEvent) => {
    if (event.button !== 0) return
    event.preventDefault()
    const handle = event.currentTarget as HTMLElement
    const startX = event.clientX
    const startWidth = props.width
    handle.setPointerCapture?.(event.pointerId)
    const move = (moveEvent: PointerEvent) => setWidth(startWidth + (startX - moveEvent.clientX))
    const stop = () => {
      handle.removeEventListener('pointermove', move)
      handle.removeEventListener('pointerup', stop)
      handle.removeEventListener('pointercancel', stop)
    }
    handle.addEventListener('pointermove', move)
    handle.addEventListener('pointerup', stop)
    handle.addEventListener('pointercancel', stop)
  }

  return (
    <aside
      ref={focusIn}
      class="verevon-space-auxpane"
      aria-label={props.label}
      tabindex="-1"
      style={{ width: `${props.width}px` }}
      onKeyDown={onPaneKeyDown}
    >
      <div
        class="verevon-space-auxpane__separator"
        role="separator"
        aria-orientation="vertical"
        aria-label={i18n.tr('Endre bredden på sidepanelet', 'Resize the side panel')}
        aria-valuemin={SPACE_AUX_PANE_MIN_WIDTH}
        aria-valuemax={SPACE_AUX_PANE_MAX_WIDTH}
        aria-valuenow={props.width}
        tabindex="0"
        onKeyDown={onSeparatorKeyDown}
        onPointerDown={onSeparatorPointerDown}
      />
      <div class="verevon-space-auxpane__frame">
        <header class="verevon-space-auxpane__header">
          <h2 class="verevon-space-auxpane__title">{props.label}</h2>
          <button
            type="button"
            class="verevon-space-auxpane__close"
            aria-label={i18n.tr('Lukk sidepanelet', 'Close the side panel')}
            onClick={close}
          >
            <X size={16} aria-hidden="true" />
          </button>
        </header>
        <div class="verevon-space-auxpane__body">{props.children}</div>
      </div>
    </aside>
  )
}
