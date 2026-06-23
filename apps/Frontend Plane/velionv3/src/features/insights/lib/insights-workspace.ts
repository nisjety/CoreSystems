import { getAuthSession, getSessionContext } from '@/shared/api/auth-client'
import {
  getInsightsOverview,
  listInsightConnectors,
  type InsightConnector,
  type InsightScorecard,
} from '@/shared/api/insights-client'
import { listConnections, type IntegrationConnection } from '@/shared/api/integrations-client'
import { listResult, withResourceTimeout } from '@/shared/read-data'
import type { MeasurementState, ResourceResult } from '@/shared/read-data'

// The read-data substrate (Phase 4 PR-1) owns MeasurementState / ResourceResult
// / withResourceTimeout. Re-exported here so existing insights importers keep
// their paths and the honesty contract stays single-sourced: `live` still never
// attaches to a rendered metric VALUE — only the connector registry is real.
export type { MeasurementState, ResourceResult }
export { withResourceTimeout } from '@/shared/read-data'

export type InsightsContext = {
  email: string
  name: string
  orgId: string
  orgLabel: string
}

export type InsightsWorkspace = {
  context: InsightsContext
  externalAnalytics: ExternalAnalyticsSlot[]
  insightConnectors: ResourceResult<InsightConnector[]>
  integrations: ResourceResult<IntegrationConnection[]>
  // Real, recorded metric scorecards. `live` ONLY when insight-core produced
  // rows; zero rows resolve to `empty`, a failure/timeout to `unavailable` —
  // `live` never attaches to an unproduced value.
  overview: ResourceResult<InsightScorecard[]>
}

export type ExternalAnalyticsSlot = {
  connectorId: 'ga4' | 'search-console'
  detail: string
  docsHref: string
  metricConcepts: string[]
  reportConcepts: string[]
  responseConcepts: string[]
  status: MeasurementState | 'connected_pending_reports'
  statusLabel: string
  title: string
}

const INSIGHTS_RESOURCE_TIMEOUT_MS = 3500

export async function loadInsightsWorkspace(): Promise<InsightsWorkspace> {
  const context = await loadInsightsContext()
  const [insightConnectors, integrations, overview] = await Promise.all([
    withResourceTimeout(readInsightConnectors(), {
      data: [],
      message: 'Insight connector registry did not respond before the dashboard timeout.',
      state: 'unavailable',
    }),
    withResourceTimeout(readIntegrations(context.orgId), {
      data: [],
      message: 'Integration registry did not respond before the dashboard timeout.',
      state: 'unavailable',
    }),
    withResourceTimeout(readInsightsOverview(), {
      data: [],
      message: 'Metric overview did not respond before the dashboard timeout.',
      state: 'unavailable',
    }),
  ])

  return {
    context,
    externalAnalytics: buildExternalAnalyticsSlots(integrations, insightConnectors),
    insightConnectors,
    integrations,
    overview,
  }
}

export async function loadInsightsContext(): Promise<InsightsContext> {
  const [session, ctx] = await Promise.all([
    withValueTimeout(getAuthSession().catch(() => null), null),
    withValueTimeout(getSessionContext().catch(() => null), null),
  ])
  const activeOrg = ctx?.orgs[0] ?? null

  return {
    email: session?.user.email ?? ctx?.email ?? '',
    name: session?.user.name ?? ctx?.name ?? '',
    orgId: activeOrg?.id ?? ctx?.orgId ?? '',
    orgLabel: activeOrg?.name ?? ctx?.orgId ?? 'Velion',
  }
}

async function withValueTimeout<T>(
  value: Promise<T>,
  fallback: T,
  timeoutMs = INSIGHTS_RESOURCE_TIMEOUT_MS,
): Promise<T> {
  let timeout: ReturnType<typeof setTimeout> | undefined
  const timeoutResult = new Promise<T>((resolve) => {
    timeout = setTimeout(() => resolve(fallback), timeoutMs)
  })

  try {
    return await Promise.race([value, timeoutResult])
  } finally {
    if (timeout) clearTimeout(timeout)
  }
}

// The gateway resolves the org from the session, so the SPA sends no org scope.
async function readInsightConnectors(): Promise<InsightsWorkspace['insightConnectors']> {
  try {
    const connectors = await listInsightConnectors()
    return {
      data: connectors,
      message: connectors.length
        ? 'Insight-core connector registry is available through the Velion gateway.'
        : 'Insight-core connector registry is live, but no connectors are registered yet.',
      state: connectors.length ? 'live' : 'empty',
    }
  } catch {
    return {
      data: [],
      message: 'Insight-core connector proxy is not available yet; falling back to integration connections.',
      state: 'unavailable',
    }
  }
}

// Map real recorded scorecards to a ResourceResult. `listResult` is the
// anti-fabrication helper: rows → `live`, zero rows → `empty` (an honest
// "not yet reporting"). `live` is therefore NEVER attached to an unproduced
// metric value. Pure, so the state mapping is unit-testable.
export function overviewResult(scorecards: InsightScorecard[]): ResourceResult<InsightScorecard[]> {
  return listResult(scorecards, {
    empty: 'Measurement layer is live, but no connector has reported a metric yet.',
    live: 'Showing real recorded metrics from the measurement layer.',
  })
}

// The gateway resolves the org from the session, so the SPA sends no org scope.
// A failure resolves to `unavailable` rather than a fabricated empty/live state.
async function readInsightsOverview(): Promise<InsightsWorkspace['overview']> {
  try {
    const overview = await getInsightsOverview()
    return overviewResult(overview.scorecards)
  } catch {
    return {
      data: [],
      message: 'Metric overview is not available yet; the measurement layer did not respond.',
      state: 'unavailable',
    }
  }
}

async function readIntegrations(orgId: string): Promise<InsightsWorkspace['integrations']> {
  if (!orgId.trim()) {
    return {
      data: [],
      message: 'No organization scope was resolved for connector measurement.',
      state: 'unavailable',
    }
  }

  try {
    const connections = normalizeIntegrationConnections(await listConnections(orgId))
    return {
      data: connections,
      message: connections.length
        ? 'Integration connection registry is live.'
        : 'Integration registry is live, but no analytics connectors are connected.',
      state: connections.length ? 'live' : 'empty',
    }
  } catch {
    return {
      data: [],
      message: 'Integration connector registry is unavailable.',
      state: 'unavailable',
    }
  }
}

export function normalizeIntegrationConnections(value: unknown): IntegrationConnection[] {
  if (Array.isArray(value)) return value
  if (value && typeof value === 'object') {
    const record = value as Record<string, unknown>
    if (Array.isArray(record.connections)) return record.connections as IntegrationConnection[]
    if (Array.isArray(record.items)) return record.items as IntegrationConnection[]
  }
  return []
}

export function buildExternalAnalyticsSlots(
  integrations: ResourceResult<IntegrationConnection[]>,
  insightConnectors?: ResourceResult<InsightConnector[]>,
): ExternalAnalyticsSlot[] {
  const ga4InsightConnector = findInsightConnector(insightConnectors?.data ?? [], 'ga4')
  const searchConsoleInsightConnector = findInsightConnector(insightConnectors?.data ?? [], 'search_console')
  const shouldUseInsightConnectors = insightConnectors?.state === 'live' || insightConnectors?.state === 'empty'
  const ga4Connection = shouldUseInsightConnectors ? null : findConnection(integrations.data, ['google-analytics', 'google analytics', 'ga4'])
  const searchConsoleConnection = shouldUseInsightConnectors ? null : findConnection(integrations.data, [
    'google-search-console',
    'search-console',
    'search console',
    'webmaster',
    'seo',
  ])

  return [
    {
      connectorId: 'ga4',
      title: 'Google Analytics 4',
      docsHref: 'https://developers.google.com/analytics/devguides/reporting/data/v1/rest/v1beta/properties/runReport',
      detail: connectorDetail(
        integrations,
        ga4Connection,
        insightConnectors,
        ga4InsightConnector,
        'OAuth connection is present; Insights still needs a GA4 report proxy before rows can be shown.',
        'Connect a GA4 property before Velion can request event and conversion reports.',
      ),
      metricConcepts: ['activeUsers', 'sessions', 'conversions', 'eventCount', 'totalRevenue'],
      reportConcepts: ['property', 'dateRanges[]', 'dimensions[]', 'metrics[]', 'limit / offset'],
      responseConcepts: ['dimensionHeaders[]', 'metricHeaders[]', 'rows[]', 'rowCount', 'metadata'],
      status: connectorStatus(integrations, ga4Connection, insightConnectors, ga4InsightConnector),
      statusLabel: connectorStatusLabel(integrations, ga4Connection, insightConnectors, ga4InsightConnector),
    },
    {
      connectorId: 'search-console',
      title: 'Search Console SEO',
      docsHref: 'https://developers.google.com/webmaster-tools/v1/searchanalytics/query',
      detail: connectorDetail(
        integrations,
        searchConsoleConnection,
        insightConnectors,
        searchConsoleInsightConnector,
        'OAuth connection is present; Insights still needs a Search Console query proxy before SEO rows can be shown.',
        'Connect Search Console before Velion can read query and page performance.',
      ),
      metricConcepts: ['queries', 'pages', 'clicks', 'impressions', 'CTR', 'position'],
      reportConcepts: ['siteUrl', 'startDate / endDate', 'dimensions[]', 'rowLimit / startRow', 'dataState'],
      responseConcepts: ['rows[].keys[]', 'rows[].clicks', 'rows[].impressions', 'rows[].ctr', 'rows[].position'],
      status: connectorStatus(integrations, searchConsoleConnection, insightConnectors, searchConsoleInsightConnector),
      statusLabel: connectorStatusLabel(integrations, searchConsoleConnection, insightConnectors, searchConsoleInsightConnector),
    },
  ]
}

function findInsightConnector(
  connectors: readonly InsightConnector[],
  kind: InsightConnector['kind'],
): InsightConnector | null {
  return connectors.find((connector) => connector.kind === kind) ?? null
}

function findConnection(
  connections: readonly IntegrationConnection[],
  needles: readonly string[],
): IntegrationConnection | null {
  if (!Array.isArray(connections)) return null
  return connections.find((connection) => {
    const haystack = [
      connection.id,
      connection.providerId,
      connection.providerName,
      connection.status,
    ].filter(Boolean).join(' ').toLowerCase()
    return needles.some((needle) => haystack.includes(needle))
  }) ?? null
}

function connectorStatus(
  integrations: ResourceResult<IntegrationConnection[]>,
  connection: IntegrationConnection | null,
  insightConnectors?: ResourceResult<InsightConnector[]>,
  insightConnector?: InsightConnector | null,
): ExternalAnalyticsSlot['status'] {
  if (insightConnectors?.state === 'live' || insightConnectors?.state === 'empty') {
    if (!insightConnector || insightConnector.status === 'not_connected' || insightConnector.status === 'needs_oauth') {
      return 'not_connected'
    }
    if (insightConnector.status === 'unavailable') return 'unavailable'
    return 'connected_pending_reports'
  }

  if (integrations.state === 'unavailable') return 'unavailable'
  if (!connection || connection.status !== 'connected') return 'not_connected'
  return 'connected_pending_reports'
}

function connectorStatusLabel(
  integrations: ResourceResult<IntegrationConnection[]>,
  connection: IntegrationConnection | null,
  insightConnectors?: ResourceResult<InsightConnector[]>,
  insightConnector?: InsightConnector | null,
): string {
  const status = connectorStatus(integrations, connection, insightConnectors, insightConnector)
  switch (status) {
    case 'connected_pending_reports':
      return 'Connected, reports pending'
    case 'not_connected':
      return 'Not connected'
    case 'unavailable':
      return 'Connector registry unavailable'
    default:
      return 'Unavailable'
  }
}

function connectorDetail(
  integrations: ResourceResult<IntegrationConnection[]>,
  connection: IntegrationConnection | null,
  insightConnectors: ResourceResult<InsightConnector[]> | undefined,
  insightConnector: InsightConnector | null,
  connectedDetail: string,
  disconnectedDetail: string,
): string {
  if (insightConnectors?.state === 'live' || insightConnectors?.state === 'empty') {
    if (insightConnector?.status === 'connected') {
      return insightConnector.reportShape?.rowsAvailable
        ? `${insightConnector.label} is connected through insight-core. Report rows are available once the UI table is wired.`
        : `${insightConnector.label} is connected through insight-core; report rows are still pending.`
    }
    if (insightConnector?.status === 'unavailable') return `${insightConnector.label} is registered but unavailable.`
    return disconnectedDetail
  }

  if (integrations.state === 'unavailable') return integrations.message
  if (connection?.status === 'connected') return connectedDetail
  return disconnectedDetail
}
