import { describe, expect, it } from 'vitest'

import type { SpaceAgent, SpaceRosterMember, SpaceThread } from '@/shared/api/spaces-client'
import {
  agentActivity,
  agentCardModel,
  LAST_ENTRY_MAX_CHARS,
  memberCardModel,
  memberLastPostAt,
  type ActivityTurn,
} from './agent-activity'

const agent = (overrides: Partial<SpaceAgent> = {}): SpaceAgent => ({
  subject_id: 'agent-1',
  role: 'editor',
  revision: 1,
  identity_published: true,
  name: 'Lagerhjelpen',
  status: 'active',
  delivery_targets: [],
  ...overrides,
}) as SpaceAgent

const thread = (overrides: Partial<SpaceThread> = {}): SpaceThread => ({
  thread_id: 't1',
  space_id: 'room-1',
  title: 'Innkjøp',
  ...overrides,
})

const reply = (content: string, overrides: Partial<ActivityTurn> = {}): ActivityTurn => ({
  threadId: 't1',
  role: 'assistant',
  agentName: 'Lagerhjelpen',
  content,
  ...overrides,
})

const lagerhjelpen = agent()
const innkjop = agent({ subject_id: 'agent-2', name: 'Innkjøpsagent' })

describe('agentActivity lead-line precedence (WS07-S8)', () => {
  it('leads with blocking when a run is paused for a person, even if another run is working and a reply exists', () => {
    const lead = agentActivity({
      agent: lagerhjelpen,
      agents: [lagerhjelpen],
      threads: [
        thread({ thread_id: 'working', latest_run_status: 'running' }),
        thread({ thread_id: 'blocked', latest_run_status: 'awaiting_approval', owner_subject_id: 'user-9' }),
      ],
      turns: [reply('Ferdig.')],
    })
    expect(lead).toEqual({ kind: 'blocking', ownerSubjectId: 'user-9' })
  })

  it('leads with working (queued or running) when nothing is blocking, ahead of the last reply', () => {
    for (const status of ['queued', 'running']) {
      const lead = agentActivity({
        agent: lagerhjelpen,
        agents: [lagerhjelpen],
        threads: [thread({ latest_run_status: status, title: ' Innkjøp ' })],
        turns: [reply('Ferdig.')],
      })
      expect(lead, status).toEqual({ kind: 'working', threadTitle: 'Innkjøp' })
    }
  })

  it('falls back to the last reply with the thread time when nothing is blocking or working', () => {
    const lead = agentActivity({
      agent: lagerhjelpen,
      agents: [lagerhjelpen],
      threads: [thread({ latest_run_status: 'completed', updated_at: '2026-10-01T10:00:00Z' })],
      turns: [reply('Alt er klart.')],
    })
    expect(lead).toEqual({ kind: 'last', text: 'Alt er klart.', at: '2026-10-01T10:00:00Z' })
  })

  it('has no lead line when there is nothing to say', () => {
    expect(agentActivity({ agent: lagerhjelpen, agents: [lagerhjelpen], threads: [], turns: [] })).toEqual({ kind: 'none' })
  })

  it('a completed or failed run is neither blocking nor working', () => {
    for (const status of ['completed', 'failed', 'cancelled']) {
      const lead = agentActivity({
        agent: lagerhjelpen,
        agents: [lagerhjelpen],
        threads: [thread({ latest_run_status: status })],
        turns: [],
      })
      expect(lead.kind, status).toBe('none')
    }
  })

  it('omits the owner and the title when they are blank', () => {
    const blocking = agentActivity({
      agent: lagerhjelpen,
      agents: [lagerhjelpen],
      threads: [thread({ latest_run_status: 'awaiting_approval', owner_subject_id: '  ' })],
      turns: [],
    })
    expect(blocking).toEqual({ kind: 'blocking' })
    const working = agentActivity({
      agent: lagerhjelpen,
      agents: [lagerhjelpen],
      threads: [thread({ latest_run_status: 'running', title: ' ' })],
      turns: [],
    })
    expect(working).toEqual({ kind: 'working' })
  })
})

describe('agentActivity attribution (WS07-S8)', () => {
  it('attributes a run to the sole active agent without any turn naming it', () => {
    const lead = agentActivity({
      agent: lagerhjelpen,
      agents: [lagerhjelpen, agent({ subject_id: 'agent-3', name: 'Pauset', status: 'paused' })],
      threads: [thread({ latest_run_status: 'running' })],
      turns: [],
    })
    expect(lead.kind).toBe('working')
  })

  it('with two active agents a run is attributed only through the thread last assistant turn naming it', () => {
    const roomAgents = [lagerhjelpen, innkjop]
    const threads = [thread({ latest_run_status: 'running' })]

    expect(
      agentActivity({ agent: lagerhjelpen, agents: roomAgents, threads, turns: [reply('Hei', { agentName: 'Lagerhjelpen' })] }).kind,
    ).toBe('working')
    expect(
      agentActivity({ agent: innkjop, agents: roomAgents, threads, turns: [reply('Hei', { agentName: 'Lagerhjelpen' })] }).kind,
    ).toBe('none')
  })

  it('uses the LAST assistant turn of the thread, not an earlier one (no working line for the earlier agent)', () => {
    const lead = agentActivity({
      agent: lagerhjelpen,
      agents: [lagerhjelpen, innkjop],
      threads: [thread({ latest_run_status: 'running' })],
      turns: [reply('Først', { agentName: 'Lagerhjelpen' }), reply('Så', { agentName: 'Innkjøpsagent' })],
    })
    expect(lead.kind).not.toBe('working')
  })

  it('ignores user turns and turns of other threads when attributing', () => {
    const lead = agentActivity({
      agent: lagerhjelpen,
      agents: [lagerhjelpen, innkjop],
      threads: [thread({ latest_run_status: 'running' })],
      turns: [
        reply('Annen tråd', { threadId: 'other', agentName: 'Lagerhjelpen' }),
        { threadId: 't1', role: 'user', content: 'Spørsmål', agentName: 'Lagerhjelpen' },
      ],
    })
    expect(lead.kind).not.toBe('working')
  })

  it('an undefined agent (ambiguous or unknown) gives no lead line, whatever is running', () => {
    const lead = agentActivity({
      agent: undefined,
      agents: [lagerhjelpen],
      threads: [thread({ latest_run_status: 'awaiting_approval' })],
      turns: [reply('Hei')],
    })
    expect(lead).toEqual({ kind: 'none' })
  })

  it('a paused sole agent is not "the sole active agent"', () => {
    const paused = agent({ status: 'paused' })
    const lead = agentActivity({
      agent: paused,
      agents: [paused],
      threads: [thread({ latest_run_status: 'running' })],
      turns: [],
    })
    expect(lead.kind).toBe('none')
  })

  it('the last-entry fallback only reads turns that name this agent', () => {
    const lead = agentActivity({
      agent: lagerhjelpen,
      agents: [lagerhjelpen, innkjop],
      threads: [thread({ latest_run_status: 'completed' })],
      turns: [reply('Min', { agentName: 'Lagerhjelpen' }), reply('Deres', { agentName: 'Innkjøpsagent' })],
    })
    expect(lead).toMatchObject({ kind: 'last', text: 'Min' })
  })
})

describe('agentActivity last entry text (WS07-S8)', () => {
  const lastOf = (content: string) =>
    agentActivity({ agent: lagerhjelpen, agents: [lagerhjelpen], threads: [], turns: [reply(content)] })

  it('caps the entry at 120 characters', () => {
    expect(LAST_ENTRY_MAX_CHARS).toBe(120)
    const lead = lastOf('x'.repeat(500))
    expect(lead.kind).toBe('last')
    expect((lead as { text: string }).text).toHaveLength(120)
  })

  it('shows plain text: markdown decoration is stripped before the cap', () => {
    const lead = lastOf('## Status\n\n**Alt** er `grønt`\n- punkt')
    expect(lead).toEqual({ kind: 'last', text: 'Status Alt er grønt punkt' })
  })

  it('collapses whitespace and drops a code block entirely', () => {
    const lead = lastOf('Før\n\n```sh\nrm -rf /\n```\n\n  Etter   ')
    expect((lead as { text: string }).text).toBe('Før Etter')
  })

  it('skips an empty latest reply and reads the previous one', () => {
    const lead = agentActivity({
      agent: lagerhjelpen,
      agents: [lagerhjelpen],
      threads: [],
      turns: [reply('Forrige'), reply('   ')],
    })
    expect(lead).toMatchObject({ kind: 'last', text: 'Forrige' })
  })

  it('takes the time from the thread the reply belongs to, preferring updated_at', () => {
    const lead = agentActivity({
      agent: lagerhjelpen,
      agents: [lagerhjelpen],
      threads: [
        thread({ thread_id: 'other', updated_at: '2026-01-01T00:00:00Z' }),
        thread({ thread_id: 't1', updated_at: '2026-10-02T10:00:00Z', latest_run_updated_at: '2026-10-03T10:00:00Z' }),
      ],
      turns: [reply('Hei')],
    })
    expect(lead).toMatchObject({ at: '2026-10-02T10:00:00Z' })
  })

  it('has no time when the thread is not loaded', () => {
    const lead = lastOf('Hei')
    expect(lead).toEqual({ kind: 'last', text: 'Hei' })
  })
})

describe('memberLastPostAt (WS07-S8)', () => {
  it('is the latest time among the posts the member owns', () => {
    const threads = [
      thread({ thread_id: 'a', owner_subject_id: 'user-1', updated_at: '2026-10-01T08:00:00Z' }),
      thread({ thread_id: 'b', owner_subject_id: 'user-1', latest_run_updated_at: '2026-10-05T08:00:00Z' }),
      thread({ thread_id: 'c', owner_subject_id: 'user-2', updated_at: '2026-11-01T08:00:00Z' }),
    ]
    expect(memberLastPostAt('user-1', threads)).toBe('2026-10-05T08:00:00Z')
  })

  it('is undefined when the member owns nothing, the id is blank or missing, or a post has no time', () => {
    const threads = [thread({ owner_subject_id: 'user-2', updated_at: '2026-10-01T08:00:00Z' }), thread({ thread_id: 'x', owner_subject_id: 'user-1' })]
    expect(memberLastPostAt('user-1', threads)).toBeUndefined()
    expect(memberLastPostAt('user-3', threads)).toBeUndefined()
    expect(memberLastPostAt('  ', threads)).toBeUndefined()
    expect(memberLastPostAt(undefined, threads)).toBeUndefined()
    expect(memberLastPostAt('user-1', [])).toBeUndefined()
  })
})

describe('card models (WS07-S8)', () => {
  const roster = [
    { subject_type: 'user', subject_id: 'user-9', role: 'owner', revision: 1, display_name: ' Kari Nordmann ' },
    { subject_type: 'user', subject_id: 'user-1', role: 'editor', revision: 1, display_name: 'Ola' },
  ] as unknown as SpaceRosterMember[]

  it('an agent card in the blocking state names the owner it waits for, and carries no approval detail', () => {
    const model = agentCardModel({
      name: 'Lagerhjelpen',
      agent: agent({ title: ' Lager ' }),
      agents: [lagerhjelpen],
      threads: [thread({ latest_run_status: 'awaiting_approval', owner_subject_id: 'user-9', latest_run_id: 'run-secret' })],
      turns: [],
      roster,
      color: '#2563eb',
    })
    expect(model).toEqual({
      kind: 'agent',
      name: 'Lagerhjelpen',
      color: '#2563eb',
      title: 'Lager',
      lead: { kind: 'blocking', ownerSubjectId: 'user-9' },
      ownerName: 'Kari Nordmann',
    })
    // Nothing about what the run wants to do, or which run it is, reaches the card.
    expect(JSON.stringify(model)).not.toContain('run-secret')
  })

  it('leaves the owner name out when the roster cannot resolve it, and omits unset color and title', () => {
    const model = agentCardModel({
      name: 'Lagerhjelpen',
      agent: lagerhjelpen,
      agents: [lagerhjelpen],
      threads: [thread({ latest_run_status: 'awaiting_approval', owner_subject_id: 'user-404' })],
      turns: [],
      roster,
    })
    expect(model).toEqual({ kind: 'agent', name: 'Lagerhjelpen', lead: { kind: 'blocking', ownerSubjectId: 'user-404' } })
  })

  it('a member card has the role and last post time, resolved by subject id', () => {
    const model = memberCardModel({
      name: 'Ola',
      subjectId: 'user-1',
      roster,
      threads: [thread({ owner_subject_id: 'user-1', updated_at: '2026-10-02T10:00:00Z' })],
    })
    expect(model).toEqual({ kind: 'member', name: 'Ola', role: 'editor', lastPostAt: '2026-10-02T10:00:00Z' })
  })

  it('a member card has neither role nor time when the subject is unknown', () => {
    expect(memberCardModel({ name: 'Ukjent', subjectId: undefined, roster, threads: [] })).toEqual({ kind: 'member', name: 'Ukjent' })
  })
})
