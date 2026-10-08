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
	coverage?: 'measured' | 'unknown'
	checkedAt?: string
  id: string
  label: string
  definition: string
  metric: string
  source: string
  surface: string
  unit: string
  value: number
  eventCount: number
  aggregation: string
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
  contractVersion: string
  coverageDetail: string
  coverage: 'unknown' | 'measured'
  generatedAt: string
  window: { from?: string; to?: string }
  scorecards: InsightScorecard[]
  sourceCount: number
}

type InsightOverviewWire = {
	coverage?: unknown
	coverage_detail?: unknown
	contract_version?: unknown
  generated_at?: unknown
  scorecards?: unknown
  source_count?: unknown
	window?: unknown
}

function asString(value: unknown): string {
  return typeof value === 'string' ? value : ''
}

// Reject malformed measurement data. Treating bad values or a broken response
// as zero would turn a contract failure into a convincing but false metric.
export function normalizeInsightOverview(value: unknown): InsightOverview {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid Insight Core report')
  const wire = value as InsightOverviewWire
  if (!Array.isArray(wire.scorecards) || typeof wire.generated_at !== 'string') throw new Error('Incomplete Insight Core report')
  const scorecards = wire.scorecards.map(normalizeScorecard)
  const window = wire.window && typeof wire.window === 'object' ? wire.window as Record<string, unknown> : {}
  const sourceCount = wire.source_count === undefined ? scorecards.length : finiteNumber(wire.source_count)
  return {
    contractVersion: typeof wire.contract_version === 'string' ? wire.contract_version : '1.x',
    coverageDetail: typeof wire.coverage_detail === 'string' ? wire.coverage_detail : '',
    coverage: wire.coverage === 'measured' ? 'measured' : 'unknown',
    generatedAt: wire.generated_at,
    window: {
      ...(typeof window.from === 'string' ? { from: window.from } : {}),
      ...(typeof window.to === 'string' ? { to: window.to } : {}),
    },
    scorecards,
    sourceCount,
  }
}

function normalizeScorecard(value: unknown): InsightScorecard {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid Insight Core scorecard')
  const wire = value as Record<string, unknown>
  for (const key of ['id', 'label', 'metric', 'surface', 'unit', 'aggregation']) {
    if (typeof wire[key] !== 'string') throw new Error(`Invalid Insight Core scorecard field: ${key}`)
  }
  if (typeof wire.source !== 'string' && wire.source !== undefined) throw new Error('Invalid Insight Core scorecard source')
  return {
    id: wire.id as string,
		coverage: wire.coverage === 'measured' ? 'measured' : 'unknown',
		checkedAt: typeof wire.checked_at === 'string' ? wire.checked_at : undefined,
    label: wire.label as string,
    definition: typeof wire.definition === 'string' ? wire.definition : 'Definition not supplied by the reporting source.',
    metric: wire.metric as string,
    source: (wire.source as string | undefined) ?? '',
    surface: wire.surface as string,
    unit: wire.unit as string,
    aggregation: wire.aggregation as string,
    eventCount: finiteNumber(wire.event_count),
    value: finiteNumber(wire.value),
  }
}

function finiteNumber(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) throw new Error('Invalid numeric value in Insight Core report')
  return value
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
