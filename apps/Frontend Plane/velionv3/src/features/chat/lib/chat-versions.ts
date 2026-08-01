/**
 * Edit/regenerate version navigation for the FINAL exchange (chat-parity §8,
 * "#49 part 2" — see `apps/Model Plane/docs/CHAT_RESUME_AND_VERSIONS_SPEC.md`).
 *
 * Design constraints, from the adversarial review that rejected the first
 * (persisted, nestable) design:
 *
 *  - CLIENT-ONLY, session-lifetime state. Nothing here touches session-core,
 *    so superseded answers can never diverge the server prompt from the view,
 *    and skill-learning never reads a discarded answer. On reload the
 *    siblings are gone (same contract as `followUps`/`branchCount`).
 *  - GUARDED TO THE NEWEST EXCHANGE. Only the trailing exchange (the last
 *    user turn and everything after it) is ever versioned; editing an earlier
 *    turn keeps today's truncate-and-resend. That makes nested version trees
 *    structurally impossible — the rejected design's fatal flaw.
 *
 * Model: the currently-displayed version of the tail lives in `turns` itself
 * (never duplicated here); `siblings` holds every OTHER version, in logical
 * order with the displayed one occupying logical position `activeIndex`.
 * Total versions = siblings.length + 1. All functions are pure — the caller
 * owns the store and applies the returned arrays.
 */

import type { ChatTurn } from '../components/chat-types'

export type ExchangeVersionState = {
  /** Thread the versions belong to — a thread switch invalidates them. */
  threadId: string | null
  /** Index in `turns` where the versioned exchange starts (its user turn). */
  anchorIndex: number
  /** Every non-displayed sibling tail, in logical (creation) order. */
  siblings: ChatTurn[][]
  /** Logical position of the tail currently materialized in `turns`. */
  activeIndex: number
}

export type VersionBadge = {
  /** 1-based position of the displayed version. */
  current: number
  /** Total sibling versions of the final exchange. */
  total: number
}

export function lastUserIndex(turns: readonly ChatTurn[]): number {
  for (let i = turns.length - 1; i >= 0; i -= 1) {
    if (turns[i]?.role === 'user') return i
  }
  return -1
}

/** Deep-copy a tail so a sibling can never alias live store objects. */
const cloneTail = (turns: readonly ChatTurn[], from: number): ChatTurn[] =>
  structuredClone(turns.slice(from)) as ChatTurn[]

const isSameNode = (
  state: ExchangeVersionState | null,
  threadId: string | null,
  anchorIndex: number,
): state is ExchangeVersionState =>
  state !== null && state.threadId === threadId && state.anchorIndex === anchorIndex

/**
 * Snapshot the outgoing final exchange as a sibling, right BEFORE regenerate /
 * edit-of-final replaces it. The replacement (about to stream into `turns`)
 * becomes the newest version, displayed. Returns the previous state unchanged
 * when there is nothing to version (no user turn, or a tail with no assistant
 * answer — a switcher between identical questions would be noise).
 */
export function beginNewVersion(
  prev: ExchangeVersionState | null,
  turns: readonly ChatTurn[],
  threadId: string | null,
): ExchangeVersionState | null {
  const anchorIndex = lastUserIndex(turns)
  if (anchorIndex < 0) return prev
  const tail = cloneTail(turns, anchorIndex)
  if (!tail.some((turn) => turn.role === 'assistant')) return prev

  if (isSameNode(prev, threadId, anchorIndex)) {
    // The displayed tail joins the shelf at its own logical position; the
    // incoming replacement takes the end.
    const siblings = [...prev.siblings]
    siblings.splice(prev.activeIndex, 0, tail)
    return { ...prev, siblings, activeIndex: siblings.length }
  }
  return { threadId, anchorIndex, siblings: [tail], activeIndex: 1 }
}

/**
 * Switch the displayed version of the final exchange to logical position
 * `target`. Returns the new state plus the full replacement `turns` array, or
 * `null` when the request is invalid — unknown target, already displayed, or
 * the version state is stale (thread switched, or the conversation moved past
 * the versioned exchange, in which case the switcher should not render at
 * all — see `versionBadge`).
 */
export function selectVersion(
  state: ExchangeVersionState | null,
  turns: readonly ChatTurn[],
  threadId: string | null,
  target: number,
): { state: ExchangeVersionState; turns: ChatTurn[] } | null {
  if (!state || state.threadId !== threadId) return null
  if (lastUserIndex(turns) !== state.anchorIndex) return null
  const total = state.siblings.length + 1
  if (!Number.isInteger(target) || target < 0 || target >= total) return null
  if (target === state.activeIndex) return null

  // Work in logical space: insert the displayed tail at its position, pick
  // the target, and shelve the rest.
  const logical = [...state.siblings]
  logical.splice(state.activeIndex, 0, cloneTail(turns, state.anchorIndex))
  const selected = logical[target]
  if (!selected) return null
  const siblings = logical.filter((_, index) => index !== target)

  return {
    state: { ...state, siblings, activeIndex: target },
    turns: [...turns.slice(0, state.anchorIndex), ...structuredClone(selected)],
  }
}

/**
 * What the switcher renders, or `null` when it must not render: no versions
 * yet, a different thread, or the conversation has moved on (a newer exchange
 * or a truncation shifted the final user turn away from the anchor).
 */
export function versionBadge(
  state: ExchangeVersionState | null,
  turns: readonly ChatTurn[],
  threadId: string | null,
): VersionBadge | null {
  if (!state || state.threadId !== threadId) return null
  if (state.siblings.length === 0) return null
  if (lastUserIndex(turns) !== state.anchorIndex) return null
  return { current: state.activeIndex + 1, total: state.siblings.length + 1 }
}
