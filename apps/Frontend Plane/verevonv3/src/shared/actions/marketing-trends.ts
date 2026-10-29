/** Explicit public keyword research; the unofficial fallback is dev-only.
 * User-entered terms are sent to Google, never combined with private accounts.
 */
export const marketingTrendsContract = {
  id: 'marketing.trends.research', method: 'POST', path: '/api/v1/visibility/trends',
  readOnly: true, requiresExplicitUserAction: true, allowedActorTypes: ['human'],
} as const

/** Reads a precomputed site report; never starts provider collection. */
export const marketingAutomaticTrendsContract = {
  id: 'marketing.trends.automatic.read', method: 'GET', path: '/api/v1/visibility/trends/automatic',
  readOnly: true, allowedActorTypes: ['human'],
} as const
