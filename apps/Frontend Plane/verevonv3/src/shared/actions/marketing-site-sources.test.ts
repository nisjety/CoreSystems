import { describe, expect, it } from 'vitest'
import { marketingSiteSourcesContracts } from './marketing-site-sources'
describe('site-source action contracts', () => {
  it('keeps reads passive and imports explicit human commands', () => {
    expect(marketingSiteSourcesContracts.read).toMatchObject({ method: 'GET', readOnly: true, allowedActorTypes: ['human'] })
    expect(marketingSiteSourcesContracts.sync).toMatchObject({ method: 'POST', path: '/api/v1/visibility/site-sources/sync', requiresExplicitUserAction: true, allowedActorTypes: ['human'] })
    expect(marketingSiteSourcesContracts.collection).toMatchObject({method:'GET',path:'/api/v1/visibility/site-sources/collection',readOnly:true,allowedActorTypes:['human']})
    expect(marketingSiteSourcesContracts.queue).toMatchObject({method:'POST',path:'/api/v1/visibility/site-sources/collection',requiresExplicitUserAction:true,allowedActorTypes:['human']})
  })
})
