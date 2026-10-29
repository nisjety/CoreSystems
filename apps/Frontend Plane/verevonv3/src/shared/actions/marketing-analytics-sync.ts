/** Human-triggered collection and scheduling; no provider grants or MCP registration. */
export const marketingAnalyticsSyncContracts = {
  read: { id: 'marketing.analytics-sync.read', method: 'GET', path: '/api/v1/visibility/analytics-sync', readOnly: true, allowedActorTypes: ['human'] },
  collect: { id: 'marketing.analytics-sync.collect', method: 'POST', path: '/api/v1/visibility/analytics-sync', requiresExplicitUserAction: true, allowedActorTypes: ['human'] },
  configure: { id: 'marketing.analytics-sync.configure', method: 'PATCH', path: '/api/v1/visibility/analytics-sync', requiresExplicitUserAction: true, allowedActorTypes: ['human'] },
} as const
