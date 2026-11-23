import { X } from '@/shared/icons'
import { createMemo, onCleanup, onSettled } from 'solid-js'

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
 * While it is open Tab stays inside the sheet (a focus trap), and the page
 * behind it is inert: it takes no focus or clicks and is hidden from assistive
 * technology. The announcer's live regions stay live. Everything is released
 * when the sheet closes, before the opener is focused again. The shell's toast
 * host is part of that background, so a toast raised while the sheet is open is
 * neither announced nor reachable until it closes (normal modal behaviour).
 *
 * The one place the sheet's keydown handler cannot see is the HTML preview
 * frame (drawn only after "Run preview"): key presses inside a frame do not
 * cross into this document. A Tab out of the frame therefore lands on the
 * trailing focus sentinel, which hands focus back to the first stop, so focus
 * still stays inside. Escape pressed inside the frame does not close the
 * sheet; the reader tabs out of the frame first.
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
  let overlay: HTMLDivElement | undefined
  let dialog: HTMLElement | undefined
  const opener: HTMLElement | null =
    typeof document !== 'undefined' && document.activeElement instanceof HTMLElement
      && document.activeElement !== document.body
      ? document.activeElement
      : null

  onCleanup(() => {
    if (opener?.isConnected) queueMicrotask(() => opener.focus())
  })

  // The trap and the inert page are set once the sheet is in the page. The
  // cleanup removes them synchronously, so the opener (possibly behind the
  // sheet until now) can be focused again.
  onSettled(() => {
    const root = overlay
    const sheet = dialog
    if (!root || !sheet) return
    const onFocusIn = (event: FocusEvent) => {
      const target = event.target
      if (!(target instanceof Node) || sheet.contains(target)) return
      // A dialog opened over the sheet (the command palette, say) takes focus on purpose.
      if (target instanceof Element && target.closest('[aria-modal="true"]')) return
      const first = tabStops(sheet)[0] ?? sheet
      first.focus()
    }
    document.addEventListener('focusin', onFocusIn)
    const releaseBackground = makeBackgroundInert(root)
    return () => {
      document.removeEventListener('focusin', onFocusIn)
      releaseBackground()
    }
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
      return
    }
    if (event.key === 'Tab' && dialog) keepTabInside(dialog, event)
  }

  return (
    <div
      ref={(element) => { overlay = element }}
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
        {/* Reached only by a Tab that leaves the preview frame (whose keydown never
            reaches onKeyDown); it is not a tab stop of its own. */}
        <span
          class="sr-only"
          tabindex="0"
          data-focus-sentinel=""
          onFocus={() => {
            if (dialog) (tabStops(dialog)[0] ?? dialog).focus()
          }}
        />
      </section>
    </div>
  )
}

/** What a Tab press can land on. Anything `tabindex="-1"` is script-focusable only and is filtered out below. */
const TAB_STOP_SELECTOR = 'a[href], button, input:not([type="hidden"]), select, textarea, iframe, [tabindex]'

/**
 * The elements in `root` that a Tab press can reach, in document order. Disabled
 * controls and anything hidden or inert (up to `root`) are left out, as the
 * browser's own tab order leaves them out.
 */
function tabStops(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(TAB_STOP_SELECTOR)).filter((element) => {
    if (element.tabIndex < 0 || element.matches(':disabled')) return false
    if (element.hasAttribute('data-focus-sentinel')) return false
    for (let node: HTMLElement | null = element; node && node !== root; node = node.parentElement) {
      if (node.hidden || node.hasAttribute('inert')) return false
    }
    return (element as HTMLElement & { checkVisibility?: () => boolean }).checkVisibility?.() ?? true
  })
}

/**
 * Keeps a Tab press inside the sheet: from its last stop Tab goes to the first,
 * and from its first stop (or the sheet itself) Shift+Tab goes to the last. Any
 * other press is left to the browser, which moves to the next stop in order.
 */
function keepTabInside(root: HTMLElement, event: KeyboardEvent): void {
  const stops = tabStops(root)
  const first = stops[0]
  const last = stops[stops.length - 1]
  if (!first || !last) {
    event.preventDefault()
    root.focus()
    return
  }
  const active = document.activeElement
  const outside = !(active instanceof Node && root.contains(active))
  if (event.shiftKey) {
    if (outside || active === root || active === first) {
      event.preventDefault()
      last.focus()
    }
  } else if (outside || active === last) {
    event.preventDefault()
    first.focus()
  }
}

/**
 * What stays live under the sheet: the announcer's live regions (an inert region is
 * not spoken) and any dialog opened over the sheet, which has to take focus.
 */
const STAYS_LIVE_SELECTOR = '[data-announcer-region], [aria-modal="true"]'

/**
 * Makes the page behind `root` inert: every element beside the path from `root`
 * up to <body>, and every element added beside that path while it stays open.
 * An element that is already inert (another modal's background) is left alone.
 * What stays live is left live, and a wrapper around it is made inert only where
 * it is not the live part itself. Returns the release, which removes only the
 * `inert` that this call added.
 */
function makeBackgroundInert(root: HTMLElement): () => void {
  const marked = new Set<Element>()
  const mark = (element: Element): void => {
    if (element.contains(root) || element.hasAttribute('inert')) return
    if (element.matches(STAYS_LIVE_SELECTOR)) return
    if (element.querySelector(STAYS_LIVE_SELECTOR)) {
      Array.from(element.children).forEach(mark)
      return
    }
    element.setAttribute('inert', '')
    marked.add(element)
  }

  const watched: Element[] = []
  let node: Element = root
  while (node !== document.body && node.parentElement) {
    const parent: Element = node.parentElement
    watched.push(parent)
    Array.from(parent.children).forEach(mark)
    node = parent
  }

  const observer = new MutationObserver((records) => {
    for (const record of records) {
      record.addedNodes.forEach((added) => {
        if (added instanceof Element) mark(added)
      })
    }
  })
  for (const parent of watched) observer.observe(parent, { childList: true })

  return () => {
    observer.disconnect()
    for (const element of marked) element.removeAttribute('inert')
    marked.clear()
  }
}
