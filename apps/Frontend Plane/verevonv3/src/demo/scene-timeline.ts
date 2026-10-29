/**
 * Timeline scheduler and pointer interpolation for scenes.
 *
 * Pure where it matters: `pointerAt` is a function of the keyframes, the
 * scene time and a target resolver, so the harness can ask for the pointer at
 * any frame and get the same answer twice. `createSceneScheduler` only ever
 * uses `setTimeout`, never `Date.now`, so it advances under virtual time.
 */
import type {
  SceneEvent,
  ScenePointerKeyframe,
  ScenePointerState,
  SceneTarget,
} from './scene-types'

/** 1 - (1 - p)^3: the contract's ease-out. */
export function easeOut(progress: number): number {
  const p = Math.min(1, Math.max(0, progress))
  return 1 - (1 - p) ** 3
}

export type SceneScheduler = {
  start(): void
  readonly started: boolean
  /** Clears every pending timer. */
  dispose(): void
}

export function createSceneScheduler(
  events: readonly SceneEvent[],
  fire: (event: SceneEvent) => void,
  setTimer: (callback: () => void, ms: number) => number = (callback, ms) => window.setTimeout(callback, ms),
  clearTimer: (id: number) => void = (id) => window.clearTimeout(id),
): SceneScheduler {
  let started = false
  const timers: number[] = []
  const ordered = [...events].sort((a, b) => a.at - b.at)
  return {
    get started() {
      return started
    },
    start() {
      if (started) throw new Error('Scene already started')
      started = true
      for (const event of ordered) {
        timers.push(setTimer(() => fire(event), Math.max(0, event.at)))
      }
    },
    dispose() {
      for (const timer of timers) clearTimer(timer)
      timers.length = 0
    },
  }
}

export type Point = { x: number; y: number }
export type TargetResolver = (target: SceneTarget) => DOMRect | null

function isVisible(element: Element): boolean {
  const rect = element.getBoundingClientRect()
  if (rect.width === 0 && rect.height === 0) return false
  const style = window.getComputedStyle(element)
  return style.visibility !== 'hidden' && style.display !== 'none'
}

function accessibleName(element: Element): string {
  const label = element.getAttribute('aria-label') ?? element.getAttribute('title')
  if (label?.trim()) return label.trim()
  return (element.textContent ?? '').replace(/\s+/g, ' ').trim()
}

const ROLE_SELECTORS: Record<NonNullable<SceneTarget['role']>, string> = {
  button: 'button, [role="button"], input[type="submit"]',
  link: 'a[href], [role="link"]',
  textbox: 'textarea, input, [role="textbox"], [contenteditable="true"]',
  tab: '[role="tab"]',
  any: 'button, a[href], [role="button"], [role="tab"], textarea, input',
}

/** Resolves a target against the live DOM: first visible match wins. */
export function resolveSceneTarget(target: SceneTarget, root: ParentNode = document): DOMRect | null {
  if (target.selector) {
    const candidates = Array.from(root.querySelectorAll(target.selector))
    const match = candidates.find(isVisible)
    return match ? match.getBoundingClientRect() : null
  }
  if (target.name) {
    const wanted = target.name.trim().toLowerCase()
    const candidates = Array.from(root.querySelectorAll(ROLE_SELECTORS[target.role ?? 'any']))
    const match = candidates.find((element) => isVisible(element) && accessibleName(element).toLowerCase() === wanted)
    return match ? match.getBoundingClientRect() : null
  }
  return null
}

function keyframePoint(frame: ScenePointerKeyframe, resolve: TargetResolver, fallback: Point): Point {
  if (frame.target) {
    const rect = resolve(frame.target)
    if (rect) {
      const offset = frame.offset ?? { x: 0.5, y: 0.5 }
      const nudge = frame.nudge ?? { x: 0, y: 0 }
      return {
        x: rect.left + rect.width * offset.x + nudge.x,
        y: rect.top + rect.height * offset.y + nudge.y,
      }
    }
    if (frame.x != null && frame.y != null) return { x: frame.x, y: frame.y }
    return fallback
  }
  return { x: frame.x ?? fallback.x, y: frame.y ?? fallback.y }
}

/**
 * Pointer state at scene time `tMs`. Positions interpolate between keyframes
 * with an ease-out; `visible`/`pressed` step to each keyframe's values at its
 * `at`. Before the first keyframe the pointer is hidden at the first point.
 */
export function pointerAt(
  keyframes: readonly ScenePointerKeyframe[],
  tMs: number,
  resolve: TargetResolver,
): ScenePointerState {
  if (keyframes.length === 0) return { x: 0, y: 0, visible: false, pressed: false }
  const frames = [...keyframes].sort((a, b) => a.at - b.at)
  const first = frames[0]!
  const origin: Point = { x: 0, y: 0 }
  if (tMs < first.at) {
    const point = keyframePoint(first, resolve, origin)
    return { ...point, visible: false, pressed: false }
  }
  let index = 0
  for (let i = 0; i < frames.length; i += 1) {
    if (frames[i]!.at <= tMs) index = i
  }
  const current = frames[index]!
  const next = frames[index + 1]
  const currentPoint = keyframePoint(current, resolve, origin)
  // A hidden pointer does not travel: the next target is left unresolved (and
  // unpinned) until its own keyframe is current, so a button that appears
  // while the pointer is hidden is measured after the layout has settled.
  if (!next || next.ease === 'hold' || !current.visible) {
    return { ...currentPoint, visible: current.visible, pressed: current.pressed }
  }
  const span = next.at - current.at
  const raw = span <= 0 ? 1 : (tMs - current.at) / span
  const progress = next.ease === 'linear' ? Math.min(1, Math.max(0, raw)) : easeOut(raw)
  const nextPoint = keyframePoint(next, resolve, currentPoint)
  return {
    x: currentPoint.x + (nextPoint.x - currentPoint.x) * progress,
    y: currentPoint.y + (nextPoint.y - currentPoint.y) * progress,
    visible: current.visible,
    pressed: current.pressed,
  }
}
