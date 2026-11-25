import { createEffect, createSignal } from 'solid-js'

import type { AttentionActor } from '@/shared/api/spaces-attention-items-client'
import { getSpaceAgents, getSpaceRoster, type SpaceAgent, type SpaceRosterMember } from '@/shared/api/spaces-client'

/**
 * The names an attention row can show for the person or agent it is about (WS11-S7).
 *
 * A row carries ids only. A person's name comes from the room's roster, which Control answers only
 * to a member of that room, so a room the viewer has left has no roster and the row keeps its
 * fallback words. Control names only people: it sends an agent (a service member) with an empty
 * display name. An agent's name therefore comes from the room's agent read, where the gateway fills
 * it from the Application binding of that service subject. It is keyed by the agent's subject id and
 * by its agent reference, so the attention actor's agentRef finds it either way. A name is used only
 * when one of these reads sent a non-empty one: an empty name is never replaced by an invented one.
 * The agent read is best effort: when it fails, people are still named and agents keep the fallback.
 *
 * One read serves every row of a room and is reused for {@link ROSTER_NAMES_TTL_MS}. Concurrent
 * callers share the read in flight, and a failed read is not kept, so the next row to ask tries again.
 */

/** How long one room's names are reused before its roster is read again. */
export const ROSTER_NAMES_TTL_MS = 60_000

/** Display names for one room: people by subject id, agents by subject id and by agent reference. */
export type RosterNames = ReadonlyMap<string, string>

type Held = { readonly at: number; readonly names: Promise<RosterNames>; readonly value: RosterNames | undefined }
const held = new Map<string, Held>()

function trimmed(value: unknown): string {
  return typeof value === 'string' ? value.trim() : ''
}

function namesOf(members: readonly SpaceRosterMember[], agents: readonly SpaceAgent[]): RosterNames {
  const names = new Map<string, string>()
  for (const agent of agents) {
    const name = trimmed(agent.name)
    if (!name) continue
    if (agent.subject_id) names.set(agent.subject_id, name)
    if (agent.agent_ref && !names.has(agent.agent_ref)) names.set(agent.agent_ref, name)
  }
  // The roster wins wherever it names someone: it is Control's answer for that subject.
  for (const member of members) {
    const name = trimmed(member.display_name)
    if (member.subject_id && name) names.set(member.subject_id, name)
  }
  return names
}

/** The room's agents, or none when they cannot be read: agent names are best effort. */
function agentsOf(spaceRef: string): Promise<readonly SpaceAgent[]> {
  return Promise.resolve()
    .then(() => getSpaceAgents(spaceRef))
    .then(
      (agents) => (Array.isArray(agents) ? agents : []),
      () => [],
    )
}

/** The room's names when a fresh read has already answered; undefined otherwise. Reads nothing. */
export function knownRosterNames(spaceRef: string, now: number = Date.now()): RosterNames | undefined {
  const entry = held.get(spaceRef)
  return entry?.value !== undefined && now - entry.at < ROSTER_NAMES_TTL_MS ? entry.value : undefined
}

/** The room's names: a fresh read if one is held, otherwise the roster is asked. */
export function rosterNames(spaceRef: string, now: number = Date.now()): Promise<RosterNames> {
  const entry = held.get(spaceRef)
  if (entry && now - entry.at < ROSTER_NAMES_TTL_MS) return entry.names
  const read: Promise<RosterNames> = Promise.all([getSpaceRoster(spaceRef), agentsOf(spaceRef)]).then(
    ([members, agents]) => {
      const names = namesOf(members, agents)
      if (held.get(spaceRef)?.names === read) held.set(spaceRef, { at: now, names: read, value: names })
      return names
    },
    (error: unknown) => {
      if (held.get(spaceRef)?.names === read) held.delete(spaceRef)
      throw error
    },
  )
  held.set(spaceRef, { at: now, names: read, value: undefined })
  return read
}

/** Forgets every held read. Tests only: each test starts with no room's names held. */
export function resetRosterNamesForTests(): void {
  held.clear()
}

/** The subject id an attention actor names: a person's, or an agent's (its roster subject). */
export function actorSubjectId(actor: AttentionActor): string | null {
  if (actor.kind === 'agent') return actor.agentRef
  if (actor.kind === 'user') return actor.subjectId
  return null
}

/** The name the room's roster gives the actor, or undefined when it does not name them. */
export function actorDisplayName(actor: AttentionActor, names: RosterNames | undefined): string | undefined {
  const subject = actorSubjectId(actor)
  return subject && names ? names.get(subject) : undefined
}

/**
 * The room's names for one row, as a reactive value. Undefined until the roster has answered, and
 * again while another room's names load. A roster that cannot be read leaves it undefined for good.
 */
export function createRosterNames(spaceRef: () => string): () => RosterNames | undefined {
  const [names, setNames] = createSignal<RosterNames | undefined>(knownRosterNames(spaceRef()))
  createEffect(spaceRef, (ref) => {
    setNames(knownRosterNames(ref))
    let live = true
    void rosterNames(ref).then(
      (found) => {
        if (live) setNames(found)
      },
      () => {
        // Unknown names are not guessed: the row keeps the fallback words.
      },
    )
    return () => {
      live = false
    }
  })
  return names
}
