import { createEffect, createSignal, Show } from 'solid-js'

import { ArrowDown, ArrowUp } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import './space-more-unread-pill.css'

type Side = 'above' | 'below'

/**
 * The "more unread" pill of the Spaces sidebar (WS16-S10, docs/plans WS16 D8.7).
 *
 * When rooms that carry a server badge (`data-badge-n` on the row, set by the panel from WS11's
 * attention store) are scrolled out of the panel, a pill at the edge says how many and takes the
 * reader to the nearest one. It only OBSERVES the rows the panel already drew: it counts nothing,
 * derives no badge and reads no store of its own, so it cannot disagree with the badges. Rows in a
 * collapsed section have no box and are ignored, since they cannot be scrolled to.
 *
 * The count sits in a polite live region so a screen-reader user hears "3 more unread" without
 * hunting for the badges; the button itself is a real button, reachable by keyboard.
 *
 */
export function SpaceMoreUnreadPill(props: {
  /** The scrolling element the rows live in; it is also the observer's root. */
  container: () => HTMLElement | undefined
}) {
  const i18n = useI18n()
  const [sides, setSides] = createSignal<ReadonlyMap<HTMLElement, Side>>(new Map())
  const count = () => sides().size
  const sorted = () => {
    const entries = [...sides().entries()]
    // DOM order: the first row below the viewport is the nearest one downwards.
    entries.sort(([a], [b]) => (a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1))
    return entries
  }
  const direction = (): Side => (sorted().some(([, side]) => side === 'below') ? 'below' : 'above')

  createEffect(
    () => props.container(),
    (root) => {
      setSides(new Map())
      if (!root || typeof IntersectionObserver === 'undefined') return undefined
      const out = new Map<HTMLElement, Side>()
      const observer = new IntersectionObserver(
        (entries) => {
          for (const entry of entries) {
            const el = entry.target as HTMLElement
            const box = entry.boundingClientRect
            if (entry.isIntersecting || (box.width === 0 && box.height === 0)) {
              out.delete(el)
              continue
            }
            const top = entry.rootBounds?.top ?? root.getBoundingClientRect().top
            out.set(el, box.top < top ? 'above' : 'below')
          }
          setSides(new Map(out))
        },
        { root, threshold: 0.6 },
      )
      // Rows come and go (a badge appears, a section collapses, a search narrows the list), so the
      // observed set is rebuilt after any change to the list, coalesced to one frame. Changes made by
      // the pill itself are ignored or it would watch its own count.
      const scan = () => {
        observer.disconnect()
        out.clear()
        root.querySelectorAll<HTMLElement>('[data-badge-n]').forEach((row) => observer.observe(row))
        if (out.size === 0) setSides(new Map())
      }
      let frame = 0
      const mutations = new MutationObserver((records) => {
        const foreign = records.some((record) => {
          const node = record.target.nodeType === 1 ? (record.target as Element) : record.target.parentElement
          return !node?.closest('.space-more-unread')
        })
        if (!foreign || frame) return
        frame = requestAnimationFrame(() => {
          frame = 0
          scan()
        })
      })
      mutations.observe(root, { childList: true, subtree: true, attributes: true, attributeFilter: ['data-badge-n', 'hidden'] })
      scan()
      return () => {
        if (frame) cancelAnimationFrame(frame)
        mutations.disconnect()
        observer.disconnect()
      }
    },
  )

  function jump(): void {
    const list = sorted()
    const below = list.find(([, side]) => side === 'below')
    const target = below ?? list[list.length - 1]
    if (!target) return
    const [row] = target
    const calm = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches
    row.scrollIntoView({ block: 'center', behavior: calm ? 'auto' : 'smooth' })
    row.querySelector<HTMLElement>('a')?.focus({ preventScroll: true })
  }

  const text = () => i18n.tr(`${count()} flere uleste`, count() === 1 ? '1 more unread' : `${count()} more unread`)
  return (
    <div class="space-more-unread" role="status" aria-live="polite" data-side={count() > 0 ? direction() : undefined}>
      <Show when={count() > 0}>
        <button type="button" class="space-more-unread__button" onClick={jump}>
          {direction() === 'below'
            ? <ArrowDown size={13} strokeWidth={2} aria-hidden="true" />
            : <ArrowUp size={13} strokeWidth={2} aria-hidden="true" />}
          <span>{text()}</span>
        </button>
      </Show>
    </div>
  )
}
