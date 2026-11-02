import { requestJson } from './http'

export type RunItem = {
  id: string
  /** Temporal run id (`run_…`), stamped once the job starts. The evidence
   * endpoint only accepts run ids — `id` is the durable job id (`job_…`)
   * and 400s there — so evidence lookups must use this when present. */
  runId?: string | null
  kind: string
  status: string
  createdAt: string
  startedAt?: string | null
  completedAt?: string | null
  target: string
  progress: {
    completed?: number
    total?: number | null
    pages?: number
    urlCount?: number
    query?: string | null
  }
  stats: Record<string, unknown>
  /** WS15.L5: where the job came from (`knowledge`, `schedule`, `api`, ...). Absent on older gateways. */
  origin?: string | null
  /** WS15.L5: the job's current phase (`discover`, `fetch`, `ingest`, ...). Absent on older gateways. */
  phase?: string | null
  /** WS15.L5: what the user asked for (`run`, `paused`, `cancelled`). Differs from `status` while a request is in flight. */
  desiredState?: string | null
}

export type ScheduleItem = {
  id: string
  name: string
  kind: string
  status: string
  cron?: string | null
  scheduleAt?: string | null
  createdAt: string
  lastRunAt?: string | null
  nextRunAt?: string | null
  target: string
  config: Record<string, unknown>
}

export type EvidenceTimeline = {
  runId: string
  timeline: Array<{
    run_id: string
    kind: string
    stage: string
    status: string
    seq: number
    completed: number
    total?: number | null
    discovered: number
    queued: number
    retries: number
    blocks: number
    timestamp: string
    payload?: Record<string, unknown>
  }>
  warnings: Array<{ id: string; stage: string; summary: string }>
}

export type ManualEvidence =
  | {
      kind: 'scrape'
      targetUrl: string
      extractedAt: string
      fingerprint: string
      statusCode: number
      metadata: Record<string, unknown>
      driver: Record<string, unknown>
      formats: Record<string, unknown>
      sourceTrace: Record<string, unknown> | null
    }
  | {
      kind: 'extract'
      targetUrl: string
      extractedAt: string
      result: unknown
      results: unknown[]
    }

export type SourcePayload = {
  graph?: { available?: boolean; nodeCount?: number; edgeCount?: number }
  integrations: Array<{
    id: string
    title: string
    provider: string
    status: string
    detail: string
    counts: Record<string, number>
    capabilities: string[]
    samples: Array<{ kind: string; label: string }>
    readOnly: boolean
  }>
  quarrySources: Array<{
    id: string
    name: string
    url: string
    kind: string
    status: string
    createdAt: string
    updatedAt: string
    config: Record<string, unknown>
  }>
}

export type QuarrySource = SourcePayload['quarrySources'][number]

export type SourceCreateInput = {
  name: string
  url: string
  kind: string
  monitor?: boolean
  preset?: string
  config?: Record<string, unknown>
}

export type SourceCreateResult = {
  source: QuarrySource
}

export type ProfilePayload = {
  profiles: Array<{
    id: string
    restorable: boolean
    cookies: number
    storage: number
    locale?: string | null
    timezone?: string | null
  }>
}

export type RunCreateResult = {
  run: {
    id: string
    kind: string
    status: string
    createdAt: string
    target: string
  }
  evidence?: ManualEvidence
}

export type RunCreateRequest =
  | {
       kind: 'batch'
       urls: string[]
     }
  | {
       kind: string
       url: string
       prompt?: string
       schema?: Record<string, unknown> | string
       extractMode?: 'product' | 'article' | 'general'
     }

/**
 * `activate` is deliberately not a member: a schedule is created inactive and
 * activation is a separate, approval-gated step. The registry input schema is
 * strict, so a caller cannot smuggle one in either.
 */
export type ScheduleCreateRequest = {
  name: string
  kind: string
  /** Required for every kind except `batch`. */
  targetUrl?: string
  /** Required for `batch`. */
  urls?: string[]
  maxPages?: number
  overlapPolicy?: 'skip' | 'cancel' | 'allow'
  /** One of `cron` or `scheduleAt` is required. */
  cron?: string
  scheduleAt?: string
}

export function listIngestionRuns(signal?: AbortSignal) {
  return requestJson<RunItem[]>('/api/ingestions/runs', { signal })
}

export function listIngestionSchedules(signal?: AbortSignal) {
  return requestJson<ScheduleItem[]>('/api/ingestions/schedules', { signal })
}

/** What this installation can honour on the Schedules surface (WS15.S0). */
export type ScheduleCapabilities = {
  trigger: boolean
  backfill: boolean
  overlap: boolean
  /** Schedule kinds that have a backend workflow here. Empty means none, so Save stays disabled. */
  kinds: string[]
  needsActivation: boolean
  /** WS15.L5: the edge honours pause/resume/cancel of a running job. Off hides the job-row controls. */
  jobControl: boolean
}

/** Every capability off: what the page shows when the capability read fails. */
export const NO_SCHEDULE_CAPABILITIES: ScheduleCapabilities = {
  trigger: false,
  backfill: false,
  overlap: false,
  kinds: [],
  needsActivation: false,
  jobControl: false,
}

/** Reads GET /api/ingestions/schedules/capabilities. Any failure yields the all-off default. */
export async function getScheduleCapabilities(signal?: AbortSignal): Promise<ScheduleCapabilities> {
  try {
    const data: Partial<ScheduleCapabilities> =
      (await requestJson<Partial<ScheduleCapabilities>>('/api/ingestions/schedules/capabilities', { signal })) ?? {}
    return {
      trigger: data.trigger === true,
      backfill: data.backfill === true,
      overlap: data.overlap === true,
      kinds: Array.isArray(data.kinds) ? data.kinds.filter((kind): kind is string => typeof kind === 'string') : [],
      needsActivation: data.needsActivation === true,
      jobControl: data.jobControl === true,
    }
  } catch {
    return { ...NO_SCHEDULE_CAPABILITIES, kinds: [] }
  }
}

export function listIngestionSources(signal?: AbortSignal) {
  return requestJson<SourcePayload>('/api/ingestions/sources', { signal })
}

export function listIngestionProfiles(signal?: AbortSignal) {
  return requestJson<ProfilePayload>('/api/ingestions/profiles', { signal })
}

export function getIngestionEvidence(runId: string, signal?: AbortSignal) {
  return requestJson<EvidenceTimeline>(`/api/ingestions/evidence?runId=${encodeURIComponent(runId)}`, { signal })
}

export function createIngestionRun(body: RunCreateRequest, signal?: AbortSignal) {
  return requestJson<RunCreateResult>('/api/ingestions/runs', {
    method: 'POST',
    body: JSON.stringify(body),
    signal,
  })
}

export function createIngestionSchedule(body: ScheduleCreateRequest, signal?: AbortSignal) {
  return requestJson<ScheduleItem>('/api/ingestions/schedules', {
    method: 'POST',
    body: JSON.stringify(body),
    signal,
  })
}

/** The schedule verbs that need no approval (ingestions.pause_schedule). */
export type SchedulePauseAction = 'pause_schedule'

/** The schedule verbs behind ingestions.change_schedule_state, which is approval-gated. */
export type ScheduleStateAction = 'unpause_schedule' | 'trigger_schedule' | 'backfill_schedule' | 'delete_schedule'

export type ScheduleActionOptions = { startAt?: string; endAt?: string }

export function runIngestionScheduleAction(
  action: SchedulePauseAction | ScheduleStateAction,
  scheduleId: string,
  signal?: AbortSignal,
  options?: ScheduleActionOptions,
) {
  return requestJson('/api/ingestions/actions', {
    method: 'POST',
    body: JSON.stringify({ action, scheduleId, ...(options?.startAt ? { startAt: options.startAt } : {}), ...(options?.endAt ? { endAt: options.endAt } : {}) }),
    signal,
  })
}

// ── Job control (WS15.L5) ────────────────────────────────────────────────────

/** The job family a control call addresses. Crawl is the only one the gateway proxies today. */
export type RunControlKind = 'crawl'

/** What the edge answers to a status read or a control request. */
export type RunControlStatus = {
  id: string
  kind: string
  status: string
  phase?: string | null
  /** `run`, `paused` or `cancelled`: the state the user asked for. */
  desiredState?: string | null
  progress?: RunItem['progress']
}

function runControlPath(runId: string, kind: RunControlKind, verb?: 'pause' | 'resume') {
  const base = `/api/v1/knowledge/${kind}/${encodeURIComponent(runId)}`
  return verb ? `${base}/${verb}` : base
}

/** ingestions.get_run_status */
export function getIngestionRunStatus(runId: string, kind: RunControlKind = 'crawl', signal?: AbortSignal) {
  return requestJson<RunControlStatus>(runControlPath(runId, kind), { signal })
}

/** ingestions.pause_run */
export function pauseIngestionRun(runId: string, kind: RunControlKind = 'crawl', signal?: AbortSignal) {
  return requestJson<RunControlStatus>(runControlPath(runId, kind, 'pause'), { method: 'POST', signal })
}

/** ingestions.resume_run */
export function resumeIngestionRun(runId: string, kind: RunControlKind = 'crawl', signal?: AbortSignal) {
  return requestJson<RunControlStatus>(runControlPath(runId, kind, 'resume'), { method: 'POST', signal })
}

/** ingestions.cancel_run. Irreversible: the caller confirms first. */
export function cancelIngestionRun(runId: string, kind: RunControlKind = 'crawl', signal?: AbortSignal) {
  return requestJson<RunControlStatus>(runControlPath(runId, kind), { method: 'DELETE', signal })
}

export function createIngestionSource(input: SourceCreateInput, signal?: AbortSignal) {
  return requestJson<SourceCreateResult>('/api/ingestions/sources', {
    method: 'POST',
    body: JSON.stringify(input),
    signal,
  })
}

export function deleteIngestionSource(id: string, signal?: AbortSignal) {
  return requestJson<{ deleted: boolean; sourceId: string }>(
    `/api/ingestions/sources/${encodeURIComponent(id)}`,
    { method: 'DELETE', signal },
  )
}
