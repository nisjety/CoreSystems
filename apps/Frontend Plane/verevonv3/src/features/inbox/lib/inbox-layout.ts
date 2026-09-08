import { createSignal, type Accessor } from 'solid-js'

/**
 * Resizable inbox columns, persisted per-user (localStorage). Widths are driven
 * through CSS custom properties so the responsive media-queries in global.css
 * still win on narrow viewports (they override grid-template-columns wholesale).
 * A future enhancement can sync these to a server-side inbox-preferences surface.
 */

const LIST_KEY = 'verevon.inbox.listWidth'
// v2 starts from a width that fits the three-pane laptop layout. The previous
// key could contain values that were silently capped by CSS, making the handle
// appear to move state without moving the panel.
const ASIDE_KEY = 'verevon.inbox.asideWidth.v2'

export const LIST_DEFAULT = 340
export const LIST_MIN = 260
export const LIST_MAX = 560
export const ASIDE_DEFAULT = 280
export const ASIDE_MIN = 220
export const ASIDE_MAX = 620

const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), max)

function read(key: string, fallback: number, min: number, max: number): number {
  const storage = typeof localStorage === 'undefined' ? null : localStorage
  // Some embedded/private browser contexts expose a storage-shaped global but
  // deny or omit its methods. Column resizing is an enhancement, so a broken
  // persistence layer must never stop the Inbox from rendering.
  if (!storage || typeof storage.getItem !== 'function') return fallback
  try {
    const raw = storage.getItem(key)
    const parsed = raw ? Number.parseInt(raw, 10) : NaN
    return Number.isFinite(parsed) ? clamp(parsed, min, max) : fallback
  } catch {
    return fallback
  }
}

function write(key: string, value: number): void {
  try {
    localStorage.setItem(key, String(Math.round(value)))
  } catch {
    /* storage may be unavailable (private mode) — resizing still works in-session */
  }
}

export interface InboxLayout {
  listWidth: Accessor<number>
  asideWidth: Accessor<number>
  resetWidths: () => void
  startListResize: (event: PointerEvent) => void
  startAsideResize: (event: PointerEvent) => void
}

export function createInboxLayout(): InboxLayout {
  const [listWidth, setListWidth] = createSignal(read(LIST_KEY, LIST_DEFAULT, LIST_MIN, LIST_MAX))
  const [asideWidth, setAsideWidth] = createSignal(read(ASIDE_KEY, ASIDE_DEFAULT, ASIDE_MIN, ASIDE_MAX))

  const drag = (
    event: PointerEvent,
    current: number,
    direction: 1 | -1,
    min: number,
    max: number,
    set: (w: number) => void,
    key: string,
  ) => {
    event.preventDefault()
    const startX = event.clientX
    let latest = current
    const move = (e: PointerEvent) => {
      latest = clamp(current + (e.clientX - startX) * direction, min, max)
      set(latest)
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      document.body.style.userSelect = ''
      document.body.style.cursor = ''
      write(key, latest)
    }
    document.body.style.userSelect = 'none'
    document.body.style.cursor = 'col-resize'
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  return {
    listWidth,
    asideWidth,
    resetWidths: () => {
      setListWidth(LIST_DEFAULT)
      setAsideWidth(ASIDE_DEFAULT)
      write(LIST_KEY, LIST_DEFAULT)
      write(ASIDE_KEY, ASIDE_DEFAULT)
    },
    // Handle sits on the LIST's right edge: dragging right widens the list.
    startListResize: (event) => drag(event, listWidth(), 1, LIST_MIN, LIST_MAX, setListWidth, LIST_KEY),
    // Handle sits on the ASIDE's left edge: dragging left widens the aside.
    // Start from the rendered width because the responsive CSS track may have
    // clamped a stored preference to the currently available viewport.
    startAsideResize: (event) => {
      const handle = event.currentTarget as HTMLElement | null
      const renderedWidth = handle?.parentElement
        ?.querySelector<HTMLElement>('.verevon-inbox-aside')
        ?.getBoundingClientRect().width
      drag(
        event,
        renderedWidth && renderedWidth > 0 ? renderedWidth : asideWidth(),
        -1,
        ASIDE_MIN,
        ASIDE_MAX,
        setAsideWidth,
        ASIDE_KEY,
      )
    },
  }
}
