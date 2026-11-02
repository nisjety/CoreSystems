import { decideScroll, isPinnedToBottom, shouldShowJumpPill } from './timeline-anchor'

/**
 * The DOM half of the room timeline's scroll policy (WS15-S1). The decisions live in
 * `timeline-anchor.ts` and stay pure; this wires them to a real scroll root: it remembers where the
 * reader is, watches the list for content changes, and applies the policy's answer.
 *
 * The list must be the scroll root and a positioned element (`position: relative`, set by
 * `space-timeline.css` under `data-anchored`), so `offsetTop` of its posts is relative to it. Each post
 * is a direct child `.verevon-room-post` carrying `data-thread-id`.
 */
export interface TimelineAnchorHooks {
  /** Start at the newest post. False when a deep link will scroll to a specific post. */
  readonly startAtBottom: boolean
  /** Whether the reader's own post is the cause of the next change. */
  readonly takeOwnSend: () => boolean
  /** Unseen-post count and pill visibility are owned by the component. */
  readonly setUnseen: (count: number) => void
  readonly setJump: (visible: boolean) => void
  readonly unseen: () => number
  /** Hands the component a function that forces one evaluation (used right after an own send). */
  readonly register: (tick: (() => void) | undefined) => void
}

type Metrics = { scrollHeight: number; scrollTop: number; clientHeight: number }

function postsOf(el: HTMLElement): HTMLElement[] {
  const out: HTMLElement[] = []
  for (const child of Array.from(el.children)) {
    if (child instanceof HTMLElement && child.classList.contains('verevon-room-post')) out.push(child)
  }
  return out
}

/** First post whose bottom edge is below the top of the viewport; the posts are in document order. */
function findAnchor(posts: readonly HTMLElement[], top: number): HTMLElement | null {
  let lo = 0
  let hi = posts.length - 1
  let found = -1
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    const post = posts[mid] as HTMLElement
    if (post.offsetTop + post.offsetHeight > top) {
      found = mid
      hi = mid - 1
    } else {
      lo = mid + 1
    }
  }
  return found >= 0 ? (posts[found] as HTMLElement) : null
}

/** Starts anchoring `el`. Returns the teardown. */
export function startAnchoring(el: HTMLElement, hooks: TimelineAnchorHooks): () => void {
  let pinned = true
  let anchorEl: HTMLElement | null = null
  let anchorTop = 0
  let lastHeight = el.scrollHeight
  let lastFirstId: string | undefined
  let lastLastId: string | undefined
  let frame = 0
  let disposed = false

  const metrics = (): Metrics => el
  const idOf = (post: HTMLElement | undefined) => post?.dataset.threadId

  function snapshot(): void {
    const posts = postsOf(el)
    pinned = isPinnedToBottom(metrics())
    anchorEl = findAnchor(posts, el.scrollTop)
    anchorTop = anchorEl ? anchorEl.offsetTop - el.scrollTop : 0
    lastHeight = el.scrollHeight
    lastFirstId = idOf(posts[0])
    lastLastId = idOf(posts[posts.length - 1])
    if (pinned && hooks.unseen() !== 0) hooks.setUnseen(0)
    hooks.setJump(shouldShowJumpPill(metrics(), pinned ? 0 : hooks.unseen()))
  }

  function tick(): void {
    if (disposed) return
    const posts = postsOf(el)
    const nextHeight = el.scrollHeight
    const firstId = idOf(posts[0])
    const lastId = idOf(posts[posts.length - 1])
    let appended = 0
    let prepended = 0
    if (lastLastId !== undefined && lastId !== lastLastId) {
      for (let i = posts.length - 1; i >= 0; i -= 1) {
        if (idOf(posts[i]) === lastLastId) {
          appended = posts.length - 1 - i
          break
        }
      }
    }
    if (lastFirstId !== undefined && firstId !== lastFirstId) {
      for (let i = 0; i < posts.length; i += 1) {
        if (idOf(posts[i]) === lastFirstId) {
          prepended = i
          break
        }
      }
    }
    const ownSend = hooks.takeOwnSend()
    const heightChanged = nextHeight !== lastHeight
    if (!heightChanged && appended === 0 && prepended === 0 && !ownSend) return

    const shift = anchorEl && anchorEl.isConnected ? anchorEl.offsetTop - el.scrollTop - anchorTop : 0
    const decision = decideScroll({
      change: appended > 0 ? 'append' : prepended > 0 ? 'prepend' : 'resize',
      pinnedToBottom: pinned,
      prevHeight: lastHeight,
      nextHeight,
      ownSend,
      anchorShift: anchorEl && anchorEl.isConnected ? shift : undefined,
      appended,
      pendingNew: hooks.unseen(),
    })

    if (decision.action === 'follow-bottom') {
      el.scrollTop = el.scrollHeight
    } else if (decision.action === 'hold-anchor') {
      el.scrollTop += decision.delta
    } else if (decision.action === 'show-jump') {
      hooks.setUnseen(decision.pending)
      // New posts arrived below a reader who is scrolled up; growth above the anchor is still held.
      if (Math.abs(shift) >= 1) el.scrollTop += shift
    }
    snapshot()
  }

  function schedule(): void {
    if (frame || disposed) return
    frame = requestAnimationFrame(() => {
      frame = 0
      tick()
    })
  }

  function onScroll(): void {
    // Content that changed since the last snapshot is judged against the old position first.
    if (el.scrollHeight !== lastHeight) tick()
    else snapshot()
  }

  const observed = new WeakSet<Element>()
  const resizeObserver = typeof ResizeObserver === 'function' ? new ResizeObserver(schedule) : undefined
  function observeChildren(): void {
    if (!resizeObserver) return
    for (const child of Array.from(el.children)) {
      if (observed.has(child)) continue
      observed.add(child)
      resizeObserver.observe(child)
    }
  }
  observeChildren()
  resizeObserver?.observe(el)
  const mutationObserver = typeof MutationObserver === 'function'
    ? new MutationObserver(() => {
        observeChildren()
        schedule()
      })
    : undefined
  mutationObserver?.observe(el, { childList: true })
  el.addEventListener('scroll', onScroll, { passive: true })
  hooks.register(schedule)

  if (hooks.startAtBottom) el.scrollTop = el.scrollHeight
  snapshot()

  return () => {
    disposed = true
    if (frame) cancelAnimationFrame(frame)
    el.removeEventListener('scroll', onScroll)
    resizeObserver?.disconnect()
    mutationObserver?.disconnect()
    hooks.register(undefined)
  }
}
