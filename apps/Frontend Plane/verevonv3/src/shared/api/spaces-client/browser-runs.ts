// WS20-S5: the room browser run client. Receipts only (SD-23): every field here is an id, a count, a phase, a host or a
// closed word. A URL, title, rationale, selector, frame or decided-by subject never reaches this module.
//
// Wire (apps/gateway/src/domains/spaces/browser_runs.rs):
//   POST /api/v1/spaces/{ref}/browser-runs                          R1 -> 202 { data: { run_id, thread_id, plan_id, view: { scope } } }
//   GET  /api/v1/spaces/{ref}/browser-runs?thread_id=&limit=        R2 -> { data: [run], meta: { next_cursor } }
//   GET  /api/v1/spaces/{ref}/browser-runs/{run}/timeline?after=    R3 -> { data: { run, events, next_event_id, valid_until } }
//   GET  /api/v1/spaces/{ref}/browser-runs/{run}/evidence/{id}      R4 -> image bytes, the run's starter only
// Errors arrive as the typed `{error:{code,message,details}}` envelope (ApiError). Their copy lives in
// shared/i18n/errors.ts under the WS20 banner.
import { requestJson } from '../http'

/** The most hosts one room run may name (R1). */
export const SPACE_BROWSER_MAX_HOSTS = 8
/** The longest goal, in characters (R1). */
export const SPACE_BROWSER_MAX_GOAL_CHARS = 2000

/** The header every room run shows: ids, the starter, the phase, the times and the fixed control, hand-off and view. */
export type SpaceBrowserRunHeader = {
  readonly runId: string
  readonly threadId: string
  readonly startedBySubjectId: string
  readonly phase: string
  readonly startedAt: string
  readonly finishedAt: string | null
  readonly controlMode: string
  readonly handoffOpen: boolean
  readonly viewScope: string
}

/** One list row (R2): the header plus the hosts and the step count. */
export type SpaceBrowserRun = SpaceBrowserRunHeader & {
  readonly hosts: readonly string[]
  readonly stepCount: number
}

/** One timeline row (R3). Only the closed fields the projection keeps are present. */
export type SpaceBrowserEvent = {
  readonly eventId: string
  readonly at: string
  readonly kind: string
  readonly step?: string
  readonly actionType?: string
  readonly host?: string
  readonly status?: string
  readonly riskCategory?: string
  readonly evidenceId?: string
  readonly approvalId?: string
  readonly decision?: string
  readonly handoffKind?: string
  readonly outcome?: string
}

export type SpaceBrowserTimeline = {
  readonly run: SpaceBrowserRunHeader
  readonly events: readonly SpaceBrowserEvent[]
  readonly nextEventId: string | null
  readonly validUntil: string
}

/** The receipt the start answers: the ids and the view scope (`owner` in v1). */
export type SpaceBrowserRunReceipt = {
  readonly runId: string
  readonly threadId: string
  readonly planId: string
  readonly viewScope: string
}

/** What the start panel sends. The live view is never shared in v1, so `shareView` is not a field here. */
export type SpaceBrowserRunStart = {
  readonly threadId: string
  readonly goal: string
  readonly allowedDomains: readonly string[]
  readonly startUrl?: string
  readonly maxSteps?: number
  readonly maxRuntimeSeconds?: number
  /** One send's identity (16 to 64 letters, digits, `_` or `-`): a retry with the same id is the same run. */
  readonly clientSendId: string
}

/** Why a typed host is not a room host, as a closed word the panel can say. `null` is a valid host. */
export type SpaceBrowserHostProblem =
  | 'empty'
  | 'scheme'
  | 'path'
  | 'port'
  | 'ip_literal'
  | 'localhost'
  | 'wildcard'
  | 'format'

type WireRecord = Readonly<Record<string, unknown>>

function isRecord(value: unknown): value is WireRecord {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function text(value: unknown): string | null {
  return typeof value === 'string' && value.length > 0 ? value : null
}

function count(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : null
}

function parseHeader(wire: unknown): SpaceBrowserRunHeader | null {
  if (!isRecord(wire)) return null
  const started = isRecord(wire.started_by) ? text(wire.started_by.subject_id) : null
  const control = isRecord(wire.control) ? text(wire.control.mode) : null
  const view = isRecord(wire.view) ? text(wire.view.scope) : null
  const handoffOpen = isRecord(wire.handoff) && wire.handoff.open === true
  const runId = text(wire.run_id)
  const threadId = text(wire.thread_id)
  const phase = text(wire.phase)
  const startedAt = text(wire.started_at)
  if (!runId || !threadId || !started || !phase || !startedAt || !control || !view) return null
  return {
    runId,
    threadId,
    startedBySubjectId: started,
    phase,
    startedAt,
    finishedAt: text(wire.finished_at),
    controlMode: control,
    handoffOpen,
    viewScope: view,
  }
}

function parseRun(wire: unknown): SpaceBrowserRun | null {
  const header = parseHeader(wire)
  if (!header || !isRecord(wire)) return null
  const hosts = Array.isArray(wire.hosts)
    ? wire.hosts.filter((host): host is string => typeof host === 'string' && host.length > 0)
    : []
  const stepCount = count(wire.step_count)
  if (stepCount === null) return null
  return { ...header, hosts, stepCount }
}

function parseEvent(wire: unknown): SpaceBrowserEvent | null {
  if (!isRecord(wire)) return null
  const eventId = text(wire.event_id)
  const at = text(wire.at)
  const kind = text(wire.kind)
  if (!eventId || !at || !kind) return null
  const optional = {
    ...(text(wire.step) ? { step: text(wire.step) as string } : {}),
    ...(text(wire.action_type) ? { actionType: text(wire.action_type) as string } : {}),
    ...(text(wire.host) ? { host: text(wire.host) as string } : {}),
    ...(text(wire.status) ? { status: text(wire.status) as string } : {}),
    ...(text(wire.risk_category) ? { riskCategory: text(wire.risk_category) as string } : {}),
    ...(text(wire.evidence_id) ? { evidenceId: text(wire.evidence_id) as string } : {}),
    ...(text(wire.approval_id) ? { approvalId: text(wire.approval_id) as string } : {}),
    ...(text(wire.decision) ? { decision: text(wire.decision) as string } : {}),
    ...(text(wire.handoff_kind) ? { handoffKind: text(wire.handoff_kind) as string } : {}),
    ...(text(wire.outcome) ? { outcome: text(wire.outcome) as string } : {}),
  }
  return { eventId, at, kind, ...optional }
}

function unreadable(): Error {
  return new Error('space_browser_run_unreadable')
}

/** Starts a room browser run on one thread of the Space (R1). A retry with the same `clientSendId` is the same run. */
export async function startSpaceBrowserRun(
  spaceRef: string,
  input: SpaceBrowserRunStart,
): Promise<SpaceBrowserRunReceipt> {
  const body = {
    thread_id: input.threadId,
    goal: input.goal,
    allowed_domains: [...input.allowedDomains],
    ...(input.startUrl ? { start_url: input.startUrl } : {}),
    ...(input.maxSteps !== undefined ? { max_steps: input.maxSteps } : {}),
    ...(input.maxRuntimeSeconds !== undefined ? { max_runtime_s: input.maxRuntimeSeconds } : {}),
    share_view: false,
    client_send_id: input.clientSendId,
  }
  const wire = await requestJson<{ data?: unknown }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/browser-runs`,
    { method: 'POST', body: JSON.stringify(body) },
  )
  const raw = wire?.data
  const data = isRecord(raw) ? raw : null
  const runId = data ? text(data.run_id) : null
  const threadId = data ? text(data.thread_id) : null
  const planId = data ? text(data.plan_id) : null
  const viewScope = data && isRecord(data.view) ? text(data.view.scope) : null
  if (!runId || !threadId || !planId || !viewScope) throw unreadable()
  return { runId, threadId, planId, viewScope }
}

/** The Space's browser runs of one thread, newest first (R2). Receipts only. */
export async function listSpaceBrowserRuns(
  spaceRef: string,
  options: { readonly threadId: string; readonly limit?: number },
): Promise<{ readonly runs: readonly SpaceBrowserRun[]; readonly nextCursor: string | null }> {
  const query = new URLSearchParams({ thread_id: options.threadId })
  if (options.limit !== undefined) query.set('limit', String(options.limit))
  const wire = await requestJson<{ data?: unknown; meta?: unknown }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/browser-runs?${query.toString()}`,
  )
  const items: unknown = wire?.data
  if (!Array.isArray(items)) throw unreadable()
  const runs: SpaceBrowserRun[] = []
  for (const item of items) {
    const run = parseRun(item)
    if (!run) throw unreadable()
    runs.push(run)
  }
  const rawMeta = wire?.meta
  const meta: WireRecord = isRecord(rawMeta) ? rawMeta : {}
  return { runs, nextCursor: text(meta.next_cursor) }
}

/** One page of a run's room timeline (R3). The cursor is the last event id the caller has seen. */
export async function readSpaceBrowserTimeline(
  spaceRef: string,
  runId: string,
  options: { readonly after?: string; readonly limit?: number } = {},
): Promise<SpaceBrowserTimeline> {
  const query = new URLSearchParams()
  if (options.after) query.set('after', options.after)
  if (options.limit !== undefined) query.set('limit', String(options.limit))
  const suffix = query.toString() ? `?${query.toString()}` : ''
  const wire = await requestJson<{ data?: unknown }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/browser-runs/${encodeURIComponent(runId)}/timeline${suffix}`,
  )
  const raw = wire?.data
  const data = isRecord(raw) ? raw : null
  const run = data ? parseHeader(data.run) : null
  if (!data || !run || !Array.isArray(data.events)) throw unreadable()
  const events: SpaceBrowserEvent[] = []
  for (const item of data.events as unknown[]) {
    const event = parseEvent(item)
    if (!event) throw unreadable()
    events.push(event)
  }
  const validUntil = text(data.valid_until)
  if (!validUntil) throw unreadable()
  return { run, events, nextEventId: text(data.next_event_id), validUntil }
}

/**
 * The URL of one evidence image (R4). It answers the run's starter only; any other member gets 403
 * `browser_evidence_withheld`, so a caller shows the image only when it is the starter (`startedBySubjectId`).
 */
export function spaceBrowserEvidenceUrl(spaceRef: string, runId: string, evidenceId: string): string {
  return `/api/v1/spaces/${encodeURIComponent(spaceRef)}/browser-runs/${encodeURIComponent(runId)}/evidence/${encodeURIComponent(evidenceId)}`
}

/**
 * The first thing the panel says about a host: a bare hostname is the only shape a room run takes. The gateway and
 * Control check the same rules again; this is a hint for the person, never the decision.
 */
export function spaceBrowserHostProblem(input: string): SpaceBrowserHostProblem | null {
  const host = input.trim().toLowerCase()
  if (!host) return 'empty'
  if (/^[a-z][a-z0-9+.-]*:\/\//.test(host)) return 'scheme'
  if (/[/?#]/.test(host)) return 'path'
  if (host.includes(':')) return 'port'
  if (host.includes('*')) return 'wildcard'
  if (host === 'localhost' || host.endsWith('.localhost')) return 'localhost'
  if (host.length > 253) return 'format'
  const labels = host.split('.')
  const labelOk = (label: string) => /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(label)
  if (labels.length < 2 || !labels.every(labelOk)) {
    return /^[\d.]+$/.test(host) ? 'ip_literal' : 'format'
  }
  if (/^\d+$/.test(labels[labels.length - 1] ?? '')) return 'ip_literal'
  return null
}
