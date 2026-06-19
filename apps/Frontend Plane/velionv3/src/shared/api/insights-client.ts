import { requestJson } from './http'

// The gateway returns `kind` and `status` as free-form strings sourced from the
// connector registry ("social", "inbox", "agents", "native", "planned",
// "external_analytics", "google_analytics_4", ...). They are NOT a fixed union,
// so the SPA must treat them as opaque strings and never assume a closed set.
export type InsightConnectorKind = string
export type InsightConnectorStatus = string

export type InsightDateRange = {
  endDate: string
  startDate: string
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

// Gateway-facing only. The browser must not call Application Plane insight-core
// directly, attach internal service keys, or send any org header: the gateway
// resolves the org from the session and serves the connector registry through
// `GET /api/v1/insights/connectors` as a `{ data: InsightConnector[] }` envelope.
// `requestJson` already unwraps the top-level `data`, so this resolves to the
// connector array directly.
export function listInsightConnectors(): Promise<InsightConnector[]> {
  return requestJson<InsightConnector[]>('/api/v1/insights/connectors')
}
