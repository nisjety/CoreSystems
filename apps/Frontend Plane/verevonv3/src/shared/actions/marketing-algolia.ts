/** Human account consent and site mapping are distinct from passive report reads. */
export const marketingAlgoliaContracts = {
  connections: { id: 'marketing.algolia.connections', method: 'GET', path: '/api/v1/integrations/algolia/connections', readOnly: true, allowedActorTypes: ['human'] },
  connect: { id: 'marketing.algolia.connect', method: 'POST', path: '/api/v1/integrations/algolia/connect-session', requiresExplicitUserAction: true, allowedActorTypes: ['human'] },
  session: { id: 'marketing.algolia.session', method: 'GET', path: '/api/v1/integrations/algolia/connect-sessions/:id/status', readOnly: true, allowedActorTypes: ['human'] },
  applications: { id: 'marketing.algolia.applications', method: 'GET', path: '/api/v1/integrations/algolia/connections/:id/applications', readOnly: true, allowedActorTypes: ['human'] },
  indices: { id: 'marketing.algolia.indices', method: 'GET', path: '/api/v1/integrations/algolia/connections/:id/indices', readOnly: true, allowedActorTypes: ['human'] },
  mapping: { id: 'marketing.algolia.mapping', method: 'GET', path: '/api/v1/visibility/site-sources/algolia-mapping', readOnly: true, allowedActorTypes: ['human'] },
  save: { id: 'marketing.algolia.mapping.save', method: 'PUT', path: '/api/v1/visibility/site-sources/algolia-mapping', requiresExplicitUserAction: true, allowedActorTypes: ['human'] },
  disconnectSite: { id: 'marketing.algolia.mapping.disconnect-site', method: 'DELETE', path: '/api/v1/visibility/site-sources/algolia-mapping', requiresExplicitUserAction: true, allowedActorTypes: ['human'] },
} as const
