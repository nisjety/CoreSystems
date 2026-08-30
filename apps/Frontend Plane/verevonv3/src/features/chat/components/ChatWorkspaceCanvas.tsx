import { PanelRightClose } from '@/shared/icons'
import { createEffect } from 'solid-js'
import { Dynamic, type JSX } from '@solidjs/web'
import type { ChatTab } from './chat-types'
import { chatSurfaceSpec } from '../lib/chat-surfaces'

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
  onClose: () => void
}) {
  let bodyRef: HTMLDivElement | undefined

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
  const closeCanvas = (event: MouseEvent) => {
    // Capture the owning page before the canvas unmounts. Returning focus to
    // the selected tab keeps the keyboard path symmetrical with the opening
    // handoff above and avoids leaving focus on a detached close button.
    const page = event.currentTarget instanceof Element
      ? event.currentTarget.closest('.verevon-chat-page')
      : null
    props.onClose()
    queueMicrotask(() => {
      page?.querySelector<HTMLElement>('.verevon-chat-tabs [role="tab"][aria-selected="true"]')?.focus()
    })
  }

  return (
    <aside
      class="verevon-chat-workspace-canvas"
      aria-labelledby={titleId()}
    >
      <header class="verevon-chat-workspace-canvas__head">
        <div class="verevon-chat-workspace-canvas__title">
          <span class="verevon-chat-workspace-canvas__icon"><Dynamic component={metadata().icon} size={15} /></span>
          <div>
            <h2 id={titleId()}>{metadata().label}</h2>
            <p>{metadata().description}</p>
          </div>
        </div>
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
