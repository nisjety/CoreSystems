/** Capability declarations: execution remains session/site-authorized by the gateway. */
export const marketingAuditContracts = {
  read: { id: 'marketing.audit.read', method: 'GET', path: '/api/v1/visibility/audits', readOnly: true, allowedActorTypes: ['human'] },
  start: { id: 'marketing.audit.start', method: 'POST', path: '/api/v1/visibility/audits', requiresExplicitUserAction: true, allowedActorTypes: ['human'] },
  save: { id: 'marketing.audit.finding.save', method: 'POST', path: '/api/v1/visibility/work/audit', requiresExplicitUserAction: true, allowedActorTypes: ['human'] },
} as const
