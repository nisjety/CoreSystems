import type { Analysis, Competitors, Context } from '../tools-client'

/** Test evidence only; never imported by the running UI. */
export const context: Context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 7 }
export function analysis(): Analysis {
  return {
    ...context, source: 'visibility_analysis', period: { startDate: '2026-10-01', endDate: '2026-10-27' }, generatedAt: '2026-10-28T12:00:00Z',
    coverage: { searchStatus: 'connected', searchAt: '2026-10-27T12:00:00Z', searchTruncated: false, auditStatus: 'complete', auditAt: '2026-10-28T10:00:00Z', auditId: `audit_${'1'.repeat(32)}`, checked: 1, failed: 0, discoveryStatus: 'complete', partial: true, linkLimit: 100 },
    pages: [
      { url: context.site, title: 'Measured homepage', description: 'Observed description', canonical: context.site, auditStatus: 'checked', httpStatus: 200, observedAt: '2026-10-28T10:00:00Z', clicks: 0, impressions: 100, ctr: 0, position: 12, issues: 0, inbound: 0, outbound: 1, depth: 0, discovered: true },
      { url: `${context.site}unmeasured`, title: 'Unmeasured page', description: '', canonical: '', auditStatus: 'not_checked', httpStatus: null, observedAt: '', clicks: null, impressions: null, ctr: null, position: null, issues: null, inbound: 1, outbound: null, depth: 1, discovered: true },
    ],
    groups: [
      { key: 'near_page_one', entity: 'query', items: [{ key: 'relevant query', clicks: 0, impressions: 100, ctr: 0, position: 12 }] },
      { key: 'low_ctr', entity: 'query', items: [] },
      { key: 'technical_review', entity: 'page', items: [] },
      { key: 'few_incoming_links', entity: 'page', items: [{ key: `${context.site}unmeasured`, url: `${context.site}unmeasured` }] },
    ],
    edges: [{ from: context.site, to: `${context.site}unmeasured`, status: 'not_checked' }],
  }
}
export const comparisons = (): Competitors => ({ ...context, source: 'quarry_comparison', limit: 3, entries: [] })
