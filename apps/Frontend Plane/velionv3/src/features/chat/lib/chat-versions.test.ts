import { describe, expect, it } from 'vitest'
import type { ChatTurn } from '../components/chat-types'
import {
  beginNewVersion,
  lastUserIndex,
  selectVersion,
  versionBadge,
  type ExchangeVersionState,
} from './chat-versions'

const turn = (over: Partial<ChatTurn>): ChatTurn => ({
  id: 'id',
  role: 'user',
  content: '',
  createdAt: '',
  streaming: false,
  tools: [],
  attachments: [],
  ...over,
})

const u = (id: string, content = `q-${id}`) => turn({ id, content })
const a = (id: string, content = `ans-${id}`) => turn({ id, role: 'assistant', content })

const THREAD = 'thread_1'

describe('lastUserIndex', () => {
  it('finds the last user turn', () => {
    expect(lastUserIndex([u('u1'), a('a1'), u('u2'), a('a2')])).toBe(2)
  })
  it('is -1 with no user turn', () => {
    expect(lastUserIndex([a('a1')])).toBe(-1)
    expect(lastUserIndex([])).toBe(-1)
  })
})

describe('beginNewVersion', () => {
  it('creates the first sibling from the outgoing exchange, new version displayed', () => {
    const turns = [u('u1'), a('a1'), u('u2'), a('a2')]
    const state = beginNewVersion(null, turns, THREAD)
    expect(state).toMatchObject({ threadId: THREAD, anchorIndex: 2, activeIndex: 1 })
    expect(state?.siblings).toHaveLength(1)
    expect(state?.siblings[0]?.map((t) => t.id)).toEqual(['u2', 'a2'])
  })

  it('appends on repeat regeneration of the same exchange', () => {
    const turns1 = [u('u1'), a('a1')]
    const s1 = beginNewVersion(null, turns1, THREAD)
    // regenerated answer landed:
    const turns2 = [u('u1'), a('a1b')]
    const s2 = beginNewVersion(s1, turns2, THREAD)
    expect(s2?.siblings.map((tail) => tail.map((t) => t.id))).toEqual([['u1', 'a1'], ['u1', 'a1b']])
    expect(s2?.activeIndex).toBe(2)
    expect(versionBadge(s2, [u('u1'), a('a1c')], THREAD)).toEqual({ current: 3, total: 3 })
  })

  it('regenerating while VIEWING an old sibling shelves it in place and appends the new one at the end', () => {
    // versions: [v0, v1(live)] then switch to v0, then regenerate v0.
    const s1 = beginNewVersion(null, [u('u1'), a('a1')], THREAD) as ExchangeVersionState
    const sel = selectVersion(s1, [u('u1'), a('a1b')], THREAD, 0)
    expect(sel?.turns.map((t) => t.id)).toEqual(['u1', 'a1'])
    const s2 = beginNewVersion(sel!.state, sel!.turns, THREAD)
    // logical order preserved: v0 back at 0, v1 at 1, new live at 2
    expect(s2?.siblings.map((tail) => tail[1]?.id)).toEqual(['a1', 'a1b'])
    expect(s2?.activeIndex).toBe(2)
  })

  it('starts over when the exchange anchor moved (a newer exchange exists)', () => {
    const s1 = beginNewVersion(null, [u('u1'), a('a1')], THREAD)
    const longer = [u('u1'), a('a1x'), u('u2'), a('a2')]
    const s2 = beginNewVersion(s1, longer, THREAD)
    expect(s2).toMatchObject({ anchorIndex: 2, activeIndex: 1 })
    expect(s2?.siblings).toHaveLength(1)
    expect(s2?.siblings[0]?.map((t) => t.id)).toEqual(['u2', 'a2'])
  })

  it('starts over on a different thread', () => {
    const s1 = beginNewVersion(null, [u('u1'), a('a1')], THREAD)
    const s2 = beginNewVersion(s1, [u('u1'), a('a1')], 'thread_2')
    expect(s2?.threadId).toBe('thread_2')
    expect(s2?.siblings).toHaveLength(1)
  })

  it('refuses to version a tail with no assistant answer', () => {
    const prev = beginNewVersion(null, [u('u1'), a('a1')], THREAD)
    expect(beginNewVersion(prev, [u('u1'), a('a1x'), u('u2')], THREAD)).toBe(prev)
    expect(beginNewVersion(null, [u('u1')], THREAD)).toBeNull()
  })

  it('snapshots by copy — later mutation of the source turns does not reach the sibling', () => {
    const src = [u('u1'), a('a1')]
    const state = beginNewVersion(null, src, THREAD)
    src[1] = a('a1', 'MUTATED')
    expect(state?.siblings[0]?.[1]?.content).toBe('ans-a1')
  })
})

describe('selectVersion', () => {
  const setup = () => {
    // two prior siblings + live: v0=[u1,a1], v1=[u1,a1b], live=[u1,a1c]
    let state = beginNewVersion(null, [u('u1'), a('a1')], THREAD) as ExchangeVersionState
    state = beginNewVersion(state, [u('u1'), a('a1b')], THREAD) as ExchangeVersionState
    return { state, turns: [u('u1'), a('a1c')] }
  }

  it('swaps the displayed tail and shelves the current one', () => {
    const { state, turns } = setup()
    const sel = selectVersion(state, turns, THREAD, 0)
    expect(sel?.turns.map((t) => t.id)).toEqual(['u1', 'a1'])
    expect(sel?.state.activeIndex).toBe(0)
    // the previously-live a1c is shelved, order intact: [a1b, a1c]
    expect(sel?.state.siblings.map((tail) => tail[1]?.id)).toEqual(['a1b', 'a1c'])
    expect(versionBadge(sel!.state, sel!.turns, THREAD)).toEqual({ current: 1, total: 3 })
  })

  it('round-trips: switching away and back restores the exact tail', () => {
    const { state, turns } = setup()
    const back = selectVersion(state, turns, THREAD, 0)
    const forth = selectVersion(back!.state, back!.turns, THREAD, 2)
    expect(forth?.turns.map((t) => t.id)).toEqual(['u1', 'a1c'])
    expect(forth?.state.activeIndex).toBe(2)
    expect(versionBadge(forth!.state, forth!.turns, THREAD)).toEqual({ current: 3, total: 3 })
  })

  it('preserves the shared prefix before the anchor', () => {
    const state = beginNewVersion(null, [u('u1'), a('a1'), u('u2'), a('a2')], THREAD) as ExchangeVersionState
    const turns = [u('u1'), a('a1'), u('u2'), a('a2b')]
    const sel = selectVersion(state, turns, THREAD, 0)
    expect(sel?.turns.map((t) => t.id)).toEqual(['u1', 'a1', 'u2', 'a2'])
  })

  it('rejects out-of-range, already-active, and stale requests', () => {
    const { state, turns } = setup()
    expect(selectVersion(state, turns, THREAD, 5)).toBeNull()
    expect(selectVersion(state, turns, THREAD, -1)).toBeNull()
    expect(selectVersion(state, turns, THREAD, state.activeIndex)).toBeNull()
    expect(selectVersion(state, turns, 'other-thread', 0)).toBeNull()
    expect(selectVersion(null, turns, THREAD, 0)).toBeNull()
    // conversation moved on: a newer exchange shifted the anchor
    const moved = [...turns, u('u2'), a('a2')]
    expect(selectVersion(state, moved, THREAD, 0)).toBeNull()
    // conversation truncated to before the anchor
    expect(selectVersion(state, [], THREAD, 0)).toBeNull()
  })
})

describe('versionBadge', () => {
  it('hides until a sibling exists', () => {
    expect(versionBadge(null, [u('u1'), a('a1')], THREAD)).toBeNull()
  })

  it('shows n/N on the versioned exchange', () => {
    const state = beginNewVersion(null, [u('u1'), a('a1')], THREAD)
    expect(versionBadge(state, [u('u1'), a('a1b')], THREAD)).toEqual({ current: 2, total: 2 })
  })

  it('hides when the conversation moves past the versioned exchange', () => {
    const state = beginNewVersion(null, [u('u1'), a('a1')], THREAD)
    const moved = [u('u1'), a('a1b'), u('u2'), a('a2')]
    expect(versionBadge(state, moved, THREAD)).toBeNull()
  })

  it('hides on a different thread and after truncation', () => {
    const state = beginNewVersion(null, [u('u1'), a('a1')], THREAD)
    expect(versionBadge(state, [u('u1'), a('a1b')], 'thread_2')).toBeNull()
    expect(versionBadge(state, [], THREAD)).toBeNull()
  })
})
