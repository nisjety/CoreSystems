import { createEffect, createSignal } from 'solid-js'

import type { AttentionActor } from '@/shared/api/spaces-attention-items-client'
import { getSpaceRoster, type SpaceRosterMember } from '@/shared/api/spaces-client'

/**
 * The names an attention row can show for the person or agent it is about (WS11-S7).
 *
 * A row carries ids only. Its name comes from the room's roster, which Control answers only to a
 * member of that room, so a room the viewer has left has no roster and the row keeps its fallback
 * words. A name is used only when the roster sent a non-empty one: an empty display name is never
 * replaced by an invented one (the roster page says the same).
 *
 * One read serves every row of a room and is reused for {@link ROSTER_NAMES_TTL_MS}. Concurrent
 * callers share the read in flight, and a failed read is not kept, so the next row to ask tries again.
 */

/** How long one room's names are reused before its roster is read again. */
export const ROSTER_NAMES_TTL_MS = 60_000

/** Display names by subject id, people and agents alike, for one room. */
export type RosterNames = ReadonlyMap<string, string>

type Held = { readonly at: number; readonly names: Promise<RosterNames>; readonly value: RosterNames | undefined }
const held = new Map<string, Held>()

function namesOf(members: readonly SpaceRosterMember[]): RosterNames {
  const names = new Map<string, string>()
  for (const member of members) {
    const name = typeof member.display_name === 'string' ? member.display_name.trim() : ''
    if (member.subject_id && name) names.set(member.subject_id, name)
  }
  return names
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
  const read: Promise<RosterNames> = getSpaceRoster(spaceRef).then(
    (members) => {
      const names = namesOf(members)
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
