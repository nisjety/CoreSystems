import { describe, expect, it } from 'vitest'
import { marketingWebsiteContracts } from './marketing-websites'

describe('marketing website contracts', () => {
  it('retires local DNS commands while preserving provider account access refresh',()=>{
    expect('ownershipChallenge' in marketingWebsiteContracts).toBe(false)
    expect('ownershipVerify' in marketingWebsiteContracts).toBe(false)
    expect(marketingWebsiteContracts.verify).toMatchObject({method:'POST',path:'/api/v1/visibility/websites/:websiteId/verify',allowedActorTypes:['human','model']})
  })
  it('declares GA4 reporting-host mapping as a human-only website revision command', () => {
    expect(marketingWebsiteContracts.mapGA4ReportingSite).toEqual({
      id: 'marketing.websites.ga4.map-reporting-site',
      method: 'PATCH',
      path: '/api/v1/visibility/websites/:websiteId/mappings',
      allowedActorTypes: ['human'],
    })
  })
})
