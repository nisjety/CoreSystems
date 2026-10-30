/** Shared website context for human controls and authenticated machine clients.
 * These owner HTTP contracts are not registered in the generic action executor:
 * provider writes and OAuth consent are never model-executable operations. */
export const marketingWebsiteContracts = {
  list: { id: 'marketing.websites.list', method: 'GET', path: '/api/v1/visibility/websites', allowedActorTypes: ['human', 'model'] },
  get: { id: 'marketing.websites.get', method: 'GET', path: '/api/v1/visibility/websites/:websiteId', allowedActorTypes: ['human', 'model'] },
  add: { id: 'marketing.websites.add', method: 'POST', path: '/api/v1/visibility/websites', allowedActorTypes: ['human', 'model'] },
  update: { id: 'marketing.websites.update', method: 'PATCH', path: '/api/v1/visibility/websites/:websiteId', allowedActorTypes: ['human', 'model'] },
  verify: { id: 'marketing.websites.verify', method: 'POST', path: '/api/v1/visibility/websites/:websiteId/verify', allowedActorTypes: ['human', 'model'] },
  googleConnect: { id: 'marketing.websites.google.connect', method: 'POST', path: '/api/v1/visibility/websites/:websiteId/google/connect', allowedActorTypes: ['human'] },
  mapGA4ReportingSite: { id: 'marketing.websites.ga4.map-reporting-site', method: 'PATCH', path: '/api/v1/visibility/websites/:websiteId/mappings', allowedActorTypes: ['human'] },
} as const
