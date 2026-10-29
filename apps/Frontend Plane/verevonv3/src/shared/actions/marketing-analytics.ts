/** Read-only owner API contracts for authenticated human and machine clients.
 * Provider tokens, grant mutations and arbitrary report bodies are not inputs. */
export const marketingAnalyticsContracts = {
  ga4: { id: 'marketing.ga4.read', method: 'GET', path: '/api/v1/visibility/ga4', query: ['websiteId', 'site', 'quarter'], allowedActorTypes: ['human', 'model'] },
  clarity: { id: 'marketing.clarity.read', method: 'GET', path: '/api/v1/visibility/clarity', query: ['websiteId', 'site'], allowedActorTypes: ['human', 'model'], numOfDays: 3, scope: 'clarity_project', includes: ['traffic', 'aggregate_ux'], replayAuthority: 'microsoft_clarity' },
} as const
