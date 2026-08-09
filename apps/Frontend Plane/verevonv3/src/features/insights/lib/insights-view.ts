import type { InsightOverviewQuery } from '@/shared/api/insights-client'

export type InsightSurface = 'social' | 'inbox' | 'agents' | 'chat' | 'knowledge' | 'ingestion' | 'campaigns' | 'external_analytics'
export type InsightsSection = 'overview' | InsightSurface | 'experiments'
export type InsightsRange = '7d' | '30d' | '90d' | 'all'
export type InsightsScope = 'organization' | 'me'

const sectionSurfaces: Partial<Record<InsightsSection, InsightSurface>> = {
  agents: 'agents',
  chat: 'chat',
  campaigns: 'campaigns',
  external_analytics: 'external_analytics',
  inbox: 'inbox',
  knowledge: 'knowledge',
  ingestion: 'ingestion',
  social: 'social',
}

const rangeDays: Partial<Record<InsightsRange, number>> = {
  '7d': 7,
  '30d': 30,
  '90d': 90,
}

export function surfaceForInsightsSection(section: InsightsSection): InsightSurface | undefined {
  return sectionSurfaces[section]
}

// Builds a reporting query without organization identity. The gateway resolves
// org scope from the authenticated session and allowlists this query before it
// reaches insight-core.
export function buildInsightsOverviewQuery(
  section: InsightsSection,
  range: InsightsRange,
  now = new Date(),
  scope: InsightsScope = 'organization',
): InsightOverviewQuery {
  const surface = surfaceForInsightsSection(section)
  const query: InsightOverviewQuery = {
    ...(surface ? { surface } : {}),
    ...(scope === 'me' ? { scope } : {}),
  }
  const days = rangeDays[range]
  if (!days) return query

  const from = new Date(now.getTime() - days * 24 * 60 * 60 * 1000)
  return {
    ...query,
    from: from.toISOString(),
    to: now.toISOString(),
  }
}
