/** Reads never collect. Importing first-party samples requires a deliberate administrator action. */
export const marketingWebVitalsContracts = {
  read: { id: 'marketing.web-vitals.read', method: 'GET', path: '/api/v1/visibility/web-vitals', readOnly: true, allowedActorTypes: ['human'] },
  sync: { id: 'marketing.web-vitals.sync', method: 'POST', path: '/api/v1/visibility/web-vitals/sync', requiresExplicitUserAction: true, allowedActorTypes: ['human'] },
} as const
