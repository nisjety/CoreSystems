/** Capability metadata only; no agent tool is registered by this declaration. */
export const marketingDiscoveryContracts = {
  read: { id: 'marketing.discovery.read', method: 'GET', path: '/api/v1/visibility/discovery', query: ['websiteId', 'site'], allowedActorTypes: ['human'], readOnly: true },
  start: { id: 'marketing.discovery.start', method: 'POST', path: '/api/v1/visibility/discovery', allowedActorTypes: ['human'], requiresExplicitUserAction: true },
} as const
