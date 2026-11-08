/**
 * One shared `IntersectionObserver` for the room timeline's posts (WS15-S8). A post that comes within
 * `margin` of the scroll root is "near", and the windowed store then reads its transcript tail. The
 * root is the timeline list itself (the single scroll root), because a margin on the implicit root
 * would not reach past the list's own clip edge.
 *
 * Without `IntersectionObserver` (a test DOM, an old engine) every post counts as near at once, which
 * is the pre-windowing behaviour bounded by the store's concurrency.
 */
export type PostWatcher = {
  /** Calls `onChange` with the post's nearness now and whenever it changes. Returns the stop function. */
  observe: (element: Element, onChange: (near: boolean) => void) => () => void
  dispose: () => void
}

export function createVisibilityWatcher(root: () => Element | undefined, margin: string): PostWatcher {
  let observer: IntersectionObserver | undefined
  const listeners = new WeakMap<Element, (near: boolean) => void>()

  function ensure(): IntersectionObserver {
    if (!observer) {
      observer = new IntersectionObserver(
        (entries) => {
          for (const entry of entries) listeners.get(entry.target)?.(entry.isIntersecting)
        },
        { root: root() ?? null, rootMargin: margin },
      )
    }
    return observer
  }

  return {
    observe(element, onChange) {
      if (typeof IntersectionObserver === 'undefined') {
        onChange(true)
        return () => undefined
      }
      listeners.set(element, onChange)
      ensure().observe(element)
      return () => {
        listeners.delete(element)
        observer?.unobserve(element)
      }
    },
    dispose() {
      observer?.disconnect()
      observer = undefined
    },
  }
}
