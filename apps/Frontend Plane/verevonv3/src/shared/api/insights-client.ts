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

// The compact per-org metric overview the gateway assembles from insight-core.
// `sourceCount` is the number of REAL scorecards (produced rows); the SPA uses
// it to decide a `live` vs honest-`empty` state — `live` never attaches to an
// unproduced value.
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
export async function getInsightsOverview(): Promise<InsightOverview> {
  const raw = await requestJson<unknown>('/api/v1/insights/overview')
  return normalizeInsightOverview(raw)
}
