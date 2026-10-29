import { describe, expect, it } from 'vitest'
import type { ChatMessageVersion } from '@/shared/api/chat-client'
import {
  activeLeafId,
  canMoveBranches,
  rewindPlan,
  serverVersionBadge,
  type VersionedTurn,
} from './chat-versions-server'

const version = (overrides: Partial<ChatMessageVersion>): ChatMessageVersion => ({
  parentMessageId: 'user-1',
  siblingOrdinal: 1,
  siblingCount: 1,
  onActivePath: true,
  siblings: ['answer-1'],
  ...overrides,
})

/** A thread: a question, its answer (the active one of three versions), and a follow-up. */
const versionedThread = (): VersionedTurn[] => [
  { id: 'user-1', version: version({ parentMessageId: '', siblings: ['user-1'] }) },
  {
    id: 'answer-2',
    version: version({ siblingOrdinal: 2, siblingCount: 3, siblings: ['answer-1', 'answer-2', 'answer-3'] }),
  },
  { id: 'user-2', version: version({ parentMessageId: 'answer-2', siblings: ['user-2'] }) },
  { id: 'answer-4', version: version({ parentMessageId: 'user-2', siblings: ['answer-4'] }) },
]

describe('serverVersionBadge', () => {
  it('reads the place of a turn among its versions and the neighbours to switch to', () => {
    expect(serverVersionBadge(version({ siblingOrdinal: 2, siblingCount: 3, siblings: ['a', 'b', 'c'] }))).toEqual({
      current: 2,
      total: 3,
      previousId: 'a',
      nextId: 'c',
    })
  })

  it('has no previous version at the first and no next at the last', () => {
    const first = serverVersionBadge(version({ siblingOrdinal: 1, siblingCount: 2, siblings: ['a', 'b'] }))
    expect(first).toEqual({ current: 1, total: 2, previousId: null, nextId: 'b' })
    const last = serverVersionBadge(version({ siblingOrdinal: 2, siblingCount: 2, siblings: ['a', 'b'] }))
    expect(last).toEqual({ current: 2, total: 2, previousId: 'a', nextId: null })
  })

  it('shows no switcher for a turn with one version, or for a turn the server did not version', () => {
    expect(serverVersionBadge(version({}))).toBeNull()
    expect(serverVersionBadge(undefined)).toBeNull()
  })
})

describe('activeLeafId and canMoveBranches', () => {
  it('names the newest active turn as the leaf', () => {
    expect(activeLeafId(versionedThread())).toBe('answer-4')
  })

  it('reports no leaf and no branch moves for a transcript without a version tree', () => {
    const plain = [{ id: 'user-1' }, { id: 'answer-1' }]
    expect(canMoveBranches(plain)).toBe(false)
    expect(activeLeafId(plain)).toBe('')
    expect(canMoveBranches(versionedThread())).toBe(true)
  })
})

describe('rewindPlan', () => {
  it('counts the active-path turns a rewind hides and names the leaf the move must find', () => {
    expect(rewindPlan(versionedThread(), 'answer-2')).toEqual({
      hiddenTurns: 2,
      expectedLeafMessageId: 'answer-4',
    })
  })

  it('offers nothing at the leaf, for a turn off the active path, or without a version tree', () => {
    expect(rewindPlan(versionedThread(), 'answer-4')).toBeNull()
    const off = versionedThread()
    off[1] = { id: 'answer-1', version: version({ siblingOrdinal: 1, siblingCount: 3, onActivePath: false }) }
    expect(rewindPlan(off, 'answer-1')).toBeNull()
    expect(rewindPlan([{ id: 'user-1' }, { id: 'answer-1' }], 'user-1')).toBeNull()
    expect(rewindPlan(versionedThread(), 'no-such-turn')).toBeNull()
  })
})
