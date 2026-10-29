/** Humans and future governed tools share the same authorized site capability. */
export const marketingMonitoringContracts = {
  read: { id: 'marketing.monitoring.read', method: 'GET', path: '/api/v1/visibility/monitoring', readOnly: true, allowedActorTypes: ['human'] },
  configure: { id: 'marketing.monitoring.configure', method: 'PATCH', path: '/api/v1/visibility/monitoring', requiresExplicitUserAction: true, allowedActorTypes: ['human'] },
} as const
