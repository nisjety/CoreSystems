/** Explicit human preparation; not an MCP/agent registration or a provider grant. */
export const marketingPreparationContracts = {
  read: { id: 'marketing.preparation.read', method: 'GET', path: '/api/v1/visibility/preparation', readOnly: true, allowedActorTypes: ['human'] },
  prepare: { id: 'marketing.preparation.prepare', method: 'POST', path: '/api/v1/visibility/preparation', requiresExplicitUserAction: true, allowedActorTypes: ['human'] },
  sources: { id: 'marketing.sources.read', method: 'GET', path: '/api/v1/visibility/sources', readOnly: true, allowedActorTypes: ['human'] },
} as const
