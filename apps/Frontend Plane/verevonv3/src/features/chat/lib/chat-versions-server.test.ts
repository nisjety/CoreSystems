import { describe, expect, it } from 'vitest'
import type { ChatMessageVersion } from '@/shared/api/chat-client'
import {
  activeLeafId,
  canMoveBranches,
  rewindPlan,
  serverVersionBadge,
  turnExcerpt,
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
  {
    id: 'user-1',
    role: 'user',
    content: 'Hva koster frakten?',
    version: version({ parentMessageId: '', siblings: ['user-1'] }),
  },
  {
    id: 'answer-2',
    role: 'assistant',
    content: 'Frakten koster 99 kr.',
    version: version({ siblingOrdinal: 2, siblingCount: 3, siblings: ['answer-1', 'answer-2', 'answer-3'] }),
  },
  {
    id: 'user-2',
    role: 'user',
    content: 'Og retur?',
    version: version({ parentMessageId: 'answer-2', siblings: ['user-2'] }),
  },
  {
    id: 'answer-4',
    role: 'assistant',
    content: 'Retur er gratis.',
    version: version({ parentMessageId: 'user-2', siblings: ['answer-4'] }),
  },
]

const plain: VersionedTurn[] = [
  { id: 'user-1', role: 'user', content: 'Hei' },
  { id: 'answer-1', role: 'assistant', content: 'Hei der' },
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
    expect(canMoveBranches(plain)).toBe(false)
    expect(activeLeafId(plain)).toBe('')
    expect(canMoveBranches(versionedThread())).toBe(true)
  })
})

describe('rewindPlan', () => {
  it('lists the active-path turns a rewind hides, oldest first, and names the leaf the move must find', () => {
    expect(rewindPlan(versionedThread(), 'answer-2')).toEqual({
      hiddenTurns: 2,
      hidden: [
        { id: 'user-2', role: 'user', excerpt: 'Og retur?' },
        { id: 'answer-4', role: 'assistant', excerpt: 'Retur er gratis.' },
      ],
      expectedLeafMessageId: 'answer-4',
    })
  })

  it('leaves out a turn the version tree marks as off the active path', () => {
    const thread = versionedThread()
    thread[2] = { ...thread[2]!, version: version({ parentMessageId: 'answer-2', onActivePath: false }) }
    expect(rewindPlan(thread, 'answer-2')?.hidden.map((turn) => turn.id)).toEqual(['answer-4'])
  })

  it('offers nothing at the leaf, for a turn off the active path, or without a version tree', () => {
    expect(rewindPlan(versionedThread(), 'answer-4')).toBeNull()
    const off = versionedThread()
    off[1] = {
      id: 'answer-1',
      role: 'assistant',
      content: 'Frakten koster 99 kr.',
      version: version({ siblingOrdinal: 1, siblingCount: 3, onActivePath: false }),
    }
    expect(rewindPlan(off, 'answer-1')).toBeNull()
    expect(rewindPlan(plain, 'user-1')).toBeNull()
    expect(rewindPlan(versionedThread(), 'no-such-turn')).toBeNull()
  })

  it('keeps a turn with no text in the preview, with an empty excerpt', () => {
    const thread = versionedThread()
    thread[2] = { ...thread[2]!, content: '   ' }
    expect(rewindPlan(thread, 'answer-2')?.hidden[0]).toEqual({ id: 'user-2', role: 'user', excerpt: '' })
  })
})

describe('turnExcerpt', () => {
  it('keeps a short text whole and collapses its whitespace onto one line', () => {
    expect(turnExcerpt('  Hei\n\n  på   deg  ')).toBe('Hei på deg')
  })

  it('cuts a long text short with an ellipsis', () => {
    const excerpt = turnExcerpt('ord '.repeat(60))
    expect(excerpt.endsWith('…')).toBe(true)
    expect(excerpt.length).toBeLessThanOrEqual(121)
  })
})
