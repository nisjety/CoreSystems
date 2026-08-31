import { PanelRightClose } from '@/shared/icons'
import { readClientValue, writeClientValue } from '@/shared/session/client-storage'
import { createEffect, createSignal } from 'solid-js'
import type { JSX } from '@solidjs/web'
import type { ChatTab } from './chat-types'
import { chatSurfaceSpec } from '../lib/chat-surfaces'

const CHAT_WORKSPACE_CANVAS_WIDTH_KEY = 'verevon.chat.workspaceCanvasWidth.v1'
const DEFAULT_CANVAS_WIDTH = 440
const MIN_CANVAS_WIDTH = 340
const MAX_CANVAS_WIDTH = 880
// Preserve enough reading width that expanding the preview never turns the
// conversation into a narrow, IDE-like gutter.
const MIN_CONVERSATION_WIDTH = 480
const RESIZE_STEP = 32

function readCanvasWidth(): number {
  const stored = Number.parseInt(readClientValue(CHAT_WORKSPACE_CANVAS_WIDTH_KEY) ?? '', 10)
  return Number.isFinite(stored) && stored >= MIN_CANVAS_WIDTH && stored <= MAX_CANVAS_WIDTH
    ? stored
    : DEFAULT_CANVAS_WIDTH
}

/**
 * Contextual workspace rail for the chat surface.
 *
 * The transcript remains the primary reading surface. Evidence, generated
 * files, and execution detail open beside it as a task context, so changing
 * tabs never destroys the user's place in the conversation. The live-run
 * watcher owns the Work rail while a run is active; this canvas hosts the
 * evidence-only destinations (Sources, Output, and Trace) so two rails never
 * compete for the same task.
 */
export function ChatWorkspaceCanvas(props: {
  active: Exclude<ChatTab, 'chat'>
  children: JSX.Element
  navigation: JSX.Element
  onClose: () => void
}) {
  let bodyRef: HTMLDivElement | undefined
  let canvasRef: HTMLElement | undefined
  let stopResize: (() => void) | undefined
  const [canvasWidth, setCanvasWidth] = createSignal(readCanvasWidth())
  const [resizing, setResizing] = createSignal(false)

  const maxCanvasWidth = () => {
    const availableWidth = canvasRef?.parentElement?.getBoundingClientRect().width
      ?? (typeof window === 'undefined' ? MAX_CANVAS_WIDTH + MIN_CONVERSATION_WIDTH : window.innerWidth)
    return Math.max(
      MIN_CANVAS_WIDTH,
      Math.min(MAX_CANVAS_WIDTH, Math.floor(availableWidth - MIN_CONVERSATION_WIDTH)),
    )
  }

  const clampCanvasWidth = (value: number) => Math.round(
    Math.max(MIN_CANVAS_WIDTH, Math.min(value, maxCanvasWidth())),
  )

  const applyCanvasWidth = (value: number, persist = false) => {
    const next = clampCanvasWidth(value)
    setCanvasWidth(next)
    if (persist) writeClientValue(CHAT_WORKSPACE_CANVAS_WIDTH_KEY, String(next))
  }

  const finishResize = () => {
    stopResize?.()
    stopResize = undefined
    if (!resizing()) return
    setResizing(false)
    document.body.classList.remove('verevon-chat-canvas-resizing')
    writeClientValue(CHAT_WORKSPACE_CANVAS_WIDTH_KEY, String(canvasWidth()))
  }

  const beginResize = (event: PointerEvent) => {
    // On phones the canvas is an overlay, not a split pane. A resize affordance
    // there would block a useful edge gesture without changing its layout.
    if (event.button !== 0 || typeof window === 'undefined' || window.innerWidth <= 720) return
    event.preventDefault()
    finishResize()
    const startX = event.clientX
    const startWidth = canvasRef?.getBoundingClientRect().width ?? canvasWidth()
    setResizing(true)
    document.body.classList.add('verevon-chat-canvas-resizing')

    const move = (moveEvent: PointerEvent) => {
      // The canvas sits on the right, so dragging its left edge left widens it.
      applyCanvasWidth(startWidth + startX - moveEvent.clientX)
    }
    const end = () => finishResize()
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', end, { once: true })
    window.addEventListener('pointercancel', end, { once: true })
    stopResize = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', end)
      window.removeEventListener('pointercancel', end)
    }
  }

  const resizeWithKeyboard = (event: KeyboardEvent) => {
    if (event.key === 'ArrowLeft') {
      event.preventDefault()
      applyCanvasWidth(canvasWidth() + RESIZE_STEP, true)
    } else if (event.key === 'ArrowRight') {
      event.preventDefault()
      applyCanvasWidth(canvasWidth() - RESIZE_STEP, true)
    } else if (event.key === 'Home') {
      event.preventDefault()
      applyCanvasWidth(MIN_CANVAS_WIDTH, true)
    } else if (event.key === 'End') {
      event.preventDefault()
      applyCanvasWidth(maxCanvasWidth(), true)
    }
  }

  createEffect(
    () => undefined,
    () => {
      const keepWidthInBounds = () => applyCanvasWidth(canvasWidth())
      window.addEventListener('resize', keepWidthInBounds)
      return () => window.removeEventListener('resize', keepWidthInBounds)
    },
  )

  createEffect(
    () => undefined,
    () => finishResize,
  )

  // Opening a contextual surface is a real focus transition, not just a
  // visual toggle. Move focus into the panel after it mounts so keyboard and
  // screen-reader users do not remain on a now-hidden tab control. The body is
  // programmatically focusable but not part of the normal tab order; the first
  // interactive control inside it remains the next Tab stop.
  createEffect(
    () => props.active,
    () => {
      queueMicrotask(() => bodyRef?.focus({ preventScroll: true }))
    },
  )

  const metadata = () => chatSurfaceSpec(props.active)

  const titleId = () => `verevon-chat-canvas-title-${props.active}`
  const closeCanvas = () => {
    // Capture the owning page before the canvas unmounts. Returning focus to
    // the selected tab keeps the keyboard path symmetrical with the opening
    // handoff above and avoids leaving focus on a detached close button.
    const page = document.querySelector<HTMLElement>('.verevon-chat-page')
    props.onClose()
    // Solid commits the tab change after this handler returns. Defer one
    // macrotask so we focus a control from the committed header rather than a
    // tab that is still carrying the outgoing aria-selected state.
    queueMicrotask(() => setTimeout(() => {
      const selectedSurface = page?.querySelector<HTMLElement>(
        '.verevon-chat-header [role="tab"][aria-selected="true"], .verevon-chat-tabs [role="tab"][aria-selected="true"]',
      )
      const fallbackSurface = page?.querySelector<HTMLElement>('.verevon-chat-header [aria-haspopup="menu"]')
      const focusTarget = selectedSurface ?? fallbackSurface
      focusTarget?.focus()
    }, 0))
  }

  return (
    <aside
      class={{
        'verevon-chat-workspace-canvas': true,
        'verevon-chat-workspace-canvas--resizing': resizing(),
      }}
      aria-labelledby={titleId()}
      style={{ 'flex-basis': `${canvasWidth()}px` }}
      ref={(element) => { canvasRef = element }}
    >
      <div
        class="verevon-chat-workspace-canvas__resize-handle"
        role="separator"
        aria-label="Endre bredde på arbeidsflaten"
        aria-orientation="vertical"
        aria-valuemin={MIN_CANVAS_WIDTH}
        aria-valuemax={maxCanvasWidth()}
        aria-valuenow={canvasWidth()}
        tabindex="0"
        onPointerDown={beginResize}
        onKeyDown={resizeWithKeyboard}
        onDblClick={() => applyCanvasWidth(DEFAULT_CANVAS_WIDTH, true)}
      >
        <span aria-hidden="true" />
      </div>
      <header class="verevon-chat-workspace-canvas__head">
        <h2 id={titleId()} class="sr-only">{metadata().label}</h2>
        <div class="verevon-chat-workspace-canvas__navigation">{props.navigation}</div>
        <button
          type="button"
          class="verevon-chat-workspace-canvas__close"
          aria-label="Lukk arbeidsflate"
          onClick={closeCanvas}
        >
          <PanelRightClose size={16} />
        </button>
      </header>
      <div
        id={`verevon-chat-tabpanel-${props.active}`}
        class="verevon-chat-workspace-canvas__body"
        role="tabpanel"
        aria-labelledby={titleId()}
        tabindex="-1"
        ref={(element) => { bodyRef = element }}
      >
        {props.children}
      </div>
    </aside>
  )
}
