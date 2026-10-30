/** Read-only insights reuse the authenticated Marketing/site permission boundary. */
export const marketingInsightsContracts = {
  insights: { id: 'marketing.audit.insights.read', method: 'GET', path: '/api/v1/visibility/audit-insights', readOnly: true, allowedActorTypes: ['human'] },
  weekly: { id: 'marketing.reports.weekly.read', method: 'GET', path: '/api/v1/visibility/reports/weekly', readOnly: true, allowedActorTypes: ['human'] },
} as const
