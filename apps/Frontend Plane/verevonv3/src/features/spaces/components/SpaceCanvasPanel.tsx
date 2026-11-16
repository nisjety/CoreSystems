import { createEffect, createMemo, createSignal, onCleanup, Show } from 'solid-js'

import { ChatMarkdown } from '@/features/chat/components/ChatMessages'
import { ApiError, clientError } from '@/shared/api/http'
import {
  getSpaceCanvas,
  type SpaceCanvas,
  type SpaceCanvasVersion,
  type SpaceRosterMember,
} from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import { canvasErrorMessage } from '@/shared/i18n/errors'

import { CANVAS_REFRESH_MS, CANVAS_TAB_ID, canvasVersionMeta } from '../lib/space-canvas'
import { formatWhenWithTime } from '../lib/space-thread-presentation'
import { useActiveSpaceTab } from './SpaceCockpit'
import './space-canvas.css'

/**
 * The room's Canvas tab (WS19-S5): the one shared Markdown document of this Space, read-only in this slice.
 * Editing, saving, the conflict banner, history and restore are later slices of WS19.
 *
 * # Lazy on purpose
 *
 * Panels mount eagerly (the cockpit hides, it never unmounts), so the canvas is read only while this tab is the
 * active one. While it stays active and the document is visible, the head is checked every 15 seconds with
 * `known_version_id`, so an unchanged head moves no text.
 *
 * # States
 *
 * loading, empty (the room has no canvas yet), view (the head as Markdown, with who saved it and when) and
 * unavailable, which names the reason from the typed error and offers Try again. A failed read is never shown as an
 * empty canvas, and a refresh that fails for a moment keeps the text on screen.
 *
 * Presentation only: the gateway and the owning planes decide who may read the canvas.
 */
export interface SpaceCanvasPanelProps {
  readonly spaceRef: string
  /** The room's members, to name who saved a version. Subject ids are never shown. */
  readonly roster?: () => readonly SpaceRosterMember[]
}

type PanelState =
  | { readonly kind: 'loading' }
  | { readonly kind: 'empty' }
  | { readonly kind: 'view'; readonly canvas: SpaceCanvas; readonly head: SpaceCanvasVersion }
  | { readonly kind: 'unavailable'; readonly error: unknown }

export function SpaceCanvasPanel(props: SpaceCanvasPanelProps) {
  const i18n = useI18n()
  const active = useActiveSpaceTab()
  const isActive = createMemo(() => (active() as string) === CANVAS_TAB_ID)
  const [state, setState] = createSignal<PanelState>({ kind: 'loading' })
  const viewState = createMemo(() => {
    const current = state()
    return current.kind === 'view' ? current : undefined
  })
  const unavailableState = createMemo(() => {
    const current = state()
    return current.kind === 'unavailable' ? current : undefined
  })
  // Each read takes a new call id; an answer that lands after a newer read (or after unmount) is dropped.
  let callId = 0

  onCleanup(() => {
    callId += 1
  })

  const headVersionId = (): string | null => {
    const current = state()
    return current.kind === 'view' ? current.head.versionId : null
  }

  // One read. A refresh (`quiet`) sends the head on screen, keeps it unless the head moved, and keeps it when
  // the read fails for a moment. A cold read, or a refused one, is what the person needs to see.
  const read = async (spaceRef: string, quiet: boolean) => {
    const id = ++callId
    if (!quiet && state().kind !== 'view') setState({ kind: 'loading' })
    try {
      const answer = await getSpaceCanvas(spaceRef, quiet ? headVersionId() : null)
      if (id !== callId || answer.unchanged) return
      if (answer.canvas === null) setState({ kind: 'empty' })
      else if (answer.head !== null) setState({ kind: 'view', canvas: answer.canvas, head: answer.head })
      else setState({ kind: 'unavailable', error: clientError(503, 'canvas_unavailable') })
    } catch (error) {
      if (id !== callId) return
      const refused = error instanceof ApiError && error.status >= 400 && error.status < 500
      if (quiet && !refused) return
      setState({ kind: 'unavailable', error })
    }
  }

  createEffect(
    () => (isActive() ? props.spaceRef : ''),
    (spaceRef) => {
      if (!spaceRef) return undefined
      void read(spaceRef, false)
      const refresh = () => {
        if (document.visibilityState === 'visible') void read(spaceRef, true)
      }
      const timer = setInterval(refresh, CANVAS_REFRESH_MS)
      document.addEventListener('visibilitychange', refresh)
      return () => {
        clearInterval(timer)
        document.removeEventListener('visibilitychange', refresh)
      }
    },
  )

  return (
    <section class="verevon-space-view verevon-space-canvas" aria-labelledby="space-canvas-title">
      <div class="verevon-space-view__heading">
        <div>
          <p class="verevon-space-eyebrow">{i18n.tr('Rommets delte dokument', "The room's shared document")}</p>
          <h2 id="space-canvas-title">{i18n.tr('Lerret', 'Canvas')}</h2>
          <p>{i18n.tr(
            'Ett delt dokument for rommet. Medlemmer og agenter skriver det sammen, og hver lagring blir en ny versjon.',
            'One shared document for the room. Members and agents write it together, and each save becomes a new version.',
          )}</p>
        </div>
      </div>

      <Show when={state().kind === 'loading'}>
        <p class="verevon-space-inline-status" role="status">{i18n.tr('Laster lerretet …', 'Loading the canvas…')}</p>
      </Show>

      <Show when={state().kind === 'empty'}>
        <div class="verevon-space-empty-state" role="status">
          <p>{i18n.tr('Rommet har ikke et lerret ennå.', 'This room has no canvas yet.')}</p>
        </div>
      </Show>

      <Show when={unavailableState()}>
        {(unavailable) => (
          <div class="verevon-space-projection-error" role="alert">
            <p>{canvasErrorMessage(unavailable().error, i18n.tr)}</p>
            <button
              type="button"
              class="verevon-space-secondary-action"
              onClick={() => void read(props.spaceRef, false)}
            >
              {i18n.tr('Prøv igjen', 'Try again')}
            </button>
          </div>
        )}
      </Show>

      <Show when={viewState()}>
        {(view) => (
          <>
            <p class="verevon-space-canvas__meta">
              {canvasVersionMeta(
                view().head,
                props.roster?.() ?? [],
                formatWhenWithTime(view().head.createdAt, { locale: i18n.locale() }),
                i18n.tr,
              )}
            </p>
            <div class="verevon-space-canvas__body">
              <ChatMarkdown content={view().head.content ?? ''} />
            </div>
          </>
        )}
      </Show>
    </section>
  )
}
