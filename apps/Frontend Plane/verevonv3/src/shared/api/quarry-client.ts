/**
 * Typed Quarry-v2 API client.
 *
 * Wraps the generated `@quarry/client` SDK (openapi-generator typescript-fetch
 * output of `apps/Ingestion Plane/Quarry-v2/docs/openapi.yaml`) so the SPA gets
 * compile-time-safe access to the resource / change-tracking / schedule /
 * team surfaces without hand-maintained duplicate types.
 *
 * Routing: every call goes through the same-origin gateway (`/api/quarry`)
 * exactly like `ingestions-client.ts` — the browser never talks to quarry-edge
 * directly, keeping the session-cookie-first posture. The generated runtime is
 * configured with `basePath + fetchApi + credentials`, so dev-proxy and prod
 * reverse-proxy behave identically.
 *
 * Field naming: the SDK emits camelCase (generator convention); Quarry's wire
 * uses snake_case, and the generated FromJSON mappers translate. Never reach
 * into raw JSON for these — use the decoded interfaces re-exported here.
 */
import {
  ChangeTrackingApi,
  ResourcesApi,
  SchedulesApi,
  TeamApi,
  Configuration,
  type ListJobsByKindKindEnum,
} from '@quarry/client'
import { gatewayBaseUrl } from './config'

/** Base path the gateway exposes for proxied Quarry-v2 traffic. */
export const QUARRY_API_BASE = '/api/quarry'

const quarryConfig = new Configuration({
  basePath: QUARRY_API_BASE,
  credentials: 'include',
  fetchApi: (input, init) =>
    fetch(`${gatewayBaseUrl()}${input instanceof Request ? input.url : String(input)}`, init),
})

const changeTracking = new ChangeTrackingApi(quarryConfig)
const resources = new ResourcesApi(quarryConfig)
const schedules = new SchedulesApi(quarryConfig)
const team = new TeamApi(quarryConfig)

// ── Decoded wire shapes (re-exported from the generated models) ────────────

export type {
  BaselineSnapshot,
  ChangeRecord,
  JobHistoryEvent,
  JobSummary,
  RequestQueueSummary,
  ScheduleRefreshRun200ResponseData,
  ScheduleSummary,
  Snapshot,
  Source,
  TeamActivityEntry,
  TeamConcurrency,
  TeamQueueStatus,
} from '@quarry/client'
export type { ListJobsByKindKindEnum } from '@quarry/client'

// ── Change tracking ─────────────────────────────────────────────────────────

/** Re-fetch a tracked URL and compare against its durable baseline. */
export function checkChange(url: string, signal?: AbortSignal) {
  return changeTracking.changeCheck({ changeCheckRequest: { url } }, { signal })
    .then((r) => r.data)
}

/** Most recent baseline for a tracked URL. */
export function getChangeLatest(url: string, signal?: AbortSignal) {
  return changeTracking.changeLatest({ url }, { signal }).then((r) => r.data)
}

/** Ordered baseline chain (most recent first). */
export function getChangeHistory(url: string, limit?: number, signal?: AbortSignal) {
  return changeTracking.changeHistory({ url, limit }, { signal }).then((r) => r.data)
}

/** Promote the latest baseline into the public Snapshot shape. */
export function promoteChangeToSnapshot(url: string, signal?: AbortSignal) {
  return changeTracking.promoteTrackedResultToSnapshot({ url }, { signal })
    .then((r) => r.data)
}

/**
 * Enqueue an immediate re-check onto the org-scoped durable frontier
 * (idempotent enqueue — `accepted: false` when already queued).
 */
export function scheduleRefreshRun(url: string, priority?: string | null, signal?: AbortSignal) {
  return changeTracking.scheduleRefreshRun(
    { scheduleRefreshRunRequest: { url, priority: priority ?? undefined } },
    { signal },
  ).then((r) => r.data)
}

// ── Resources ───────────────────────────────────────────────────────────────

export function listQuarrySources(limit?: number, cursor?: string, signal?: AbortSignal) {
  return resources.listSources({ limit, cursor }, { signal }).then((r) => r.data)
}

export function listQuarrySnapshots(
  params: { limit?: number; cursor?: string; url?: string } = {},
  signal?: AbortSignal,
) {
  return resources.listSnapshots(params, { signal }).then((r) => r.data)
}

export function listQuarryJobsByKind(
  kind: ListJobsByKindKindEnum,
  orgId: string,
  params: { limit?: number; cursor?: string } = {},
  signal?: AbortSignal,
) {
  return resources.listJobsByKind({ kind, orgId, ...params }, { signal }).then((r) => r.data)
}

export function listQuarryRunEvents(
  runId: string,
  params: { afterSeq?: number; limit?: number } = {},
  signal?: AbortSignal,
) {
  return resources.listRunEvents({ runId, ...params }, { signal }).then((r) => r.data)
}

export function listQuarryRequestQueues(limit?: number, cursor?: string, signal?: AbortSignal) {
  return resources.listRequestQueues({ limit, cursor }, { signal }).then((r) => r.data)
}

// ── Schedules ───────────────────────────────────────────────────────────────

export function listQuarrySchedules(limit?: number, cursor?: string, signal?: AbortSignal) {
  return schedules.listSchedules({ limit, cursor }, { signal }).then((r) => r.data)
}

export function pauseQuarrySchedule(scheduleId: string, signal?: AbortSignal) {
  return schedules.pauseSchedule({ scheduleId }, { signal })
}

export function unpauseQuarrySchedule(scheduleId: string, signal?: AbortSignal) {
  return schedules.unpauseSchedule({ scheduleId }, { signal })
}

/**
 * Trigger an immediate scheduled run. NOTE: control-plane stub today — returns
 * 202 `trigger-accepted` until the Temporal SDK swap lands.
 */
export function triggerQuarrySchedule(scheduleId: string, signal?: AbortSignal) {
  return schedules.triggerSchedule({ scheduleId }, { signal })
}

// ── Team aggregates ─────────────────────────────────────────────────────────

export function getTeamCreditUsage(period?: string, signal?: AbortSignal) {
  return team.teamCreditUsage({ period }, { signal }).then((r) => r.data)
}

export function getTeamTokenUsage(period?: string, signal?: AbortSignal) {
  return team.teamTokenUsage({ period }, { signal }).then((r) => r.data)
}

export function getTeamConcurrency(signal?: AbortSignal) {
  return team.teamConcurrency({ signal }).then((r) => r.data)
}

export function getTeamQueueStatus(signal?: AbortSignal) {
  return team.teamQueueStatus({ signal }).then((r) => r.data)
}

export function getTeamActivity(signal?: AbortSignal) {
  return team.teamActivity({ signal }).then((r) => r.data)
}
