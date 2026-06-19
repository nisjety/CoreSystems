import { requestJson } from './http'

/**
 * Audit read client.
 *
 * Backs the Trust Center: queries the gateway audit proxy (audit-core) for
 * tool-action events so the UI can show, per integration, which data categories
 * the AI has actually fetched and whether the AI has touched the source at all.
 *
 * The gateway scopes every read to the caller's authorized org server-side; the
 * SPA never passes an org id here.
 */

/** A single audit row as returned by audit-core (snake_case, loosely typed). */
export interface AuditEvent {
  id?: string
  event?: string
  actor?: string
  userId?: string
  user_id?: string
  resource?: string
  outcome?: string
  createdAt?: string
  created_at?: string
  /** Free-form per-event metadata. For `tool_action`: `{ tool, data_category, source }`. */
  details?: Record<string, unknown>
}

export interface AuditQuery {
  event?: string
  userId?: string
  since?: string
  until?: string
  limit?: number
}

type AuditListResponse = { data?: AuditEvent[] } | AuditEvent[]

function rowsFrom(payload: AuditListResponse): AuditEvent[] {
  if (Array.isArray(payload)) return payload
  return Array.isArray(payload?.data) ? payload.data : []
}

/**
 * List audit events for the current org. `requestJson` already unwraps a
 * top-level `{ data }` envelope, but audit-core nests rows under `data` AND
 * carries `meta`, so the unwrap may hand back either shape — normalize both.
 */
export async function listAuditEvents(query: AuditQuery = {}): Promise<AuditEvent[]> {
  const params = new URLSearchParams()
  if (query.event) params.set('event', query.event)
  if (query.userId) params.set('user_id', query.userId)
  if (query.since) params.set('since', query.since)
  if (query.until) params.set('until', query.until)
  if (query.limit != null) params.set('limit', String(query.limit))

  const suffix = params.toString()
  const payload = await requestJson<AuditListResponse>(
    `/api/v1/audit${suffix ? `?${suffix}` : ''}`,
  )
  return rowsFrom(payload)
}

/** Convenience: the tool-action events the Trust Center aggregates over. */
export function listToolActionEvents(limit = 500): Promise<AuditEvent[]> {
  return listAuditEvents({ event: 'tool_action', limit })
}

function detailString(details: Record<string, unknown> | undefined, ...keys: string[]): string | undefined {
  if (!details) return undefined
  for (const key of keys) {
    const value = details[key]
    if (typeof value === 'string' && value.trim().length > 0) return value.trim()
  }
  return undefined
}

/** Tool name recorded on a `tool_action` event (`details.tool`). */
export function toolName(event: AuditEvent): string | undefined {
  return detailString(event.details, 'tool', 'toolName')
}

/** Data category the tool touched (`details.data_category`). */
export function dataCategory(event: AuditEvent): string | undefined {
  return detailString(event.details, 'data_category', 'dataCategory')
}

/**
 * The source/connection the action relates to, used to attribute an event to a
 * connected app. Falls back across the field names audit producers may use.
 */
export function eventSource(event: AuditEvent): string | undefined {
  return (
    detailString(event.details, 'source', 'provider', 'connection', 'connectionId', 'providerId') ??
    (typeof event.resource === 'string' ? event.resource : undefined)
  )
}

/**
 * Per-source aggregation of tool-action events: the distinct data categories
 * fetched and the distinct tools used. The Trust Center keys this by a
 * normalized source token (provider id, connection id, or resource).
 */
export interface ToolActionSummary {
  /** Whether any tool-action event was recorded for this source (= "used by AI"). */
  usedByAi: boolean
  /** Distinct data categories the AI fetched for this source. */
  dataCategories: string[]
  /** Distinct tools the AI invoked against this source. */
  tools: string[]
  /** Number of tool-action events attributed to this source. */
  count: number
}

/**
 * Build a map from normalized source token → {@link ToolActionSummary}.
 * Tokens are lowercased so callers can match a connection/provider id
 * case-insensitively. Events with no attributable source are ignored.
 */
export function aggregateToolActions(events: AuditEvent[]): Map<string, ToolActionSummary> {
  const summaries = new Map<string, ToolActionSummary>()
  for (const event of events) {
    const source = eventSource(event)
    if (!source) continue
    const key = source.toLowerCase()
    const existing = summaries.get(key) ?? {
      usedByAi: true,
      dataCategories: [],
      tools: [],
      count: 0,
    }
    existing.count += 1
    const category = dataCategory(event)
    if (category && !existing.dataCategories.includes(category)) {
      existing.dataCategories.push(category)
    }
    const tool = toolName(event)
    if (tool && !existing.tools.includes(tool)) {
      existing.tools.push(tool)
    }
    summaries.set(key, existing)
  }
  return summaries
}
