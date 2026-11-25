import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { AttentionActor } from '@/shared/api/spaces-attention-items-client'
import type { SpaceAgent, SpaceRosterMember } from '@/shared/api/spaces-client'

// WS11-S7: the names an attention row shows. The roster and agent reads are replaced; each test
// states what the room answered. Control's roster sends a service (agent) member with display_name
// '' (it joins users only), so an agent's name comes from the room's agent read.

const roster = vi.hoisted(() => ({ getSpaceRoster: vi.fn(), getSpaceAgents: vi.fn() }))
vi.mock('@/shared/api/spaces-client', () => ({
  getSpaceRoster: roster.getSpaceRoster,
  getSpaceAgents: roster.getSpaceAgents,
}))

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

function roomAgent(subjectId: string, name: string | undefined, agentRef?: string): SpaceAgent {
  return {
    subject_id: subjectId,
    role: 'editor',
    revision: 1,
    identity_published: true,
    ...(agentRef ? { agent_ref: agentRef } : {}),
    ...(name === undefined ? {} : { name }),
    delivery_targets: [],
  }
}

const person = (subjectId: string | null): AttentionActor => ({ kind: 'user', subjectId, agentRef: null })
const agent = (agentRef: string | null): AttentionActor => ({ kind: 'agent', subjectId: null, agentRef })

beforeEach(() => {
  roster.getSpaceRoster.mockReset()
  roster.getSpaceAgents.mockReset()
  roster.getSpaceAgents.mockResolvedValue([])
  resetRosterNamesForTests()
})

describe('rosterNames', () => {
  it('names people from the roster, trimmed, and leaves out a blank name', async () => {
    roster.getSpaceRoster.mockResolvedValue([member('user-1', '  Ola Nordmann '), member('user-2', '   ')])

    const names = await rosterNames('room-names')

    expect(names.get('user-1')).toBe('Ola Nordmann')
    expect(names.has('user-2')).toBe(false)
    expect(roster.getSpaceRoster).toHaveBeenCalledWith('room-names')
  })

  it('names an agent from the room agent read, since Control sends a service member with no name', async () => {
    roster.getSpaceRoster.mockResolvedValue([member('user-1', 'Ola Nordmann'), member('svc-1', '', 'service')])
    roster.getSpaceAgents.mockResolvedValue([
      roomAgent('svc-1', ' Research bot ', 'agent-def-1'),
      roomAgent('svc-2', '  '),
      roomAgent('svc-3', undefined),
    ])

    const names = await rosterNames('room-agents')

    expect(names.get('svc-1')).toBe('Research bot')
    expect(names.get('agent-def-1')).toBe('Research bot')
    expect(names.has('svc-2')).toBe(false)
    expect(names.has('svc-3')).toBe(false)
    expect(names.get('user-1')).toBe('Ola Nordmann')
    expect(roster.getSpaceAgents).toHaveBeenCalledWith('room-agents')
  })

  it('keeps the roster name over an agent name for the same subject', async () => {
    roster.getSpaceRoster.mockResolvedValue([member('svc-1', 'Control name', 'service')])
    roster.getSpaceAgents.mockResolvedValue([roomAgent('svc-1', 'Binding name')])

    expect((await rosterNames('room-precedence')).get('svc-1')).toBe('Control name')
  })

  it('still names people when the agent read fails, and leaves agents unnamed', async () => {
    roster.getSpaceRoster.mockResolvedValue([member('user-1', 'Ola Nordmann'), member('svc-1', '', 'service')])
    roster.getSpaceAgents.mockRejectedValue(new Error('agents unavailable'))

    const names = await rosterNames('room-agents-down')

    expect(names.get('user-1')).toBe('Ola Nordmann')
    expect(names.has('svc-1')).toBe(false)
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
  it('names a person by their subject id and an agent by its agent reference, from real read shapes', async () => {
    roster.getSpaceRoster.mockResolvedValue([member('user-1', 'Ola Nordmann'), member('svc-1', '', 'service')])
    roster.getSpaceAgents.mockResolvedValue([roomAgent('svc-1', 'Research bot')])
    const read = await rosterNames('room-actor')

    expect(actorDisplayName(person('user-1'), read)).toBe('Ola Nordmann')
    expect(actorDisplayName(agent('svc-1'), read)).toBe('Research bot')
  })

  const names = new Map([['user-1', 'Ola Nordmann']])

  it('names nobody the roster does not name, nobody without a reference, and nobody while the roster is unknown', () => {
    expect(actorDisplayName(person('user-9'), names)).toBeUndefined()
    expect(actorDisplayName(person(null), names)).toBeUndefined()
    expect(actorDisplayName(agent(null), names)).toBeUndefined()
    expect(actorDisplayName({ kind: 'system', subjectId: null, agentRef: null }, names)).toBeUndefined()
    expect(actorDisplayName(person('user-1'), undefined)).toBeUndefined()
  })
})
