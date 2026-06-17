import { fetchChatbotSupportStatus, type SupportIntegrationStatus } from '@/features/agents/lib/use-chatbot-support-status'
import { getAuthSession, getSessionContext } from '@/shared/api/auth-client'
import {
  getInsightsOverview,
  listInsightConnectors,
  type InsightConnector,
  type InsightOverviewResponse,
} from '@/shared/api/insights-client'
import { listConnections, type IntegrationConnection } from '@/shared/api/integrations-client'
import { listConversations, type LiveTicket } from '@/shared/api/inbox-client'
import {
  getSocialCalendar,
  listSocialAdapters,
  type SocialCalendar,
  type SocialPlatformAdapter,
} from '@/shared/api/social-client'

export type InsightsSection = 'overview' | 'social' | 'inbox' | 'agents' | 'campaigns' | 'experiments'
export type MeasurementState = 'live' | 'empty' | 'not_connected' | 'unavailable' | 'planned'

export type InsightsContext = {
  email: string
  name: string
  orgId: string
  orgLabel: string
}

export type ResourceResult<T> = {
  data: T
  message: string
  state: MeasurementState
}

export type InsightsWorkspace = {
  agents: ResourceResult<SupportIntegrationStatus | null>
  context: InsightsContext
  externalAnalytics: ExternalAnalyticsSlot[]
  inbox: ResourceResult<LiveTicket[]>
  insightConnectors: ResourceResult<InsightConnector[]>
  insightCore: ResourceResult<InsightOverviewResponse | null>
  integrations: ResourceResult<IntegrationConnection[]>
  social: ResourceResult<{
    adapters: SocialPlatformAdapter[]
    calendar: SocialCalendar
  }>
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

const emptySocialCalendar: SocialCalendar = {
  accounts: [],
  posts: [],
  recommendedWindows: [],
}

const INSIGHTS_RESOURCE_TIMEOUT_MS = 3500

export async function loadInsightsWorkspace(): Promise<InsightsWorkspace> {
  const context = await loadInsightsContext()
  const [insightCore, insightConnectors, social, inbox, agents, integrations] = await Promise.all([
    withResourceTimeout(readInsightCore(context.orgId), {
      data: null,
      message: 'Insight-core did not respond before the dashboard timeout.',
      state: 'unavailable',
    }),
    withResourceTimeout(readInsightConnectors(context.orgId), {
      data: [],
      message: 'Insight connector registry did not respond before the dashboard timeout.',
      state: 'unavailable',
    }),
    withResourceTimeout(readSocial(context.orgId), {
      data: { adapters: [], calendar: emptySocialCalendar },
      message: 'Social measurement did not respond before the dashboard timeout.',
      state: 'unavailable',
    }),
    withResourceTimeout(readInbox(context.orgId), {
      data: [],
      message: 'Inbox measurement did not respond before the dashboard timeout.',
      state: 'unavailable',
    }),
    withResourceTimeout(readAgents(), {
      data: null,
      message: 'Agent measurement did not respond before the dashboard timeout.',
      state: 'unavailable',
    }),
    withResourceTimeout(readIntegrations(context.orgId), {
      data: [],
      message: 'Integration registry did not respond before the dashboard timeout.',
      state: 'unavailable',
    }),
  ])

  return {
    agents,
    context,
    externalAnalytics: buildExternalAnalyticsSlots(integrations, insightConnectors),
    inbox,
    insightConnectors,
    insightCore,
    integrations,
    social,
  }
}

export async function withResourceTimeout<T>(
  resource: Promise<ResourceResult<T>>,
  fallback: ResourceResult<T>,
  timeoutMs = INSIGHTS_RESOURCE_TIMEOUT_MS,
): Promise<ResourceResult<T>> {
  let timeout: ReturnType<typeof setTimeout> | undefined
  const timeoutResult = new Promise<ResourceResult<T>>((resolve) => {
    timeout = setTimeout(() => resolve(fallback), timeoutMs)
  })

  try {
    return await Promise.race([resource, timeoutResult])
  } finally {
    if (timeout) clearTimeout(timeout)
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

async function readInsightCore(orgId: string): Promise<InsightsWorkspace['insightCore']> {
  if (!orgId.trim()) {
    return {
      data: null,
      message: 'No organization scope was resolved for insight-core measurement.',
      state: 'unavailable',
    }
  }

  try {
    const overview = await getInsightsOverview(orgId)
    return {
      data: overview,
      message: 'Insight-core overview is available through the Velion gateway.',
      state: 'live',
    }
  } catch {
    return {
      data: null,
      message: 'Insight-core exists, but the Velion v3 gateway proxy is not available yet. Using derived contracts.',
      state: 'unavailable',
    }
  }
}

async function readInsightConnectors(orgId: string): Promise<InsightsWorkspace['insightConnectors']> {
  if (!orgId.trim()) {
    return {
      data: [],
      message: 'No organization scope was resolved for insight-core connectors.',
      state: 'unavailable',
    }
  }

  try {
    const result = await listInsightConnectors(orgId)
    return {
      data: result.connectors,
      message: result.connectors.length
        ? 'Insight-core connector registry is available through the Velion gateway.'
        : 'Insight-core connector registry is live, but no analytics connectors are connected.',
      state: result.connectors.length ? 'live' : 'empty',
    }
  } catch {
    return {
      data: [],
      message: 'Insight-core connector proxy is not available yet; falling back to integration connections.',
      state: 'unavailable',
    }
  }
}

async function readSocial(orgId: string): Promise<InsightsWorkspace['social']> {
  if (!orgId.trim()) {
    return {
      data: { adapters: [], calendar: emptySocialCalendar },
      message: 'No organization scope was resolved for social measurement.',
      state: 'unavailable',
    }
  }

  try {
    const [calendar, adaptersResult] = await Promise.all([
      getSocialCalendar(orgId),
      listSocialAdapters(orgId),
    ])
    const adapters = adaptersResult.adapters
    return {
      data: { adapters, calendar },
      message: calendar.accounts.length || calendar.posts.length
        ? 'Social calendar and adapter contracts are live.'
        : 'Social contracts are live, but no accounts or posts exist yet.',
      state: calendar.accounts.length || calendar.posts.length ? 'live' : 'empty',
    }
  } catch {
    return {
      data: { adapters: [], calendar: emptySocialCalendar },
      message: 'The org-scoped social gateway is unavailable.',
      state: 'unavailable',
    }
  }
}

async function readInbox(orgId: string): Promise<InsightsWorkspace['inbox']> {
  if (!orgId.trim()) {
    return {
      data: [],
      message: 'No organization scope was resolved for inbox measurement.',
      state: 'unavailable',
    }
  }

  try {
    const result = await listConversations(orgId, { limit: 100 })
    return {
      data: result.tickets,
      message: result.tickets.length
        ? 'Conversation-core summaries are live.'
        : 'Conversation-core is live, but no conversations matched the current scope.',
      state: result.tickets.length ? 'live' : 'empty',
    }
  } catch {
    return {
      data: [],
      message: 'The inbox conversation contract is unavailable.',
      state: 'unavailable',
    }
  }
}

async function readAgents(): Promise<InsightsWorkspace['agents']> {
  try {
    const status = await fetchChatbotSupportStatus()
    return {
      data: status,
      message: status.status === 'connected'
        ? 'Chatbot support runtime status is live.'
        : status.message,
      state: status.status === 'connected' ? 'live' : status.status === 'not-configured' ? 'not_connected' : 'unavailable',
    }
  } catch {
    return {
      data: null,
      message: 'Agent runtime measurement is unavailable.',
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
