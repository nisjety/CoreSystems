import { createSignal } from 'solid-js'

import {
  buildMentionsPayload,
  MAX_ADDRESSED_AGENTS,
  MAX_MENTIONED_PEOPLE,
  mentionPresent,
  type AddressedAgent,
  type MentionCandidate,
  type MentionedPerson,
} from './mention-model'
import type { SpaceMentionsInput } from '@/shared/api/spaces-client'

/**
 * The composer's mention state (WS08-S6): who the words name (`people`, `typed` agents) and which
 * agents the address bar holds for every post (`locked`). Signals plus a small per-viewer
 * convenience in `localStorage` (the locked agents per thread, the last 20 sent mentions). Browser
 * storage may be empty or throw, so every read and write is guarded and the state works without it.
 * Never an authorization input.
 */

const RECENT_KEY = 'verevon.space.mentions.recent'
const RECENT_MAX = 20
const LOCK_KEY_PREFIX = 'verevon.space.addressed'

function readStorage(key: string): string | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage.getItem(key)
  } catch {
    return null
  }
}

function writeStorage(key: string, value: string | null): void {
  try {
    if (typeof localStorage === 'undefined') return
    if (value === null) localStorage.removeItem(key)
    else localStorage.setItem(key, value)
  } catch {
    // Storage is a convenience; the box works without it.
  }
}

function lockKey(spaceRef: string, threadId: string | undefined): string {
  return `${LOCK_KEY_PREFIX}.${spaceRef}.${threadId ?? 'new'}`
}

/** The last sent mention subject ids for a room, most recent first. */
export function readRecentMentions(spaceRef: string): readonly string[] {
  try {
    const raw = readStorage(`${RECENT_KEY}.${spaceRef}`)
    const parsed: unknown = raw ? JSON.parse(raw) : []
    return Array.isArray(parsed) ? parsed.filter((id): id is string => typeof id === 'string').slice(0, RECENT_MAX) : []
  } catch {
    return []
  }
}

function rememberMentions(spaceRef: string, ids: readonly string[]): void {
  if (ids.length === 0) return
  const merged = [...ids, ...readRecentMentions(spaceRef)]
  const unique = merged.filter((id, index) => merged.indexOf(id) === index).slice(0, RECENT_MAX)
  writeStorage(`${RECENT_KEY}.${spaceRef}`, JSON.stringify(unique))
}

function readLocked(spaceRef: string, threadId: string | undefined): readonly AddressedAgent[] {
  try {
    const raw = readStorage(lockKey(spaceRef, threadId))
    const parsed: unknown = raw ? JSON.parse(raw) : []
    if (!Array.isArray(parsed)) return []
    const agents: AddressedAgent[] = []
    for (const item of parsed) {
      if (!item || typeof item !== 'object') continue
      const entry = item as Record<string, unknown>
      if (typeof entry.subjectId === 'string' && typeof entry.label === 'string') {
        agents.push({
          subjectId: entry.subjectId,
          label: entry.label,
          source: 'locked',
          ...(typeof entry.color === 'string' ? { color: entry.color } : {}),
        })
      }
    }
    return agents.slice(0, MAX_ADDRESSED_AGENTS)
  } catch {
    return []
  }
}

export interface MentionState {
  readonly people: () => readonly MentionedPerson[]
  readonly agents: () => readonly AddressedAgent[]
  /** Pick from the picker: a person is recorded by label, an agent joins the bar as `typed`. Returns false at the agent cap. */
  readonly pick: (candidate: MentionCandidate) => boolean
  readonly toggleLock: (subjectId: string) => void
  readonly removeAgent: (subjectId: string) => void
  /** Text changed: drop the typed picks whose `@label` left the words. Locked agents stay. */
  readonly reconcile: (text: string) => void
  /** The mentions to send with this text, or undefined. */
  readonly payload: (text: string) => SpaceMentionsInput | undefined
  /** A post was accepted: typed picks are spent, locked agents stay, the sent ids become "recent". */
  readonly sent: (payload: SpaceMentionsInput | undefined) => void
  /** The thread the box is aimed at changed: load that thread's locked agents. */
  readonly rebind: (threadId: string | undefined) => void
  readonly clear: () => void
}

export function createMentionState(spaceRef: () => string, threadId: () => string | undefined): MentionState {
  const [people, setPeople] = createSignal<readonly MentionedPerson[]>([])
  const [agents, setAgents] = createSignal<readonly AddressedAgent[]>(readLocked(spaceRef(), threadId()))
  // Plain mirrors so a read straight after a write is current before Solid flushes the signal.
  let peopleNow: readonly MentionedPerson[] = []
  let agentsNow: readonly AddressedAgent[] = agents()
  const writePeople = (next: readonly MentionedPerson[]) => {
    peopleNow = next
    setPeople(next)
  }
  const writeAgents = (next: readonly AddressedAgent[]) => {
    agentsNow = next
    setAgents(next)
  }
  const persistLocks = () => {
    const locked = agentsNow.filter((agent) => agent.source === 'locked')
    writeStorage(
      lockKey(spaceRef(), threadId()),
      locked.length === 0
        ? null
        : JSON.stringify(locked.map(({ subjectId, label, color }) => ({ subjectId, label, ...(color ? { color } : {}) }))),
    )
  }

  return {
    people,
    agents,
    pick(candidate) {
      if (candidate.kind === 'person') {
        if (peopleNow.some((person) => person.subjectId === candidate.subjectId)) {
          writePeople(peopleNow.map((person) => (person.subjectId === candidate.subjectId ? { ...person, label: candidate.label } : person)))
        } else if (peopleNow.length < MAX_MENTIONED_PEOPLE) {
          writePeople([...peopleNow, { subjectId: candidate.subjectId, label: candidate.label }])
        } else {
          return false
        }
        return true
      }
      const existing = agentsNow.find((agent) => agent.subjectId === candidate.subjectId)
      if (existing) {
        writeAgents(agentsNow.map((agent) => (agent === existing ? { ...agent, label: candidate.label } : agent)))
        return true
      }
      if (agentsNow.length >= MAX_ADDRESSED_AGENTS) return false
      writeAgents([
        ...agentsNow,
        { subjectId: candidate.subjectId, label: candidate.label, source: 'typed', ...(candidate.color ? { color: candidate.color } : {}) },
      ])
      return true
    },
    toggleLock(subjectId) {
      writeAgents(agentsNow.map((agent) =>
        agent.subjectId === subjectId ? { ...agent, source: agent.source === 'locked' ? 'typed' : 'locked' } : agent,
      ))
      persistLocks()
    },
    removeAgent(subjectId) {
      writeAgents(agentsNow.filter((agent) => agent.subjectId !== subjectId))
      persistLocks()
    },
    reconcile(text) {
      const keptPeople = peopleNow.filter((person) => mentionPresent(text, person.label))
      if (keptPeople.length !== peopleNow.length) writePeople(keptPeople)
      const keptAgents = agentsNow.filter((agent) => agent.source === 'locked' || mentionPresent(text, agent.label))
      if (keptAgents.length !== agentsNow.length) writeAgents(keptAgents)
    },
    payload: (text) => buildMentionsPayload({ text, people: peopleNow, agents: agentsNow }),
    sent(payload) {
      writePeople([])
      writeAgents(agentsNow.filter((agent) => agent.source === 'locked'))
      if (payload) rememberMentions(spaceRef(), [...payload.agents.map((a) => a.subjectId), ...payload.people.map((p) => p.subjectId)])
    },
    rebind(next) {
      writePeople([])
      writeAgents(readLocked(spaceRef(), next))
    },
    clear() {
      writePeople([])
      writeAgents(agentsNow.filter((agent) => agent.source === 'locked'))
    },
  }
}
