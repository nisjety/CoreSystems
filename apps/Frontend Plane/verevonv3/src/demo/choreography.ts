/**
 * Layout choreography shared by the product scenes (CONTRACT.md amendment F).
 *
 * The films now open on the product's centred start screen, with no work
 * surface. The product changes layout in single frames when the first message
 * is sent (the composer jumps from the middle of the page to its dock) and when
 * the first evidence arrives (the work surface mounts and the conversation
 * column shrinks). A film must not cut there, so both changes are softened
 * with Web Animations on the real elements. No component is edited and no
 * product CSS changed.
 *
 * Time comes from `document.timeline` (the frame clock the capture harness
 * drives exactly), never from `performance.now()`, so a capture is
 * frame-identical from run to run.
 */

/** The contract's 1 - (1 - p)^3. */
export const EASE_OUT = 'cubic-bezier(0.33, 1, 0.68, 1)'

/** True for a press or click on «Send melding». */
export function isSendPress(event: Event): boolean {
  return event.target instanceof Element && event.target.closest('button[aria-label="Send melding"]') !== null
}

function whenBody(run: () => void): void {
  if (document.body) run()
  else document.addEventListener('DOMContentLoaded', run, { once: true })
}

/**
 * The composer glides from the middle of the start screen to its dock.
 *
 * At the press on «Send melding» the composer's box is measured on the start
 * screen. When the product swaps the start screen for the thread (the same
 * composer node is moved into the dock, a microtask before the next paint) the
 * new box is measured and the composer is animated from where it was to where
 * it is, translation only, so its text never stretches.
 */
export function installComposerTravel(durationMs = 520): void {
  let before: { element: HTMLElement; top: number; left: number } | null = null
  let done = false
  const measure = (event: Event) => {
    if (done || before || !isSendPress(event)) return
    const root = document.querySelector<HTMLElement>('.verevon-chat-empty__composer > *')
    if (!root) return
    const rect = root.getBoundingClientRect()
    before = { element: root, top: rect.top, left: rect.left }
  }
  document.addEventListener('pointerdown', measure, true)
  // The harness presses with a CDP mouse (pointerdown); ?autopilot=1 only clicks.
  document.addEventListener('click', measure, true)
  const observer = new MutationObserver(() => {
    if (done || !before) return
    if (document.querySelector('.verevon-chat-empty')) return
    const element = before.element.isConnected
      ? before.element
      : document.querySelector<HTMLElement>('.verevon-chat-composer-dock__inner > :last-child')
    if (!element) return
    const after = element.getBoundingClientRect()
    const dy = before.top - after.top
    const dx = before.left - after.left
    done = true
    observer.disconnect()
    if (Math.abs(dy) < 1 && Math.abs(dx) < 1) return
    element.animate(
      [{ transform: `translate(${dx}px, ${dy}px)` }, { transform: 'translate(0, 0)' }],
      { duration: durationMs, easing: EASE_OUT },
    )
  })
  whenBody(() => observer.observe(document.body, { childList: true, subtree: true }))
}

/**
 * The work surface opens by growing from the right edge instead of appearing in
 * one frame, so the conversation column narrows continuously and the composer
 * and the thread glide with it. The surface keeps its content at full width
 * (`scene.css` gives its head and body a fixed minimum width), so nothing
 * inside it reflows while it grows; it is revealed.
 */
export function installCanvasOpen(selector: string, durationMs = 520): void {
  const seen = new WeakSet<Element>()
  const open = (node: Element) => {
    if (!(node instanceof HTMLElement) || seen.has(node)) return
    seen.add(node)
    const width = node.getBoundingClientRect().width
    if (width < 40) return
    node.animate(
      [{ flexBasis: '0px' }, { flexBasis: `${width}px` }],
      { duration: durationMs, easing: EASE_OUT },
    )
  }
  const observer = new MutationObserver((records) => {
    for (const record of records) {
      for (const node of record.addedNodes) {
        if (!(node instanceof Element)) continue
        if (node.matches(selector)) open(node)
        for (const child of node.querySelectorAll(selector)) open(child)
      }
    }
  })
  whenBody(() => observer.observe(document.body, { childList: true, subtree: true }))
}
