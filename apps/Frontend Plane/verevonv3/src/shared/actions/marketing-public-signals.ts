/** Public-evidence collection remains explicit and organization/site scoped. */
export const marketingPublicSignalsContracts = {
  read: { id: 'marketing.public-signals.read', method: 'GET', path: '/api/v1/visibility/public-signals', readOnly: true, allowedActorTypes: ['human'], cruxOriginPolicy: 'selected_then_exact_www_variant', archiveHostPolicy: 'selected_then_exact_www_variant' },
  collect: { id: 'marketing.public-signals.collect', method: 'POST', path: '/api/v1/visibility/public-signals', requiresExplicitUserAction: true, allowedActorTypes: ['human'] },
  configure: { id: 'marketing.public-signals.configure', method: 'PATCH', path: '/api/v1/visibility/public-signals', requiresExplicitUserAction: true, allowedActorTypes: ['human'] },
} as const
