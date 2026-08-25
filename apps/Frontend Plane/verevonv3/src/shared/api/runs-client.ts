import { requestJson } from './http'
import { normalizePrivacyTier, type PrivacyTier } from './privacy-tier'

// Runs-history read model for the Agent Run Console. These call the gateway
// `/api/v1/agents/runs[/:run_id]` proxy (→ model-gateway → session-core
// RunService). The live `step_update`/`*_transitioned` stream comes from
// `run-console-client`; these GETs hydrate the history rail and a selected
// run's telemetry. Normalized to camelCase, parse-defensively (snake_case OR
// camelCase) like the rest of the API layer (see `orchestration-client`).

/** Full run metadata as reported by session-core's RunService (via the gateway). */
export type RunDetail = {
  runId: string
  threadId?: string
  parentRunId?: string
  agentId?: string
  /** queued | running | completed | failed | cancelled | awaiting_approval. */
  status: string
  /** execute | plan | reactive. */
  mode?: string
  goal: string
  finalOutput?: string
  error?: string
  checkpointIndex: number
  stepsCompleted: number
  inputTokens: number
  outputTokens: number
  /** Data-residency region stamped on the run (from RunDetail.metadata.residency). */
  residency?: string
  /**
   * Venice-style privacy tier stamped on the run alongside `residency`
   * (run provenance). `undefined` when absent/unknown — the console then
   * renders no claim at all rather than guessing one.
   */
  privacyTier?: PrivacyTier
  /** RFC3339 timestamps, when present. */
  createdAt?: string
  updatedAt?: string
}

export type RunsPage = {
  runs: RunDetail[]
  hasMore: boolean
}

type RawRun = Record<string, unknown>

function str(value: unknown): string | undefined {
  return typeof value === 'string' && value.length > 0 ? value : undefined
}

/** Coerce a wire number (or numeric string) to a non-negative integer, defaulting to 0. */
function num(value: unknown): number {
  if (typeof value === 'number' && Number.isFinite(value)) return value
  if (typeof value === 'string') {
    const parsed = Number(value)
    if (Number.isFinite(parsed)) return parsed
  }
  return 0
}

/** Read a residency hint from the run's arbitrary metadata object, if present. */
function residencyFrom(raw: RawRun): string | undefined {
  const meta = raw.metadata
  if (meta && typeof meta === 'object') {
    const record = meta as Record<string, unknown>
    return str(record.residency) ?? str(record.region)
  }
  return undefined
}

/** Read a privacy tier from the run's arbitrary metadata object, if present. */
function privacyTierFrom(raw: RawRun): unknown {
  const meta = raw.metadata
  if (meta && typeof meta === 'object') {
    return (meta as Record<string, unknown>).privacy_tier ?? (meta as Record<string, unknown>).privacyTier
  }
  return undefined
}

/** Normalize a model-gateway run object (snake_case) to our camelCase shape. */
function normalizeRun(raw: RawRun): RunDetail | null {
  const runId = str(raw.run_id) ?? str(raw.runId) ?? str(raw.id)
  if (!runId) return null
  return {
    runId,
    threadId: str(raw.thread_id) ?? str(raw.threadId),
    parentRunId: str(raw.parent_run_id) ?? str(raw.parentRunId),
    agentId: str(raw.agent_id) ?? str(raw.agentId),
    status: str(raw.status) ?? 'queued',
    mode: str(raw.mode),
    goal: str(raw.goal) ?? '',
    finalOutput: str(raw.final_output) ?? str(raw.finalOutput),
    error: str(raw.error),
    checkpointIndex: num(raw.checkpoint_index ?? raw.checkpointIndex),
    stepsCompleted: num(raw.steps_completed ?? raw.stepsCompleted),
    inputTokens: num(raw.input_tokens ?? raw.inputTokens),
    outputTokens: num(raw.output_tokens ?? raw.outputTokens),
    residency: str(raw.residency) ?? residencyFrom(raw),
    // Unknown/garbage tier values degrade to undefined → neutral console UI,
    // never a fabricated claim.
    privacyTier: normalizePrivacyTier(raw.privacy_tier ?? raw.privacyTier ?? privacyTierFrom(raw)),
    createdAt: str(raw.created_at) ?? str(raw.createdAt),
    updatedAt: str(raw.updated_at) ?? str(raw.updatedAt),
  }
}

export type ListRunsParams = {
  threadId: string
  status?: string
  /** ULID cursor — return runs strictly older than this id. */
  after?: string
  limit?: number
}

/** List a thread's runs, newest-first, for the history rail. */
export async function listRuns(params: ListRunsParams, signal?: AbortSignal): Promise<RunsPage> {
  const search = new URLSearchParams({ thread_id: params.threadId })
  if (params.status && params.status.trim()) search.set('status', params.status)
  if (params.after && params.after.trim()) search.set('after', params.after)
  if (params.limit != null) search.set('limit', String(params.limit))

  const payload = await requestJson<{ runs?: RawRun[]; has_more?: boolean }>(
    `/api/v1/agents/runs?${search.toString()}`,
    { signal },
  )
  const list = Array.isArray(payload.runs) ? payload.runs : []
  return {
    runs: list.map(normalizeRun).filter((r): r is RunDetail => r !== null),
    hasMore: payload.has_more === true,
  }
}

export type ListSystemRunsParams = {
  status?: string
  /** ULID cursor — return runs strictly older than this id. */
  after?: string
  limit?: number
}

/**
 * List the org's runs that have no human owner, newest-first.
 *
 * Deliberately takes NO threadId. A cron-fired workflow owns its run and the
 * thread it lives in, so `listRuns` — which is thread-scoped — can never reach
 * one: the run exists, is fully authorized, and is invisible. This is the only
 * way to see them.
 *
 * Read-only. System runs are org-readable but mutable solely by the workload that
 * owns them, so the console must not offer resume/cancel for these.
 */
export async function listSystemRuns(
  params: ListSystemRunsParams = {},
  signal?: AbortSignal,
): Promise<RunsPage> {
  const search = new URLSearchParams()
  if (params.status && params.status.trim()) search.set('status', params.status)
  if (params.after && params.after.trim()) search.set('after', params.after)
  if (params.limit != null) search.set('limit', String(params.limit))
  const query = search.toString()

  const payload = await requestJson<{ runs?: RawRun[]; has_more?: boolean }>(
    query ? `/api/v1/agents/runs/system?${query}` : '/api/v1/agents/runs/system',
    { signal },
  )
  const list = Array.isArray(payload.runs) ? payload.runs : []
  return {
    runs: list.map(normalizeRun).filter((r): r is RunDetail => r !== null),
    hasMore: payload.has_more === true,
  }
}

/** Fetch one run's full detail for the telemetry panel. */
export async function getRun(runId: string, signal?: AbortSignal): Promise<RunDetail | null> {
  const payload = await requestJson<{ run?: RawRun }>(
    `/api/v1/agents/runs/${encodeURIComponent(runId)}`,
    { signal },
  )
  return payload.run ? normalizeRun(payload.run) : null
}

// ── Run watchers ("notify me when this run finishes") ───────────────────────
// These call the gateway `/api/v1/runs/:run_id/watchers` proxy (→
// model-gateway → capability-core), which owns the durable per-user watcher
// record and hands off to notification-core once the run finishes. Distinct
// from the reads above: a watcher is a standing subscription the caller
// registers, not a read of the run's own state.

export type RunWatchStatus = {
  watching: boolean
}

function normalizeWatchStatus(payload: { watching?: unknown }): RunWatchStatus {
  return { watching: payload.watching === true }
}

/** Register a "notify me" subscription: tell the caller when this run finishes. */
export async function watchRun(runId: string, signal?: AbortSignal): Promise<RunWatchStatus> {
  const payload = await requestJson<{ watching?: unknown }>(
    `/api/v1/runs/${encodeURIComponent(runId)}/watchers`,
    { method: 'POST', signal },
  )
  return normalizeWatchStatus(payload)
}

/** Cancel a previously-registered "notify me" subscription for this run. */
export async function unwatchRun(runId: string, signal?: AbortSignal): Promise<void> {
  await requestJson<unknown>(`/api/v1/runs/${encodeURIComponent(runId)}/watchers`, {
    method: 'DELETE',
    signal,
  })
}

/** Check whether the caller currently has a "notify me" subscription on this run. */
export async function getRunWatchStatus(runId: string, signal?: AbortSignal): Promise<RunWatchStatus> {
  const payload = await requestJson<{ watching?: unknown }>(
    `/api/v1/runs/${encodeURIComponent(runId)}/watchers`,
    { signal },
  )
  return normalizeWatchStatus(payload)
}
