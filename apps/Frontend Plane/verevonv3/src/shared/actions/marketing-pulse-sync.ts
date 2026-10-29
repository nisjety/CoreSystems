/** One UI intent composed from the same scoped, administrative backend actions.
 * It never grants provider access or substitutes a browser-supplied org ID. */
export const marketingPulseSyncContract = {
  id: 'marketing.pulse.sync', requiresExplicitUserAction: true, allowedActorTypes: ['human'],
  composes: ['marketing.analytics-sync.collect', 'marketing.preparation.prepare', 'marketing.public-signals.collect', 'marketing.site-sources.collection.queue', 'marketing.site-sources.sync', 'marketing.web-vitals.sync'],
} as const
