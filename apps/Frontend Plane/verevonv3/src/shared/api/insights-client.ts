import { requestJson } from './http'

// Insight Core's connector values are open-ended ("social", "inbox", "agents",
// "native", "planned", "external_analytics", ...). The SPA treats them as
// opaque strings and never assumes a closed set.
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

type InsightConnectorWire = {
  display_name?: unknown
  id?: unknown
  kind?: unknown
  label?: unknown
  status?: unknown
  surface?: unknown
  type?: unknown
}

// The gateway preserves Insight Core's owner contract. This adapter belongs in
// the SPA because it only translates field names for presentation; it does not
// calculate status, availability, or any metric.
export function normalizeInsightConnector(value: unknown): InsightConnector {
  const wire = (value && typeof value === 'object' ? value : {}) as InsightConnectorWire
  return {
    id: asString(wire.type ?? wire.id),
    kind: asString(wire.surface ?? wire.kind),
    label: asString(wire.display_name ?? wire.label),
    status: asString(wire.status),
  }
}

// Gateway-facing only. The browser must not call Application Plane Insight Core
// directly, attach internal service keys, or send any org header. The gateway
// resolves the org from the session and relays the owner response unchanged.
export async function listInsightConnectors(): Promise<InsightConnector[]> {
  const connectors = await requestJson<unknown>('/api/v1/insights/connectors')
  return Array.isArray(connectors) ? connectors.map(normalizeInsightConnector) : []
}

// One real, recorded metric the org's measurement layer produced. Every field
// is sourced from insight-core's rollup — `value` is a REAL aggregate and
// `source` cites the producer(s) behind it. The SPA must never render a
// scorecard the backend did not produce.
export type InsightScorecard = {
  id: string
  label: string
  metric: string
  source: string
  surface: string
  unit: string
  value: number
}

// The browser may narrow an Insights read to a reporting surface and time
// window, but never chooses the organization. The gateway derives that from
// the authenticated session before forwarding the request to insight-core.
export type InsightOverviewQuery = {
  from?: string
  // A reporting selector only. "me" is resolved to the authenticated session
  // actor by the gateway; the browser cannot name another user or tenant.
  scope?: 'organization' | 'me'
  surface?: string
  to?: string
}

// The per-org metric overview Insight Core produced. `sourceCount` is derived
// only from actual scorecards when the upstream has not included its count.
export type InsightOverview = {
  generatedAt: string
  scorecards: InsightScorecard[]
  sourceCount: number
}

type InsightOverviewWire = {
  generated_at?: unknown
  scorecards?: unknown
  source_count?: unknown
}

function asString(value: unknown): string {
  return typeof value === 'string' ? value : ''
}

function asNumber(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : 0
}

// Normalize the gateway's snake_case overview wire shape into the SPA's camelCase
// type. Unknown / missing fields collapse to empty/zero — never a fabricated
// value. A non-object payload yields a zero-scorecard overview (honest empty).
export function normalizeInsightOverview(value: unknown): InsightOverview {
  const wire = (value && typeof value === 'object' ? value : {}) as InsightOverviewWire
  const scorecards = Array.isArray(wire.scorecards)
    ? wire.scorecards.map(normalizeScorecard)
    : []
  return {
    generatedAt: asString(wire.generated_at),
    scorecards,
    sourceCount: typeof wire.source_count === 'number' ? wire.source_count : scorecards.length,
  }
}

function normalizeScorecard(value: unknown): InsightScorecard {
  const wire = (value && typeof value === 'object' ? value : {}) as Record<string, unknown>
  return {
    id: asString(wire.id),
    label: asString(wire.label),
    metric: asString(wire.metric),
    source: asString(wire.source),
    surface: asString(wire.surface),
    unit: asString(wire.unit),
    value: asNumber(wire.value),
  }
}

// Gateway-facing only (same IDOR-clean contract as the connectors route): the
// org is resolved from the session server-side; the SPA sends no org scope and
// no internal key. Resolves to the normalized overview, or an honest empty
// overview when the backend is unavailable handling is left to the caller.
export async function getInsightsOverview(query: InsightOverviewQuery = {}): Promise<InsightOverview> {
  const params = new URLSearchParams()
  if (query.surface?.trim()) params.set('surface', query.surface.trim())
  if (query.scope === 'me') params.set('scope', 'me')
  if (query.from?.trim()) params.set('from', query.from.trim())
  if (query.to?.trim()) params.set('to', query.to.trim())
  const suffix = params.size ? `?${params.toString()}` : ''
  const raw = await requestJson<unknown>(`/api/v1/insights/overview${suffix}`)
  return normalizeInsightOverview(raw)
}
