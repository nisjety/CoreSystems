// WS12-S7: which edit and delete controls a turn offers, and the calls behind them.
//
// This decides only what to OFFER. Control mints a `model.thread.revise` decision for every change
// and Session Core decides whether the caller is the message's author, the agent's owner or a
// moderator, so a wrong guess here can only show a button that is then refused, or hide one. Edit
// is for a person's own words; delete also covers an agent answer to the viewer's own run, and a
// room manager or owner removing someone else's message where the organisation allows moderation.
import {
  deleteSpaceMessage,
  editSpaceMessage,
  newMessageRevisionKey,
} from '@/shared/api/spaces-client'
import type { SpaceMessagePolicyHints } from '@/shared/api/spaces-client'
import { roleRank } from './space-roles'

/** The slice of a turn the rules read. */
export type LifecycleTurn = {
  readonly role: string
  readonly messageId?: string
  readonly authorSubjectId?: string
  readonly revisionState?: 'edited' | 'deleted'
}

/** What a turn list needs to draw the toolbar; absent means the list is read-only. */
export interface SpaceTurnLifecycle {
  readonly canEdit: (turn: LifecycleTurn) => boolean
  readonly canDelete: (turn: LifecycleTurn) => boolean
  /** Saves new text. Rejects with the ApiError, which the editor words with `translateApiError`. */
  readonly edit: (turn: LifecycleTurn, content: string) => Promise<void>
  readonly remove: (turn: LifecycleTurn) => Promise<void>
}

export interface SpaceTurnLifecycleInput {
  readonly spaceRef: () => string
  readonly threadId: () => string
  /** The `features.message_revise` flag of the Space context. */
  readonly enabled: () => boolean
  readonly viewerSubjectId: () => string | undefined
  readonly viewerRole: () => string | undefined
  /** Who started this thread; an agent answer in the viewer's own thread is theirs to delete. */
  readonly threadOwnerSubjectId: () => string | undefined
  readonly policy: () => SpaceMessagePolicyHints | null | undefined
  readonly archived?: () => boolean
  /** Called after a change landed, so the transcript is read again. */
  readonly onChanged: () => void
}

export function createSpaceTurnLifecycle(input: SpaceTurnLifecycleInput): SpaceTurnLifecycle {
  // One key per attempt: a retry of the same text reuses it (and lands on the same revision), a
  // different text gets a new one.
  const attempts = new Map<string, { readonly text: string; readonly key: string }>()
  const keyFor = (messageId: string, action: string, text: string): string => {
    const id = `${action}:${messageId}`
    const known = attempts.get(id)
    if (known && known.text === text) return known.key
    const fresh = { text, key: newMessageRevisionKey() }
    attempts.set(id, fresh)
    return fresh.key
  }
  const live = (turn: LifecycleTurn): boolean =>
    input.enabled() &&
    input.archived?.() !== true &&
    Boolean(turn.messageId) &&
    turn.revisionState !== 'deleted' &&
    (turn.role === 'user' || turn.role === 'assistant')
  const own = (turn: LifecycleTurn): boolean => {
    const viewer = input.viewerSubjectId()
    return turn.role === 'user' && viewer !== undefined && turn.authorSubjectId === viewer
  }
  const canEdit = (turn: LifecycleTurn): boolean =>
    live(turn) &&
    own(turn) &&
    roleRank(input.viewerRole() ?? '') >= roleRank('editor') &&
    input.policy()?.revise_entitled !== false
  const canDelete = (turn: LifecycleTurn): boolean => {
    if (!live(turn)) return false
    const policy = input.policy()
    const viewer = input.viewerSubjectId()
    const mayRevise = roleRank(input.viewerRole() ?? '') >= roleRank('editor') && policy?.revise_entitled !== false
    if (mayRevise && own(turn)) return true
    // An agent answer in a thread the viewer started is the viewer's run.
    if (mayRevise && turn.role === 'assistant' && viewer !== undefined && input.threadOwnerSubjectId() === viewer) {
      return true
    }
    return roleRank(input.viewerRole() ?? '') >= roleRank('manager') && policy?.moderate_entitled === true
  }
  return {
    canEdit,
    canDelete,
    async edit(turn, content) {
      if (!turn.messageId) throw new Error('message_id_missing')
      await editSpaceMessage(input.spaceRef(), input.threadId(), turn.messageId, {
        content,
        idempotencyKey: keyFor(turn.messageId, 'edit', content),
      })
      input.onChanged()
    },
    async remove(turn) {
      if (!turn.messageId) throw new Error('message_id_missing')
      await deleteSpaceMessage(input.spaceRef(), input.threadId(), turn.messageId, {
        idempotencyKey: keyFor(turn.messageId, 'delete', ''),
        // A moderator removing someone else's message says why in the closed set; the author does not.
        ...(!own(turn) && turn.role === 'user' ? { moderationReason: 'other' as const } : {}),
      })
      input.onChanged()
    },
  }
}
