import { describe, expect, it } from 'vitest'

import {
  decideScroll,
  distanceFromBottom,
  isPinnedToBottom,
  PINNED_BOTTOM_THRESHOLD_PX,
  postPosition,
  shouldShowJumpPill,
} from './timeline-anchor'

// WS15-S1: the room timeline's scroll policy. Nothing moves under a reader who is reading.
describe('the room timeline scroll policy (WS15-S1)', () => {
  it('a reader\'s own send always follows the bottom, whatever changed', () => {
    expect(decideScroll({ change: 'append', pinnedToBottom: false, prevHeight: 1000, nextHeight: 1100, ownSend: true })).toEqual({
      action: 'follow-bottom',
    })
    expect(decideScroll({ change: 'resize', pinnedToBottom: false, prevHeight: 1000, nextHeight: 1040, ownSend: true })).toEqual({
      action: 'follow-bottom',
    })
    expect(decideScroll({ change: 'prepend', pinnedToBottom: false, prevHeight: 1000, nextHeight: 1400, ownSend: true })).toEqual({
      action: 'follow-bottom',
    })
  })

  it('an append follows a reader at the bottom', () => {
    expect(decideScroll({ change: 'append', pinnedToBottom: true, prevHeight: 1000, nextHeight: 1100 })).toEqual({
      action: 'follow-bottom',
    })
  })

  it('an append to a reader scrolled up moves nothing and shows the jump pill with the unseen count', () => {
    expect(decideScroll({ change: 'append', pinnedToBottom: false, prevHeight: 1000, nextHeight: 1100 })).toEqual({
      action: 'show-jump',
      pending: 1,
    })
    expect(
      decideScroll({ change: 'append', pinnedToBottom: false, prevHeight: 1000, nextHeight: 1200, appended: 3, pendingNew: 2 }),
    ).toEqual({ action: 'show-jump', pending: 5 })
  })

  it('a prepend holds the anchor by the anchor shift when known, and by the height change otherwise', () => {
    expect(
      decideScroll({ change: 'prepend', pinnedToBottom: false, prevHeight: 1000, nextHeight: 1500, anchorShift: 420 }),
    ).toEqual({ action: 'hold-anchor', delta: 420 })
    expect(decideScroll({ change: 'prepend', pinnedToBottom: false, prevHeight: 1000, nextHeight: 1500 })).toEqual({
      action: 'hold-anchor',
      delta: 500,
    })
  })

  it('a resize compensates growth above a scrolled-up reader and ignores growth below the anchor', () => {
    // Growth below the anchor: the anchor did not move, so the reader does not either.
    expect(
      decideScroll({ change: 'resize', pinnedToBottom: false, prevHeight: 1000, nextHeight: 1200, anchorShift: 0 }),
    ).toEqual({ action: 'none' })
    // Growth above it pushes the anchor down, and the reader is moved by the same amount.
    expect(
      decideScroll({ change: 'resize', pinnedToBottom: false, prevHeight: 1000, nextHeight: 1040, anchorShift: 36 }),
    ).toEqual({ action: 'hold-anchor', delta: 36 })
    expect(
      decideScroll({ change: 'resize', pinnedToBottom: false, prevHeight: 1000, nextHeight: 960, anchorShift: -40 }),
    ).toEqual({ action: 'hold-anchor', delta: -40 })
  })

  it('a change under one pixel holds nothing, and a non-finite shift counts as none', () => {
    expect(decideScroll({ change: 'resize', pinnedToBottom: false, prevHeight: 1000, nextHeight: 1000.5 })).toEqual({
      action: 'none',
    })
    expect(
      decideScroll({ change: 'prepend', pinnedToBottom: false, prevHeight: 1000, nextHeight: 1000, anchorShift: 0.9 }),
    ).toEqual({ action: 'none' })
    expect(
      decideScroll({ change: 'prepend', pinnedToBottom: false, prevHeight: 1000, nextHeight: 1000, anchorShift: Number.NaN }),
    ).toEqual({ action: 'none' })
    // Exactly one pixel does move the reader.
    expect(
      decideScroll({ change: 'prepend', pinnedToBottom: false, prevHeight: 1000, nextHeight: 1001, anchorShift: 1 }),
    ).toEqual({ action: 'hold-anchor', delta: 1 })
  })

  it('a reader at the bottom follows a resize', () => {
    expect(decideScroll({ change: 'resize', pinnedToBottom: true, prevHeight: 1000, nextHeight: 1100 })).toEqual({
      action: 'follow-bottom',
    })
  })

  it('counts 80 px from the bottom as the pinned threshold, and never reports a negative distance', () => {
    expect(PINNED_BOTTOM_THRESHOLD_PX).toBe(80)
    // scrollHeight 1000 and clientHeight 500: the distance is 500 minus scrollTop.
    const reader = (scrollTop: number) => ({ scrollHeight: 1000, clientHeight: 500, scrollTop })
    expect(distanceFromBottom(reader(421))).toBe(79)
    expect(isPinnedToBottom(reader(421))).toBe(true)
    expect(distanceFromBottom(reader(420))).toBe(80)
    expect(isPinnedToBottom(reader(420))).toBe(false)
    expect(distanceFromBottom({ scrollHeight: 1000, clientHeight: 500, scrollTop: 600 })).toBe(0)
  })

  it('shows the jump pill only away from the bottom, with unseen posts or more than a screen up', () => {
    const atBottom = { scrollHeight: 1000, clientHeight: 500, scrollTop: 500 }
    const aBitUp = { scrollHeight: 1000, clientHeight: 500, scrollTop: 400 }
    const farUp = { scrollHeight: 3000, clientHeight: 500, scrollTop: 0 }
    // At the bottom there is nothing to jump to, even with unseen posts.
    expect(shouldShowJumpPill(atBottom, 4)).toBe(false)
    // Within a screen of the bottom, only unseen posts are worth a pill.
    expect(shouldShowJumpPill(aBitUp, 0)).toBe(false)
    expect(shouldShowJumpPill(aBitUp, 2)).toBe(true)
    // More than a viewport away, the pill is worth showing even with nothing unseen.
    expect(shouldShowJumpPill(farUp, 0)).toBe(true)
  })

  it('postPosition gives 1-based positions for the loaded posts, counting the unloaded ones before them', () => {
    expect(postPosition(0, 5)).toEqual({ posinset: 1, setsize: 5 })
    expect(postPosition(2, 5, 10)).toEqual({ posinset: 13, setsize: 15 })
    expect(postPosition(0, 3, -4)).toEqual({ posinset: 1, setsize: 3 })
  })
})
