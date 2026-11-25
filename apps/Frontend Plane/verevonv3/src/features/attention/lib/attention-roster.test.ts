import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { AttentionActor } from '@/shared/api/spaces-attention-items-client'
import type { SpaceRosterMember } from '@/shared/api/spaces-client'

// WS11-S7: the roster names an attention row shows. The roster read is replaced; each test states
// what the room's roster answered.

const roster = vi.hoisted(() => ({ getSpaceRoster: vi.fn() }))
vi.mock('@/shared/api/spaces-client', () => ({ getSpaceRoster: roster.getSpaceRoster }))

import {
  actorDisplayName,
  knownRosterNames,
  resetRosterNamesForTests,
  ROSTER_NAMES_TTL_MS,
  rosterNames,
} from './attention-roster'

function member(subjectId: string, displayName: string, subjectType: 'user' | 'service' = 'user'): SpaceRosterMember {
  return { subject_type: subjectType, subject_id: subjectId, role: 'editor', revision: 1, display_name: displayName }
}

const person = (subjectId: string | null): AttentionActor => ({ kind: 'user', subjectId, agentRef: null })
const agent = (agentRef: string | null): AttentionActor => ({ kind: 'agent', subjectId: null, agentRef })

beforeEach(() => {
  roster.getSpaceRoster.mockReset()
  resetRosterNamesForTests()
})

describe('rosterNames', () => {
  it('names people and agents by subject id, trimmed, and leaves out a blank name', async () => {
    roster.getSpaceRoster.mockResolvedValue([
      member('user-1', '  Ola Nordmann '),
      member('agent-1', 'Research bot', 'service'),
      member('user-2', '   '),
    ])

    const names = await rosterNames('room-names')

    expect(names.get('user-1')).toBe('Ola Nordmann')
    expect(names.get('agent-1')).toBe('Research bot')
    expect(names.has('user-2')).toBe(false)
    expect(roster.getSpaceRoster).toHaveBeenCalledWith('room-names')
  })

  it('shares one read between callers in the same room, and reads again once the time to live has passed', async () => {
    roster.getSpaceRoster.mockResolvedValue([member('user-1', 'Ola Nordmann')])
    const now = 1_000_000

    await Promise.all([rosterNames('room-share', now), rosterNames('room-share', now)])
    expect(roster.getSpaceRoster).toHaveBeenCalledTimes(1)

    await rosterNames('room-share', now + ROSTER_NAMES_TTL_MS - 1)
    expect(roster.getSpaceRoster).toHaveBeenCalledTimes(1)

    await rosterNames('room-share', now + ROSTER_NAMES_TTL_MS)
    expect(roster.getSpaceRoster).toHaveBeenCalledTimes(2)
  })

  it('does not keep a failed read, so the next row to ask reads the roster again', async () => {
    roster.getSpaceRoster
      .mockRejectedValueOnce(new Error('unavailable'))
      .mockResolvedValueOnce([member('user-1', 'Ola Nordmann')])

    await expect(rosterNames('room-retry')).rejects.toThrow('unavailable')
    const names = await rosterNames('room-retry')

    expect(names.get('user-1')).toBe('Ola Nordmann')
    expect(roster.getSpaceRoster).toHaveBeenCalledTimes(2)
  })

  it('answers synchronously from a fresh read, and not once that read is stale', async () => {
    roster.getSpaceRoster.mockResolvedValue([member('user-1', 'Ola Nordmann')])
    const now = 2_000_000
    expect(knownRosterNames('room-sync', now)).toBeUndefined()

    await rosterNames('room-sync', now)

    expect(knownRosterNames('room-sync', now + 1)?.get('user-1')).toBe('Ola Nordmann')
    expect(knownRosterNames('room-sync', now + ROSTER_NAMES_TTL_MS)).toBeUndefined()
  })
})

describe('actorDisplayName', () => {
  const names = new Map([
    ['user-1', 'Ola Nordmann'],
    ['agent-1', 'Research bot'],
  ])

  it('names a person by their subject id and an agent by its agent reference', () => {
    expect(actorDisplayName(person('user-1'), names)).toBe('Ola Nordmann')
    expect(actorDisplayName(agent('agent-1'), names)).toBe('Research bot')
  })

  it('names nobody the roster does not name, nobody without a reference, and nobody while the roster is unknown', () => {
    expect(actorDisplayName(person('user-9'), names)).toBeUndefined()
    expect(actorDisplayName(person(null), names)).toBeUndefined()
    expect(actorDisplayName(agent(null), names)).toBeUndefined()
    expect(actorDisplayName({ kind: 'system', subjectId: null, agentRef: null }, names)).toBeUndefined()
    expect(actorDisplayName(person('user-1'), undefined)).toBeUndefined()
  })
})
