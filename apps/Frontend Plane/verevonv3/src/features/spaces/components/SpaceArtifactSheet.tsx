import { X } from '@/shared/icons'
import { createMemo, onCleanup } from 'solid-js'

import { ArtifactsPanel } from '@/features/chat/components/ChatArtifactPanel'
import type { ArtifactPanelItem } from '@/features/chat/components/chat-types'
import { useI18n } from '@/shared/i18n'
import './space-artifacts.css'

/**
 * The per-thread artifact sheet: a modal dialog over the room holding the
 * chat module's `ArtifactsPanel` on the `room` surface, over the artifacts of
 * ONE thread. Hand-rolled `role="dialog"` (like the create-agent dialog), no
 * library: Escape and the backdrop close it and focus goes back to whatever
 * opened it.
 *
 * The room surface decides what may be drawn: document and code as text, HTML
 * only after "Run preview", everything else a "not available" card. Nothing
 * shown here is written to browser storage.
 */
export function SpaceArtifactSheet(props: {
  readonly items: () => readonly ArtifactPanelItem[]
  /** The artifact the reader pressed; opens selected. */
  readonly selectedId?: string
  readonly threadTitle?: string
  readonly onClose: () => void
}) {
  const i18n = useI18n()
  let dialog: HTMLElement | undefined
  const opener: HTMLElement | null =
    typeof document !== 'undefined' && document.activeElement instanceof HTMLElement
      && document.activeElement !== document.body
      ? document.activeElement
      : null

  onCleanup(() => {
    if (opener?.isConnected) queueMicrotask(() => opener.focus())
  })

  // The panel selects its LAST item until the reader picks another, so the
  // pressed artifact is moved to the end for the initial view only (the order is
  // fixed at open; later selection is the panel's own).
  const ordered = createMemo(() => {
    const items = [...props.items()]
    const index = items.findIndex((item) => item.artifact.id === props.selectedId)
    if (index < 0 || index === items.length - 1) return items
    const [picked] = items.splice(index, 1)
    return picked ? [...items, picked] : items
  })

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Escape' && !event.defaultPrevented) {
      event.preventDefault()
      props.onClose()
    }
  }

  return (
    <div
      class="verevon-space-artifact-sheet"
      role="presentation"
      onClick={(event) => { if (event.target === event.currentTarget) props.onClose() }}
    >
      <section
        ref={(element) => {
          dialog = element
          queueMicrotask(() => dialog?.focus())
        }}
        class="verevon-space-artifact-sheet__dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="space-artifact-sheet-title"
        tabindex="-1"
        onKeyDown={onKeyDown}
      >
        <header class="verevon-space-artifact-sheet__header">
          <h3 id="space-artifact-sheet-title">
            {props.threadTitle?.trim()
              ? i18n.tr(`Artefakter i ${props.threadTitle.trim()}`, `Artifacts in ${props.threadTitle.trim()}`)
              : i18n.tr('Artefakter i samtalen', 'Artifacts in this conversation')}
          </h3>
          <button
            type="button"
            class="verevon-space-artifact-sheet__close"
            aria-label={i18n.tr('Lukk', 'Close')}
            onClick={() => props.onClose()}
          >
            <X size={16} aria-hidden="true" />
          </button>
        </header>
        <div class="verevon-space-artifact-sheet__body">
          <ArtifactsPanel items={[...ordered()]} surface="room" />
        </div>
      </section>
    </div>
  )
}
