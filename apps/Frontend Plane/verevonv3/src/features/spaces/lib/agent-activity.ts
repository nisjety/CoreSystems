import type { SpaceAgent, SpaceRosterMember, SpaceThread } from '@/shared/api/spaces-client'
import { stripMarkdownPreview } from '@/shared/lib/markdown-plain-text'
import { AWAITING_APPROVAL_RUN_STATUS, LIVE_RUN_STATUSES } from './space-thread-presentation'

/**
 * What an agent is doing, for the identity hover card (WS07-S8). Pure: it reads
 * the same `latest_run_status` the page reads and never invents a state. The
 * card leads with the one line that matters most, in this order:
 *
 *   1. blocking  a run in this room is paused for a person ("Waiting for X"),
 *   2. working   a run is queued or running,
 *   3. last      the first 120 plain characters of the agent's latest loaded turn.
 *
 * Until the server derives `awaiting_approval` into the thread projection (WS09/
 * WS10) line 1 simply never appears. Attribution is by display name because a
 * turn carries no agent id: an agent we cannot pin down gets no lead line, and
 * a thread is attributed to an agent only when it is the room's single active
 * agent or the thread's last loaded assistant turn names it. The card never
 * carries approval detail (what the run wants to do) or approval controls.
 */

export const LAST_ENTRY_MAX_CHARS = 120

export type AgentLead =
  | { readonly kind: 'blocking'; readonly ownerSubjectId?: string }
  | { readonly kind: 'working'; readonly threadTitle?: string }
  | { readonly kind: 'last'; readonly text: string; readonly at?: string }
  | { readonly kind: 'none' }

/** A loaded transcript turn, reduced to what attribution needs. */
export type ActivityTurn = {
  readonly threadId?: string
  readonly role: 'user' | 'assistant'
  readonly agentName?: string
  readonly content: string
}

export type AgentActivityInput = {
  /** The roster agent this card is for; undefined (ambiguous or unknown) gives no lead line. */
  readonly agent: SpaceAgent | undefined
  readonly agents: readonly SpaceAgent[]
  readonly threads: readonly SpaceThread[]
  /** Loaded turns, possibly of one thread only (a post) or none (the agent card). */
  readonly turns: readonly ActivityTurn[]
}

function sameName(a: string | undefined, b: string | undefined): boolean {
  const left = a?.trim()
  return Boolean(left) && left === b?.trim()
}

function threadTime(thread: SpaceThread | undefined): string | undefined {
  return thread?.updated_at ?? thread?.latest_run_updated_at
}

export function agentActivity(input: AgentActivityInput): AgentLead {
  const { agent, agents, threads, turns } = input
  if (!agent) return { kind: 'none' }

  const activeAgents = agents.filter((entry) => entry.status === 'active')
  const isSoleActiveAgent = activeAgents.length === 1 && activeAgents[0]?.subject_id === agent.subject_id

  const lastAssistantNameIn = (threadId: string): string | undefined => {
    for (let index = turns.length - 1; index >= 0; index -= 1) {
      const turn = turns[index]
      if (turn && turn.role === 'assistant' && turn.threadId === threadId) return turn.agentName
    }
    return undefined
  }
  const attributable = (thread: SpaceThread) =>
    isSoleActiveAgent || sameName(lastAssistantNameIn(thread.thread_id), agent.name)

  const blocking = threads.find(
    (thread) => thread.latest_run_status === AWAITING_APPROVAL_RUN_STATUS && attributable(thread),
  )
  if (blocking) {
    const owner = blocking.owner_subject_id?.trim()
    return { kind: 'blocking', ...(owner ? { ownerSubjectId: owner } : {}) }
  }

  const working = threads.find(
    (thread) => LIVE_RUN_STATUSES.has(thread.latest_run_status ?? '') && attributable(thread),
  )
  if (working) {
    const title = working.title?.trim()
    return { kind: 'working', ...(title ? { threadTitle: title } : {}) }
  }

  for (let index = turns.length - 1; index >= 0; index -= 1) {
    const turn = turns[index]
    if (!turn || turn.role !== 'assistant' || !sameName(turn.agentName, agent.name)) continue
    const text = stripMarkdownPreview(turn.content).replace(/\s+/g, ' ').trim().slice(0, LAST_ENTRY_MAX_CHARS)
    if (!text) continue
    const at = threadTime(threads.find((thread) => thread.thread_id === turn.threadId))
    return { kind: 'last', text, ...(at ? { at } : {}) }
  }
  return { kind: 'none' }
}

/** Latest time a member started or advanced a post they own; undefined when none. */
export function memberLastPostAt(
  subjectId: string | undefined,
  threads: readonly SpaceThread[],
): string | undefined {
  const wanted = subjectId?.trim()
  if (!wanted) return undefined
  let latest: string | undefined
  for (const thread of threads) {
    if (thread.owner_subject_id?.trim() !== wanted) continue
    const at = threadTime(thread)
    if (at && (!latest || at > latest)) latest = at
  }
  return latest
}

/** The data a hover card renders; strings that need translation are resolved by the card. */
export type IdentityCardModel =
  | {
      readonly kind: 'agent'
      readonly name: string
      readonly color?: string
      readonly title?: string
      readonly lead: AgentLead
      /** Display name of the blocking thread's owner, when the roster resolves it. */
      readonly ownerName?: string
    }
  | {
      readonly kind: 'member'
      readonly name: string
      readonly role?: string
      readonly lastPostAt?: string
    }

export function agentCardModel(input: AgentActivityInput & {
  readonly name: string
  readonly color?: string
  readonly roster: readonly SpaceRosterMember[]
}): IdentityCardModel {
  const lead = agentActivity(input)
  const ownerName = lead.kind === 'blocking' && lead.ownerSubjectId
    ? input.roster.find((member) => member.subject_id === lead.ownerSubjectId)?.display_name?.trim() || undefined
    : undefined
  const title = input.agent?.title?.trim()
  return {
    kind: 'agent',
    name: input.name,
    ...(input.color ? { color: input.color } : {}),
    ...(title ? { title } : {}),
    lead,
    ...(ownerName ? { ownerName } : {}),
  }
}

export function memberCardModel(input: {
  readonly name: string
  readonly subjectId: string | undefined
  readonly roster: readonly SpaceRosterMember[]
  readonly threads: readonly SpaceThread[]
}): IdentityCardModel {
  const subjectId = input.subjectId?.trim()
  const member = subjectId ? input.roster.find((entry) => entry.subject_id === subjectId) : undefined
  const lastPostAt = memberLastPostAt(subjectId, input.threads)
  return {
    kind: 'member',
    name: input.name,
    ...(member?.role ? { role: member.role } : {}),
    ...(lastPostAt ? { lastPostAt } : {}),
  }
}
