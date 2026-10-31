/** Provider secrets stay in Integration Core; reading does not contact providers. */
export const marketingSiteSourcesContracts = {
  read: { id: 'marketing.site-sources.read', method: 'GET', path: '/api/v1/visibility/site-sources', readOnly: true, allowedActorTypes: ['human'] },
  sync: { id: 'marketing.site-sources.sync', method: 'POST', path: '/api/v1/visibility/site-sources/sync', requiresExplicitUserAction: true, allowedActorTypes: ['human'] },
  collection: { id: 'marketing.site-sources.collection.read', method: 'GET', path: '/api/v1/visibility/site-sources/collection', readOnly: true, allowedActorTypes: ['human'] },
  queue: { id: 'marketing.site-sources.collection.queue', method: 'POST', path: '/api/v1/visibility/site-sources/collection', requiresExplicitUserAction: true, allowedActorTypes: ['human'] },
} as const
