/**
 * Version navigation over the server's message tree (build plan WS17 S9).
 *
 * On a versioned thread the gateway reports where each turn sits among the
 * turns that share its parent, and the ids of those siblings. The switcher and
 * the rewind confirmation are computed here from that report and nothing else,
 * so the client never guesses a branch. A thread the server has not versioned
 * carries no `version` on its turns, and every function answers "nothing to
 * move" for it. Those threads keep the client-only versions in `chat-versions.ts`.
 *
 * All functions are pure and take the transcript as the client holds it, oldest
 * turn first.
 */

import type { ChatMessageVersion } from '@/shared/api/chat-client'

/** The part of a transcript turn these functions read. */
export type VersionedTurn = {
  id: string
  version?: ChatMessageVersion
}

/** What the switcher shows for a turn that has more than one version. */
export type ServerVersionBadge = {
  /** 1-based place of the turn among its versions. */
  current: number
  /** How many versions the turn has, the one shown included. */
  total: number
  /** The version one step back, or null at the first. */
  previousId: string | null
  /** The version one step forward, or null at the last. */
  nextId: string | null
}

/** What a rewind to a turn would do, for the confirmation the user sees first. */
export type RewindPlan = {
  /** The turns after the rewind target on the active path, which it hides. */
  hiddenTurns: number
  /** The leaf the client displays now, which the move must still find. */
  expectedLeafMessageId: string
}

/** Whether the server reports a version tree for this transcript at all. */
export function canMoveBranches(turns: readonly VersionedTurn[]): boolean {
  return turns.some((turn) => turn.version !== undefined)
}

/**
 * The newest turn on the active path: the leaf a branch move must still find.
 * Empty when the transcript carries no version tree, since no move can be made.
 */
export function activeLeafId(turns: readonly VersionedTurn[]): string {
  for (let index = turns.length - 1; index >= 0; index -= 1) {
    const turn = turns[index]
    if (turn?.version?.onActivePath) return turn.id
  }
  return ''
}

/**
 * The switcher's reading of one turn, or null when the turn has only one
 * version. The neighbours are the turns the switcher moves to: the siblings
 * one place either side of this one, in the server's order.
 */
export function serverVersionBadge(version: ChatMessageVersion | undefined): ServerVersionBadge | null {
  if (!version || version.siblingCount < 2) return null
  const index = version.siblingOrdinal - 1
  return {
    current: version.siblingOrdinal,
    total: version.siblingCount,
    previousId: index > 0 ? (version.siblings[index - 1] ?? null) : null,
    nextId: version.siblings[index + 1] ?? null,
  }
}

/**
 * What a rewind to `messageId` would do, or null when there is nothing to
 * rewind: the turn is not on the active path, the thread has no version tree,
 * or the turn is already the leaf.
 */
export function rewindPlan(turns: readonly VersionedTurn[], messageId: string): RewindPlan | null {
  if (!canMoveBranches(turns)) return null
  const index = turns.findIndex((turn) => turn.id === messageId)
  if (index < 0 || turns[index]?.version?.onActivePath === false) return null
  const hiddenTurns = turns
    .slice(index + 1)
    .filter((turn) => turn.version?.onActivePath !== false).length
  if (hiddenTurns === 0) return null
  return { hiddenTurns, expectedLeafMessageId: activeLeafId(turns) }
}
