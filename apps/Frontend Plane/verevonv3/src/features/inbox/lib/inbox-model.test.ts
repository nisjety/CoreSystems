import { describe, expect, it } from 'vitest'
import { resolveInboxRouteFilter, searchParamsFromInboxSlug } from './inbox-model'

describe('legacy mentions navigation', () => {
  it('falls back to the actionable personal queue instead of applying a text heuristic', () => {
    expect(resolveInboxRouteFilter(new URLSearchParams('view=mentions'))).toMatchObject({
      activeTab: 'all',
      assigned: 'mine',
      label: 'Your inbox',
    })
    expect(searchParamsFromInboxSlug(['mentions', 'x']).toString()).toBe('view=mine')
  })
})
