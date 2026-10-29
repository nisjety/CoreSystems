/**
 * In-page cursor overlay. Screenshots never contain the OS cursor, so the
 * visible pointer in a film is this element; the real `:hover` and click come
 * from the harness's CDP mouse events, not from this overlay.
 *
 * 20 px arrow, soft shadow, no pointer events. Driven by `setCursor` through
 * the signal exported here.
 */
import { createSignal } from 'solid-js'
import type { ScenePointerState } from './scene-types'

const [cursorState, setCursorState] = createSignal<ScenePointerState>({ x: 0, y: 0, visible: false, pressed: false })

export function setSceneCursor(x: number, y: number, visible: boolean, pressed: boolean): void {
  setCursorState({ x, y, visible, pressed })
}

export function SceneCursor() {
  return (
    <div
      class="verevon-scene-cursor"
      data-visible={cursorState().visible ? 'true' : 'false'}
      data-pressed={cursorState().pressed ? 'true' : 'false'}
      style={{ transform: `translate3d(${cursorState().x}px, ${cursorState().y}px, 0)` }}
      aria-hidden="true"
    >
      <svg width="20" height="20" viewBox="0 0 20 20" fill="none">
        <path
          d="M3.5 2.5 L3.5 15.5 L7 12.3 L9.4 17.5 L12 16.3 L9.6 11.2 L14.3 11.2 Z"
          fill="#111111"
          stroke="#ffffff"
          stroke-width="1.4"
          stroke-linejoin="round"
        />
      </svg>
    </div>
  )
}
