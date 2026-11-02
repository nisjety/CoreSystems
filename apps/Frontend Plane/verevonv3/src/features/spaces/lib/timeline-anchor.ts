/**
 * The room timeline's scroll policy (WS15-S1), kept pure so it can be unit-tested without a DOM.
 *
 * The timeline is a bottom-weighted list: oldest at the top, newest at the bottom, the reader usually
 * at the bottom. Content changes in three ways, and each needs a different answer so that nothing ever
 * moves under a reader who is reading:
 *
 *  - `prepend`: content arrived above the reader (an earlier page, expanded earlier turns). Hold the
 *    anchor: scroll by exactly what was added, so what the reader looks at does not move.
 *  - `append`: a post arrived at the end. A reader at the bottom follows it; a reader scrolled up is
 *    never moved and is told with the jump pill instead.
 *  - `resize`: something already on screen changed height (a run card expanded, an embed loaded, a
 *    reply streamed in). At the bottom the reader keeps following; scrolled up, growth above the
 *    anchor is compensated and growth below it is ignored.
 *
 * The reader's own send always goes to the bottom. A shared generic copy of this module may move to
 * `shared/lib/scroll-anchor-policy.ts` when the chat transcript adopts it (plan decision D-WS15-4).
 */

/** Distance from the bottom, in pixels, under which a reader counts as "at the bottom" (chat precedent). */
export const PINNED_BOTTOM_THRESHOLD_PX = 80

export type TimelineChange = 'prepend' | 'append' | 'resize'

export interface ScrollDecisionInput {
  readonly change: TimelineChange
  /** Whether the reader was at the bottom BEFORE the change (measured at the last scroll). */
  readonly pinnedToBottom: boolean
  /** Scroll height before and after the change. */
  readonly prevHeight: number
  readonly nextHeight: number
  /** The reader's own post caused this change. */
  readonly ownSend?: boolean
  /**
   * How far the anchor element moved down, when the caller tracks one. Preferred over the height
   * difference for `prepend` and `resize`, because growth below the anchor must not move the reader.
   */
  readonly anchorShift?: number
  /** Posts appended to the end by this change, for the pill's count. */
  readonly appended?: number
  /** Posts already counted as unseen before this change. */
  readonly pendingNew?: number
}

export type ScrollDecision =
  | { readonly action: 'hold-anchor'; readonly delta: number }
  | { readonly action: 'follow-bottom' }
  | { readonly action: 'show-jump'; readonly pending: number }
  | { readonly action: 'none' }

/** The policy. See the module header for the rules. */
export function decideScroll(input: ScrollDecisionInput): ScrollDecision {
  const { change, pinnedToBottom, ownSend } = input
  if (ownSend === true) return { action: 'follow-bottom' }

  if (change === 'append') {
    if (pinnedToBottom) return { action: 'follow-bottom' }
    const pending = Math.max(0, input.pendingNew ?? 0) + Math.max(0, input.appended ?? 1)
    return { action: 'show-jump', pending }
  }

  // prepend and resize: a reader at the bottom stays there; anyone else holds the anchor.
  if (pinnedToBottom) return { action: 'follow-bottom' }
  const raw = input.anchorShift ?? input.nextHeight - input.prevHeight
  const delta = Number.isFinite(raw) ? raw : 0
  if (Math.abs(delta) < 1) return { action: 'none' }
  return { action: 'hold-anchor', delta }
}

/** Pixels between the bottom of the viewport and the bottom of the content. Never negative. */
export function distanceFromBottom(el: { scrollHeight: number; scrollTop: number; clientHeight: number }): number {
  return Math.max(0, el.scrollHeight - el.scrollTop - el.clientHeight)
}

export function isPinnedToBottom(
  el: { scrollHeight: number; scrollTop: number; clientHeight: number },
  threshold: number = PINNED_BOTTOM_THRESHOLD_PX,
): boolean {
  return distanceFromBottom(el) < threshold
}

/**
 * Whether the jump pill is worth showing: there are unseen posts, or the reader is more than a
 * viewport away from the newest content. Within a screen of the bottom there is nothing to jump to.
 */
export function shouldShowJumpPill(
  el: { scrollHeight: number; scrollTop: number; clientHeight: number },
  pending: number,
): boolean {
  if (isPinnedToBottom(el)) return false
  return pending > 0 || distanceFromBottom(el) > el.clientHeight
}

/** How a post at `index` (0-based, in the loaded list) is placed in the list, for `aria-posinset`/`setsize`. */
export function postPosition(index: number, loaded: number, unloadedBefore: number = 0): { posinset: number; setsize: number } {
  const before = Math.max(0, unloadedBefore)
  return { posinset: before + index + 1, setsize: before + Math.max(0, loaded) }
}
