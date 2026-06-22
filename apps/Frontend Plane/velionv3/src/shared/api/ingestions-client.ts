import { requestJson } from './http'

export type RunItem = {
  id: string
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
    }

export type ScheduleCreateRequest = {
  name: string
  kind: string
  targetUrl: string
  cron: string
}

export function listIngestionRuns(signal?: AbortSignal) {
  return requestJson<RunItem[]>('/api/ingestions/runs', { signal })
}

export function listIngestionSchedules(signal?: AbortSignal) {
  return requestJson<ScheduleItem[]>('/api/ingestions/schedules', { signal })
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

export function runIngestionScheduleAction(action: string, scheduleId: string, signal?: AbortSignal) {
  return requestJson('/api/ingestions/actions', {
    method: 'POST',
    body: JSON.stringify({ action, scheduleId }),
    signal,
  })
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
