import { createSignal, type Accessor } from 'solid-js'

/**
 * Resizable inbox columns, persisted per-user (localStorage). Widths are driven
 * through CSS custom properties so the responsive media-queries in global.css
 * still win on narrow viewports (they override grid-template-columns wholesale).
 * A future enhancement can sync these to a server-side inbox-preferences surface.
 */

const LIST_KEY = 'velion.inbox.listWidth'
const ASIDE_KEY = 'velion.inbox.asideWidth'

export const LIST_DEFAULT = 340
export const LIST_MIN = 260
export const LIST_MAX = 560
export const ASIDE_DEFAULT = 360
export const ASIDE_MIN = 300
export const ASIDE_MAX = 620

const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), max)

function read(key: string, fallback: number, min: number, max: number): number {
  if (typeof localStorage === 'undefined') return fallback
  const raw = localStorage.getItem(key)
  const parsed = raw ? Number.parseInt(raw, 10) : NaN
  return Number.isFinite(parsed) ? clamp(parsed, min, max) : fallback
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
    startAsideResize: (event) => drag(event, asideWidth(), -1, ASIDE_MIN, ASIDE_MAX, setAsideWidth, ASIDE_KEY),
  }
}
