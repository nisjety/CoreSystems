import { describe, expect, it } from 'vitest'
import { marketingWorkContracts } from './marketing-work'

describe('marketing work contracts', () => {
  it('exposes the same read endpoint to human and model clients', () => {
    expect(marketingWorkContracts.list).toMatchObject({ method: 'GET', allowedActorTypes: ['human', 'model'], readOnly: true })
  })
  it('keeps mutations explicitly human-approved and confined to Marketing Core', () => {
    expect(marketingWorkContracts.saveOpportunity).toMatchObject({ method: 'POST', requiresExplicitUserAction: true, allowedActorTypes: ['human'] })
    expect(marketingWorkContracts.updateStatus).toMatchObject({ method: 'PATCH', requiresExplicitUserAction: true, allowedActorTypes: ['human'] })
    expect(JSON.stringify(marketingWorkContracts)).not.toMatch(/google|meta|ads/i)
  })
})
