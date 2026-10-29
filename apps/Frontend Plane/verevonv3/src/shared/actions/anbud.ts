/** Reads tenders for an explicit customer profile; organization scope comes from the session. */
export const anbudSearchContract = {
  id: 'anbud.search', method: 'POST', path: '/api/v1/anbud/matches',
  readOnly: true, requiresExplicitUserAction: true, allowedActorTypes: ['human'],
  views: { pipeline: 'relevant', all: 'all' },
  pagination: { pageSize: 50, cursor: true, globalSearchAndSort: true },
  coverage: 'all_active', retainedStages: ['review', 'working', 'sent', 'hidden'],
  advisorySignals: ['company_seo_keywords', 'user_position', 'user_department'],
} as const

/** Browses public notice metadata without a customer profile. Session scope remains enforced by the gateway. */
export const anbudCatalogueContract = {
  id: 'anbud.catalogue.read', method: 'POST', path: '/api/v1/anbud/notices/search',
  readOnly: true, allowedActorTypes: ['human'],
} as const

/** Reads the public source details of one held notice; no company profile or private uploads. */
export const anbudNoticeDetailContract = {
  id: 'anbud.notice.read', method: 'POST', path: '/api/v1/anbud/notices',
  readOnly: true, requiresExplicitUserAction: true, allowedActorTypes: ['human'],
} as const
