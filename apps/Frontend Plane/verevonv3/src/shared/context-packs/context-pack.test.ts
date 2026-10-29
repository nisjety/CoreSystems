import { describe, expect, it } from 'vitest'

import { buildModelContextPack } from './context-pack'

describe('buildModelContextPack', () => {
  it('does not advertise browser-only actions to the Model context', () => {
    const pack = buildModelContextPack({
      route: '/support/inbox',
      visibleItems: [],
    })

    expect(pack.availableActions).toEqual([])
    expect(pack.availableActions).not.toContain('tickets.create')
    expect(pack.currentView).toBe('support.inbox')
  })
})
