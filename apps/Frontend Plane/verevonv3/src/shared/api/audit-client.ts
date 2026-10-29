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

/** A validated, UI-facing audit row normalized from Audit Core's wire format. */
export interface AuditEvent {
  id?: string
  event?: string
  actor?: string
  actorRole?: string
  userId?: string
  resource?: string
  outcome?: string
  occurredAt?: string
  requestId?: string
  ipAddress?: string
  /** Zero-data-retention marker — may arrive top-level or inside `details`. */
  zdr?: boolean
  /** Free-form per-event metadata. For `tool_action`: `{ tool, data_category, source, zdr }`. */
  details?: Record<string, unknown>
}

export interface AuditQuery {
  event?: string
  userId?: string
  since?: string
  until?: string
  limit?: number
}

type AuditListResponse = { data?: unknown[] } | unknown[]

function recordFrom(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined
}

function textField(row: Record<string, unknown>, ...keys: string[]): string | undefined {
  for (const key of keys) {
    const value = row[key]
    if (typeof value === 'string' && value.trim()) return value.trim()
  }
  return undefined
}

function normalizeAuditEvent(value: unknown): AuditEvent | undefined {
  const row = recordFrom(value)
  if (!row) return undefined

  const rawId = row.id
  const details = recordFrom(row.details)
  return {
    ...(typeof rawId === 'string' || typeof rawId === 'number' ? { id: String(rawId) } : {}),
    ...(textField(row, 'event', 'action') ? { event: textField(row, 'event', 'action') } : {}),
    ...(textField(row, 'actor') ? { actor: textField(row, 'actor') } : {}),
    ...(textField(row, 'actor_role', 'actorRole') ? { actorRole: textField(row, 'actor_role', 'actorRole') } : {}),
    ...(textField(row, 'user_id', 'userId') ? { userId: textField(row, 'user_id', 'userId') } : {}),
    ...(textField(row, 'resource', 'resource_id', 'subject') ? { resource: textField(row, 'resource', 'resource_id', 'subject') } : {}),
    ...(textField(row, 'outcome', 'status') ? { outcome: textField(row, 'outcome', 'status') } : {}),
    ...(textField(row, 'occurred_at', 'occurredAt', 'created_at', 'createdAt', 'timestamp')
      ? { occurredAt: textField(row, 'occurred_at', 'occurredAt', 'created_at', 'createdAt', 'timestamp') }
      : {}),
    ...(textField(row, 'request_id', 'requestId') ? { requestId: textField(row, 'request_id', 'requestId') } : {}),
    ...(textField(row, 'ip_address', 'ipAddress') ? { ipAddress: textField(row, 'ip_address', 'ipAddress') } : {}),
    ...(typeof row.zdr === 'boolean' ? { zdr: row.zdr } : {}),
    ...(details ? { details } : {}),
  }
}

function rowsFrom(payload: AuditListResponse): AuditEvent[] {
  const rows = Array.isArray(payload)
    ? payload
    : Array.isArray(payload?.data) ? payload.data : []
  return rows.flatMap((row) => {
    const normalized = normalizeAuditEvent(row)
    return normalized ? [normalized] : []
  })
}

/**
 * List audit events for the current org. `requestJson` already unwraps a
 * top-level `{ data }` envelope, but audit-core nests rows under `data` AND
 * carries `meta`, so the unwrap may hand back either shape — normalize both.
 */
export async function listAuditEvents(query: AuditQuery = {}, signal?: AbortSignal): Promise<AuditEvent[]> {
  const params = new URLSearchParams()
  if (query.event) params.set('event', query.event)
  if (query.userId) params.set('user_id', query.userId)
  if (query.since) params.set('since', query.since)
  if (query.until) params.set('until', query.until)
  if (query.limit != null) params.set('limit', String(query.limit))

  const suffix = params.toString()
  const payload = await requestJson<AuditListResponse>(
    `/api/v1/audit${suffix ? `?${suffix}` : ''}`,
    { signal },
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

/**
 * Whether an event was processed under zero data retention. The flag may be
 * carried top-level (`zdr`) or inside `details` under any of a few names. Absent
 * ⇒ false (we never imply ZDR we can't see).
 */
export function zeroDataRetention(event: AuditEvent): boolean {
  if (event.zdr === true) return true
  const details = event.details
  if (!details) return false
  for (const key of ['zdr', 'zero_data_retention', 'zeroDataRetention']) {
    const value = details[key]
    if (value === true || value === 'true') return true
  }
  return false
}

/** One data category the AI has touched across the workspace. */
export interface DataCategoryActivity {
  category: string
  /** Tool-action events recorded against this category. */
  count: number
  /** How many of those events were processed under zero data retention. */
  zdrCount: number
}

/**
 * Workspace-level AI activity: a per-data-category rollup over ALL tool-action
 * events, regardless of whether an event could be attributed to a specific
 * connection. This is the honest granularity available today — per-connection
 * attribution is deferred until producers emit a reliable source/tool identity.
 */
export interface WorkspaceAiActivity {
  totalEvents: number
  zdrEvents: number
  categories: DataCategoryActivity[]
  tools: string[]
}

/**
 * Build the workspace rollup. Unlike {@link aggregateToolActions}, this counts
 * EVERY event — including those with no attributable source — so a tool action
 * the AI took is never silently dropped from the workspace view. Events without
 * a declared data category fall under "Uncategorized".
 */
export function aggregateWorkspaceActivity(events: AuditEvent[]): WorkspaceAiActivity {
  const categories = new Map<string, DataCategoryActivity>()
  const tools = new Set<string>()
  let zdrEvents = 0

  for (const event of events) {
    const zdr = zeroDataRetention(event)
    if (zdr) zdrEvents += 1

    const category = dataCategory(event) ?? 'Uncategorized'
    const existing = categories.get(category) ?? { category, count: 0, zdrCount: 0 }
    existing.count += 1
    if (zdr) existing.zdrCount += 1
    categories.set(category, existing)

    const tool = toolName(event)
    if (tool) tools.add(tool)
  }

  return {
    totalEvents: events.length,
    zdrEvents,
    categories: [...categories.values()].sort((a, b) => b.count - a.count),
    tools: [...tools],
  }
}
