import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { MentionCandidate } from './mention-model'
import { createMentionState, readRecentMentions } from './mention-state'

const person = (subjectId: string, name: string): MentionCandidate => ({
  id: `user:${subjectId}`,
  kind: 'person',
  subjectId,
  name,
  label: name,
  role: 'editor',
})

const agent = (subjectId: string, name: string): MentionCandidate => ({
  id: `agent:${subjectId}`,
  kind: 'agent',
  subjectId,
  name,
  label: name,
})

/** The storage keys the page holds right now (the setup gives each test a fresh store). */
const storedKeys = (): string[] =>
  Array.from({ length: window.localStorage.length }, (_, index) => window.localStorage.key(index) ?? '')

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('createMentionState pick caps (WS08-S6)', () => {
  it('refuses the 21st person and the 4th agent', () => {
    const state = createMentionState(() => 'room-a', () => 'thread-1')
    for (let index = 0; index < 20; index += 1) {
      expect(state.pick(person(`user-${index}`, `Person ${index}`))).toBe(true)
    }
    expect(state.pick(person('user-20', 'Person 20'))).toBe(false)
    for (let index = 0; index < 3; index += 1) {
      expect(state.pick(agent(`agent-${index}`, `Agent ${index}`))).toBe(true)
    }
    expect(state.pick(agent('agent-3', 'Agent 3'))).toBe(false)
    flush()
    expect(state.people()).toHaveLength(20)
    expect(state.agents()).toHaveLength(3)
  })

  it('does not count a pick that is already there, so it is not refused at the cap', () => {
    const state = createMentionState(() => 'room-a', () => 'thread-1')
    for (let index = 0; index < 20; index += 1) state.pick(person(`user-${index}`, `Person ${index}`))
    for (let index = 0; index < 3; index += 1) state.pick(agent(`agent-${index}`, `Agent ${index}`))
    expect(state.pick(person('user-0', 'Person 0'))).toBe(true)
    expect(state.pick(agent('agent-0', 'Agent 0'))).toBe(true)
    flush()
    expect(state.people()).toHaveLength(20)
    expect(state.agents()).toHaveLength(3)
  })
})

describe('createMentionState reconcile (WS08-S6)', () => {
  it('drops a typed pick whose label left the words, and keeps a locked agent', () => {
    const state = createMentionState(() => 'room-a', () => 'thread-1')
    state.pick(person('user-1', 'Kari'))
    state.pick(agent('agent-1', 'Lager'))
    state.pick(agent('agent-2', 'Innkjop'))
    state.toggleLock('agent-2')

    state.reconcile('Hei @Kari @Lager')
    flush()
    expect(state.people().map((kept) => kept.subjectId)).toEqual(['user-1'])
    expect(state.agents().map((agentPick) => agentPick.subjectId)).toEqual(['agent-1', 'agent-2'])

    state.reconcile('Hei')
    flush()
    expect(state.people()).toEqual([])
    expect(state.agents()).toEqual([{ subjectId: 'agent-2', label: 'Innkjop', source: 'locked' }])
  })
})

describe('createMentionState locks (WS08-S6)', () => {
  it('writes the lock for its own thread only, and a second thread starts without it', () => {
    const state = createMentionState(() => 'room-a', () => 'thread-1')
    state.pick(agent('agent-1', 'Lager'))
    state.toggleLock('agent-1')
    flush()

    const keys = storedKeys()
    expect(keys.some((key) => key.includes('thread-1'))).toBe(true)
    expect(keys.some((key) => key.includes('thread-2'))).toBe(false)
    expect(createMentionState(() => 'room-a', () => 'thread-2').agents()).toEqual([])
    expect(createMentionState(() => 'room-a', () => 'thread-1').agents()).toEqual([
      { subjectId: 'agent-1', label: 'Lager', source: 'locked' },
    ])
  })

  it('rebind loads the locks of the thread the box is aimed at now', () => {
    let thread = 'thread-1'
    const state = createMentionState(() => 'room-a', () => thread)
    state.pick(agent('agent-1', 'Lager'))
    state.toggleLock('agent-1')

    thread = 'thread-2'
    state.rebind('thread-2')
    flush()
    expect(state.agents()).toEqual([])

    thread = 'thread-1'
    state.rebind('thread-1')
    flush()
    expect(state.agents()).toEqual([{ subjectId: 'agent-1', label: 'Lager', source: 'locked' }])
  })

  it('unlocking removes the stored lock', () => {
    const state = createMentionState(() => 'room-a', () => 'thread-1')
    state.pick(agent('agent-1', 'Lager'))
    state.toggleLock('agent-1')
    state.toggleLock('agent-1')
    flush()
    expect(storedKeys()).toEqual([])
    expect(createMentionState(() => 'room-a', () => 'thread-1').agents()).toEqual([])
  })

  it('keeps working when browser storage throws on every read and write', () => {
    const blocked = () => {
      throw new Error('storage blocked')
    }
    vi.stubGlobal('localStorage', {
      get length() {
        return blocked()
      },
      clear: blocked,
      getItem: blocked,
      key: blocked,
      removeItem: blocked,
      setItem: blocked,
    })

    const state = createMentionState(() => 'room-a', () => 'thread-1')
    expect(state.pick(agent('agent-1', 'Lager'))).toBe(true)
    expect(() => state.toggleLock('agent-1')).not.toThrow()
    expect(state.pick(person('user-1', 'Kari'))).toBe(true)
    flush()
    expect(state.agents()).toEqual([{ subjectId: 'agent-1', label: 'Lager', source: 'locked' }])

    const payload = state.payload('Hei @Kari')
    expect(payload).toEqual({
      people: [{ subjectId: 'user-1', label: 'Kari' }],
      agents: [{ subjectId: 'agent-1', source: 'locked' }],
    })
    expect(() => state.sent(payload)).not.toThrow()
    flush()
    expect(state.people()).toEqual([])
    expect(state.agents()).toEqual([{ subjectId: 'agent-1', label: 'Lager', source: 'locked' }])
    expect(readRecentMentions('room-a')).toEqual([])
  })
})

describe('createMentionState sent (WS08-S6)', () => {
  it('spends the typed picks, keeps the locked agent, and records the sent ids as recent, newest first', () => {
    const state = createMentionState(() => 'room-a', () => 'thread-1')
    state.pick(person('user-1', 'Kari'))
    state.pick(agent('agent-1', 'Lager'))
    state.pick(agent('agent-2', 'Innkjop'))
    state.toggleLock('agent-2')

    const first = state.payload('Hei @Kari @Lager @Innkjop')
    expect(first).toEqual({
      people: [{ subjectId: 'user-1', label: 'Kari' }],
      agents: [{ subjectId: 'agent-1' }, { subjectId: 'agent-2', source: 'locked' }],
    })
    state.sent(first)
    flush()
    expect(state.people()).toEqual([])
    expect(state.agents()).toEqual([{ subjectId: 'agent-2', label: 'Innkjop', source: 'locked' }])
    expect(readRecentMentions('room-a')).toEqual(['agent-1', 'agent-2', 'user-1'])

    state.pick(person('user-2', 'Ola'))
    state.sent(state.payload('Hei @Ola'))
    expect(readRecentMentions('room-a')).toEqual(['agent-2', 'user-2', 'agent-1', 'user-1'])

    state.sent(undefined)
    expect(readRecentMentions('room-a')).toEqual(['agent-2', 'user-2', 'agent-1', 'user-1'])
  })
})
