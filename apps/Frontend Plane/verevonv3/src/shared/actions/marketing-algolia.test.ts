import { describe, expect, it } from 'vitest'
import { marketingAlgoliaContracts as contracts } from './marketing-algolia'

describe('Algolia action boundaries', () => {
  it('keeps metadata reads passive and consent/mapping commands explicitly human initiated', () => {
    for (const operation of Object.values(contracts)) {
      expect(operation.allowedActorTypes).toEqual(['human'])
      if (operation.method === 'GET') expect(operation).toHaveProperty('readOnly', true)
      else expect(operation).toHaveProperty('requiresExplicitUserAction', true)
    }
    expect(contracts.save.method).toBe('PUT')
    expect(contracts.disconnectSite.method).toBe('DELETE')
    expect(contracts.connect.path).toBe('/api/v1/integrations/algolia/connect-session')
  })
})
