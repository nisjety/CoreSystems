import { createRoot, flush } from 'solid-js'
import { describe, expect, it } from 'vitest'

import { createRoomLiveArtifacts } from './room-live-artifacts'

const art = (id: string, version: number, content = 'c') => ({ id, kind: 'document', title: 't', content, version })

describe('createRoomLiveArtifacts (WS07-S11/S12)', () => {
  it('holds artifacts by thread and replaces the same id+version', () => {
    // Writes happen outside the owner scope, as they do from a stream callback.
    const store = createRoot((dispose) => {
      void dispose
      return createRoomLiveArtifacts()
    })
    expect(store.forThread('t1')).toEqual([])
    store.note(art('a', 1, 'first'), 't1')
    flush()
    store.note(art('a', 2), 't1')
    flush()
    store.note(art('a', 1, 'again'), 't1')
    flush()
    store.note(art('b', 1), 't2')
    flush()
    expect(store.forThread('t1').map((a) => [a.id, a.version, a.content])).toEqual([
      ['a', 2, 'c'],
      ['a', 1, 'again'],
    ])
    expect(store.forThread('t2')).toHaveLength(1)
    expect([...store.all().keys()].sort()).toEqual(['t1', 't2'])
  })
})
