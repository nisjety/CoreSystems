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
  /** Credits the job has used (INNH-9). Absent while Quarry's job-credits flag is off. */
  credits?: JobCredits
  /** Who started the job: `{ type, actor?, ref? }` as Quarry records it (`manual`, `schedule`, `api`, …). */
  origin?: { type: string; actor?: string | null; ref?: string | null } | null
  /** The schedule that fired this job, when it was not started by hand. */
  scheduleId?: string | null
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

/** Quarry's per-job credit figure (INNH-9): pages fetched times the page price. A Quarry estimate, not an invoice. */
export type JobCredits = {
  used: number
  unit: string
  basis: string
  pricePerPage: number
  estimated: boolean
  /** False while the job can still fetch pages; a late page event can still raise `used`. */
  final: boolean
}

export type RunCreateRequest =
  | {
       kind: 'batch'
       urls: string[]
       /** Re-fetch each URL and persist its page to Knowledge. Absent, the pages are not persisted. */
       ingest?: boolean
     }
  | {
       kind: 'extract'
       url: string
       /** JSON Schema for structured extraction. */
       schema?: Record<string, unknown> | string
       /** Instructions for the model (1 to 10,000 characters). Alone, one free-form JSON object; with a schema, it steers the extraction within it. */
       prompt?: string
     }
  | {
       kind: string
       url: string
     }

/** One job's status as Quarry reports it (`GET /api/ingestions/jobs/{id}`). */
export type JobDetail = {
  id: string
  kind: string
  status: string
  phase: string
  terminal: boolean
  /** Present only while Quarry's job-credits flag is on. */
  credits?: JobCredits
  origin: RunItem['origin']
  createdAt: string
  finishedAt: string | null
  deadlineAt: string | null
  counts: { completed: number; failed: number; blocked: number; retried: number }
  total: number | null
  /** `exact` for a batch, `budget` for a crawl whose total is a page ceiling. */
  totalKind: string | null
  warnings: Array<{ code: string; message: string }>
  control: { desired: string | null; pending: boolean }
  scheduleId: string | null
  runId: string | null
  /** Crawl and batch jobs can be paused, resumed and cancelled. */
  controllable: boolean
}

/** Failed and blocked pages of a job (`GET /api/ingestions/jobs/{id}/errors`). */
export type JobErrors = {
  errors: Array<{
    id: string
    ts: string
    url: string | null
    code: string | null
    category: string | null
    retryable: boolean
    status: number | null
  }>
  blocked: Array<{ url: string | null; ts: string; reason: string | null }>
  robotsBlocked: Array<{ url: string | null; ts: string; reason: string | null }>
  nextSeq: number | null
}

/** Team credit use for a period (`GET /api/ingestions/usage`). */
export type CreditUsage = {
  period: string | null
  creditsUsed: number
  creditsLimit: number | null
  utilizationPercent: number
}

/** One structured or markdown result of `/v1/extract`, as the gateway hands it over. */
export type ExtractItem = {
  url: string
  status: 'ok' | 'fetched' | 'error' | string
  data?: unknown
  markdown?: string | null
  structured?: unknown
  error?: string | null
  /** Present when a schema was sent and the model answered. `status` stays `ok` either way. */
  schema_valid?: boolean
  /** Why `data` does not conform to the schema, one line per issue. */
  schema_issues?: string[]
  /** True when the page was longer than the input cap and only its start was read. */
  truncated_input?: boolean
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

/**
 * The optional long-poll of {@link getIngestionJob}. `wait` is whole seconds from 1 to 30. `resume` is
 * the `resume_token` of an earlier 408: 1 to 512 characters of letters, digits, `_`, `-` and `.`.
 */
export interface JobWaitOptions {
  wait?: number
  resume?: string
}

/**
 * `GET /api/ingestions/jobs/{id}`. With `options.wait` the gateway holds the read until the job ends or
 * the wait runs out. A wait that runs out answers 408 `TIMEOUT` with `details.reason` set to
 * `processing_continues` and `details.resume_token`; send that token back as `options.resume` to keep
 * waiting. Values outside those limits are refused before any request is sent.
 */
export async function getIngestionJob(
  id: string,
  signal?: AbortSignal,
  options: JobWaitOptions = {},
) {
  const query = new URLSearchParams()
  if (options.wait !== undefined) {
    if (!Number.isInteger(options.wait) || options.wait < 1 || options.wait > 30) {
      throw new RangeError('wait must be a whole number of seconds from 1 to 30.')
    }
    query.set('wait', String(options.wait))
  }
  if (options.resume !== undefined) {
    if (!/^[A-Za-z0-9_.-]{1,512}$/.test(options.resume)) {
      throw new RangeError('resume must be a resume token of 1 to 512 characters.')
    }
    query.set('resume', options.resume)
  }
  const suffix = query.size > 0 ? `?${query.toString()}` : ''
  return requestJson<JobDetail>(`/api/ingestions/jobs/${encodeURIComponent(id)}${suffix}`, { signal })
}

export function getIngestionJobErrors(
  id: string,
  options: { afterSeq?: number; limit?: number } = {},
  signal?: AbortSignal,
) {
  const query = new URLSearchParams()
  if (options.afterSeq !== undefined) query.set('afterSeq', String(options.afterSeq))
  if (options.limit !== undefined) query.set('limit', String(options.limit))
  const suffix = query.size > 0 ? `?${query.toString()}` : ''
  return requestJson<JobErrors>(`/api/ingestions/jobs/${encodeURIComponent(id)}/errors${suffix}`, { signal })
}

export function getIngestionUsage(period: string, signal?: AbortSignal) {
  return requestJson<CreditUsage>(`/api/ingestions/usage?period=${encodeURIComponent(period)}`, { signal })
}

/** One page a run fetched (the Apify dataset row). Optional fields are null when Quarry did not record them. */
export interface JobResultItem {
  seq: number
  url: string
  status: number | null
  contentType: string | null
  title: string | null
  fetchedAt: string | null
  wordCount: number | null
  driver: string | null
  markdownArtifactId: string | null
  sourceArtifactId: string | null
  bytes: number | null
}

export interface JobResultsPage {
  /** What the rows are: `page_index` is an index of the pages fetched, not their text. */
  kind: string
  items: JobResultItem[]
  /** The cursor for the next page; null on the last page. */
  nextSeq: number | null
  /** True when `latest` stopped scanning early on a very large run. */
  scanCapped: boolean
}

/** The pages a crawl, batch or other job fetched. Quarry answers 501 while `QUARRY_EDGE__JOB_RESULTS` is off and 403 under ZDR. */
export function getIngestionJobResults(
  id: string,
  options: { afterSeq?: number; limit?: number; latest?: boolean } = {},
  signal?: AbortSignal,
) {
  const query = new URLSearchParams()
  if (options.afterSeq !== undefined) query.set('afterSeq', String(options.afterSeq))
  if (options.limit !== undefined) query.set('limit', String(options.limit))
  if (options.latest) query.set('latest', 'true')
  const suffix = query.size > 0 ? `?${query.toString()}` : ''
  return requestJson<JobResultsPage>(`/api/ingestions/jobs/${encodeURIComponent(id)}/results${suffix}`, { signal })
}

/** The row fields a results export may name, as Quarry writes them. */
export type JobResultExportField =
  | 'seq'
  | 'url'
  | 'status'
  | 'content_type'
  | 'title'
  | 'fetched_at'
  | 'word_count'
  | 'driver'
  | 'markdown_artifact_id'
  | 'source_artifact_id'
  | 'bytes'

/** One page of a job's results as a CSV or JSON Lines file, ready to save. */
export interface JobResultsExport {
  filename: string
  contentType: string
  content: string
  /** The cursor for the next export page; null on the last page. */
  nextSeq: number | null
  kind: string
  scanCapped: boolean
}

/**
 * One page of a job's results as CSV or JSON Lines (at most 100 rows). `fields` picks and orders the
 * columns (default: all). Quarry answers 501 while `QUARRY_EDGE__JOB_RESULTS` is off and 403 under ZDR.
 */
export function exportIngestionJobResults(
  id: string,
  options: { format: 'csv' | 'jsonl'; fields?: JobResultExportField[]; afterSeq?: number; latest?: boolean },
  signal?: AbortSignal,
) {
  const query = new URLSearchParams({ format: options.format })
  if (options.fields && options.fields.length > 0) query.set('fields', options.fields.join(','))
  if (options.afterSeq !== undefined) query.set('afterSeq', String(options.afterSeq))
  if (options.latest) query.set('latest', 'true')
  return requestJson<JobResultsExport>(
    `/api/ingestions/jobs/${encodeURIComponent(id)}/results/export?${query.toString()}`,
    { signal },
  )
}

/** A text artifact, such as a fetched page's markdown, capped at 1 MiB by the gateway. */
export interface ArtifactText {
  id: string
  contentType: string
  content: string
  bytes: number
  truncated: boolean
}

export function getIngestionArtifactText(artifactId: string, signal?: AbortSignal) {
  return requestJson<ArtifactText>(`/api/ingestions/artifacts/${encodeURIComponent(artifactId)}`, { signal })
}
