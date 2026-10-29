/** Explicit manual refresh contract; the site scheduler refreshes daily.
 * Not registered as an AI-executable action. Visibility owns durable reports;
 * Integration Core owns bounded provider execution. */
export type PageSpeedStrategy = 'mobile' | 'desktop'
export const pageSpeedCheckIds = ['document-title', 'meta-description', 'http-status-code', 'is-crawlable', 'robots-txt', 'canonical', 'hreflang', 'link-text', 'crawlable-anchors', 'image-alt', 'structured-data'] as const
export type PageSpeedCheckId = typeof pageSpeedCheckIds[number]
export type PageSpeedCheckStatus = 'passed' | 'failed' | 'not_applicable' | 'manual' | 'informative' | 'error' | 'not_reported'
export type PageSpeedCheck = Readonly<{ id: PageSpeedCheckId; status: PageSpeedCheckStatus; score: number | null }>
export type PageSpeedAuditCommand = Readonly<{ site: string; strategy: PageSpeedStrategy; websiteId?: string }>
export const pageSpeedAuditContract = {
  id: 'marketing.pagespeed.run', ownerPlane: 'engagement', risk: 'low',
  allowedActorTypes: ['human'], method: 'POST', path: '/api/v1/visibility/pagespeed',
  output: { diagnostics: 'fixed_homepage_lighthouse_checks', checkIds: pageSpeedCheckIds, legacyChecksOptional: true },
} as const
