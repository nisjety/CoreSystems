import { requestJson } from './http'

export type InsightConnectorKind = 'ga4' | 'search_console' | 'unknown'
export type InsightConnectorStatus = 'connected' | 'not_connected' | 'needs_oauth' | 'unavailable'

export type InsightDateRange = {
  endDate: string
  startDate: string
}

export type InsightMetricSummary = {
  delta?: string
  label: string
  state?: string
  value: string
}

export type InsightSignalSummary = {
  detail: string
  href?: string | null
  source?: string
  state?: string
  title: string
}

export type InsightOverviewResponse = {
  generatedAt?: string
  metrics?: InsightMetricSummary[]
  range?: InsightDateRange
  signals?: InsightSignalSummary[]
  source?: string
  status?: string
}

export type InsightConnectorReportShape = {
  dimensions?: string[]
  metrics?: string[]
  range?: InsightDateRange
  rowsAvailable?: boolean
}

export type InsightConnector = {
  id: string
  kind: InsightConnectorKind
  label: string
  providerId?: string
  reportShape?: InsightConnectorReportShape
  status: InsightConnectorStatus
}

export type InsightConnectorsResponse = {
  connectors: InsightConnector[]
}

function orgHeaders(orgId: string): Record<string, string> {
  return { 'x-velion-org-id': orgId }
}

// Gateway-facing only. The browser must not call Application Plane insight-core
// directly or attach internal service keys; velion-gateway-rs will own that hop.
export function getInsightsOverview(orgId: string): Promise<InsightOverviewResponse> {
  return requestJson<InsightOverviewResponse>('/api/v1/insights/overview', {
    headers: orgHeaders(orgId),
  })
}

export function listInsightConnectors(orgId: string): Promise<InsightConnectorsResponse> {
  return requestJson<InsightConnectorsResponse>('/api/v1/insights/connectors', {
    headers: orgHeaders(orgId),
  })
}
