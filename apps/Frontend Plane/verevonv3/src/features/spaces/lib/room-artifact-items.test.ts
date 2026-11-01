import { describe, expect, it } from 'vitest'

import type { ChatArtifact } from '@/features/chat/components/chat-types'

import { roomItems } from './room-artifact-items'

const artifact = (over: Partial<ChatArtifact> = {}): ChatArtifact => ({
  id: 'a1',
  kind: 'document',
  title: 'Plan',
  content: 'v1 body',
  version: 1,
  ...over,
})

describe('roomItems (WS07-S11)', () => {
  it('is [] for an empty list and for turns without artifacts', () => {
    expect(roomItems([])).toEqual([])
    expect(roomItems([{ messageId: 'm1' }, { messageId: 'm2', artifacts: [] }])).toEqual([])
  })

  it('merges revisions of one id across turns into one item with its history', () => {
    const items = roomItems([
      { messageId: 'm1', artifacts: [artifact({ version: 1, content: 'one' })] },
      { messageId: 'm2' },
      { messageId: 'm3', artifacts: [artifact({ version: 2, content: 'two', title: 'Plan 2' })] },
    ])
    expect(items).toHaveLength(1)
    const item = items[0]!
    expect(item.artifact.version).toBe(2)
    expect(item.artifact.content).toBe('two')
    expect(item.artifact.title).toBe('Plan 2')
    expect(item.artifact.history?.map((h) => h.version)).toEqual([1, 2])
    // The newest revision's turn carries the item.
    expect(item.turn.id).toBe('m3')
  })

  it('keeps distinct ids as distinct items in first-seen order', () => {
    const items = roomItems([
      { messageId: 'm1', artifacts: [artifact({ id: 'x' }), artifact({ id: 'y', kind: 'code' })] },
    ])
    expect(items.map((i) => i.artifact.id)).toEqual(['x', 'y'])
  })

  it('lets an artifact-only turn (no messageId) contribute', () => {
    const items = roomItems([{ artifacts: [artifact({ id: 'solo' })] }])
    expect(items).toHaveLength(1)
    expect(items[0]?.artifact.id).toBe('solo')
    expect(items[0]?.turn.id).toBe('room-turn-0')
    expect(items[0]?.turn.role).toBe('assistant')
  })

  it('does not mutate its input', () => {
    const turns = [{ messageId: 'm1', artifacts: [artifact()] }]
    const before = JSON.stringify(turns)
    roomItems(turns)
    expect(JSON.stringify(turns)).toBe(before)
  })
})
