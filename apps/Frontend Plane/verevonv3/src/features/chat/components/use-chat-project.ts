import { canStartChatProject, isEligibleChatProject } from '@/shared/actions/chat-project'
import type { SpaceThreadTurn } from '@/shared/api/spaces-client'
import type { ChatTurn } from './chat-types'

// The decisions a project chat makes on the page. Each one is a pure function so
// the rules can be pinned without a mounted controller. They refuse early for the
// person's benefit only: Control and the gateway decide the same questions again
// on every turn.

/**
 * The Space a send goes to. A thread keeps the project it was started in; a new
 * chat takes the project picked for it; an existing chat that is not a project
 * never gains one, so a picked project cannot leak into a thread it did not start.
 */
export function requestedProjectRef(input: {
  readonly isNewThread: boolean
  readonly threadRef?: string
  readonly pickedRef?: string | null
}): string | undefined {
  return input.threadRef ?? (input.isNewThread ? input.pickedRef || undefined : undefined)
}

export type ChatProjectRefusal = 'temporary' | 'not_eligible' | 'role_not_allowed'

/**
 * Why a project cannot take this send, or null when it can. Temporary chat writes
 * no thread, so it cannot belong to a project. A viewer reads a room without
 * posting to it, and an organization channel is never a project.
 */
export function chatProjectRefusal(input: {
  readonly temporary: boolean
  readonly role: string | undefined
  readonly space: {
    readonly kind: string
    readonly lifecycle: string
    readonly is_organization_room?: boolean
  } | null | undefined
}): ChatProjectRefusal | null {
  if (input.temporary) return 'temporary'
  if (!input.space || !isEligibleChatProject(input.space)) return 'not_eligible'
  return canStartChatProject(input.role) ? null : 'role_not_allowed'
}

/**
 * A shared room asks once per project per browser before the first send, because
 * every member can read what is posted there. A personal Space never asks.
 */
export function audienceConfirmRequired(input: { readonly kind: string; readonly confirmed: boolean }): boolean {
  return input.kind === 'room' && !input.confirmed
}

/** DEC-06 D5: sandbox tools are refused in a room project, so a Do turn there says so up front. */
export function sandboxToolsRefusedNotice(input: { readonly kind: string | undefined; readonly doMode: boolean }): boolean {
  return input.kind === 'room' && input.doMode
}

/**
 * A room transcript's user and assistant turns as chat turns. The room carries no
 * timestamps or tool records, so those stay empty and the caller fills the times.
 * Ids are positional, so reopening the same transcript keeps the same turns.
 */
export function projectTurnsFromTranscript(threadId: string, turns: readonly SpaceThreadTurn[]): ChatTurn[] {
  const chatTurns: ChatTurn[] = []
  for (const turn of turns) {
    if (turn.role !== 'user' && turn.role !== 'assistant') continue
    chatTurns.push({
      id: `${threadId}:room:${chatTurns.length}`,
      role: turn.role,
      content: turn.content,
      createdAt: '',
      streaming: false,
      tools: [],
      attachments: [],
    })
  }
  return chatTurns
}
