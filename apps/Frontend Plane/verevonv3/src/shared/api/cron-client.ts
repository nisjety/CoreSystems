import { requestJson } from './http'

/**
 * What a schedule fires. Stored as the schedule's `task_template` and carried
 * by the sweeper into the created task's `config_json`, which is where
 * capability-core's WorkflowDispatcher reads `workflow_type` / `workflow_input`
 * to pick the workflow.
 *
 * `workflow_type` must be one of orchestrator-core's allowlisted types
 * (`workflowAllowlist` in internal/orchestration/workflowreg.go). Omitting it
 * falls back to InteractiveRunSupervision.
 *
 * `workflow_input` matters more than it looks: when it is absent the dispatcher
 * derives `{goal, policy}` from the task's title/description, and every
 * workflow EXCEPT InteractiveRunSupervision rejects that shape — their input
 * decoders use DisallowUnknownFields and none of them declare a `goal` field.
 * So for any other type an explicit `workflow_input` matching that workflow's
 * own contract is required, not optional.
 */
export interface CronTaskTemplate {
  kind?: string
  title?: string
  description?: string
  assignee?: string
  priority?: number
  workflow_type?: string
  workflow_input?: Record<string, unknown>
  policy?: string
}

/**
 * A durable, Space-bound cron schedule, mirroring capability-core's
 * `cron_schedules` row (fronted by model-gateway `/v1/cron`). New schedules
 * are authorized by Control for one Space and every fire is reauthorized; the
 * client never receives the signed authority token.
 */
export interface CronSchedule {
  id: string
  org_id: string
  name: string
  description: string
  schedule_expr: string
  timezone: string
  task_template: unknown
  enabled: boolean
  last_fire_at?: string | null
  next_fire_at?: string | null
  created_at?: string
  updated_at?: string
  space_ref?: string
  creator_subject_id?: string
  /** The newest fire's status and closed outcome token, when the schedule has fired. */
  last_fire_status?: string
  last_outcome?: string
}

/** Server capabilities the schedule list reports, so the UI never offers a missing route. */
export interface CronFeatures {
  run_now?: boolean
}

export function listCronSchedules(orgId: string): Promise<{ schedules: CronSchedule[]; features?: CronFeatures }> {
  // capability-core serialises an empty Go slice as `"schedules": null`, so an
  // org with no schedules yet answered with null and every caller that did
  // `result.schedules.filter(...)` threw — the chat's "Planlagte kjøringer"
  // panel showed "Kunne ikke laste" instead of "Ingen planlagte kjøringer".
  return requestJson<{ schedules: CronSchedule[] | null; features?: CronFeatures }>('/api/v1/cron', {
    headers: { 'x-verevon-org-id': orgId },
  }).then((result) => ({ ...result, schedules: result.schedules ?? [] }))
}

/** The manual fire Run now made, or found again for a retried `requestId`. */
export interface ScheduleRunNowResult {
  fire_id: string
  fire_key: string
  task_id: string
  status: string
  deduplicated: boolean
}

/**
 * Run one of the caller's own schedules now (EP-6-S2): one manual fire under
 * the creator's existing approval, through the same Control fire check as a
 * scheduled fire. `requestId` is generated once per press and reused on retry,
 * so a retry returns the first fire instead of making a second one.
 */
export function runScheduleNow(orgId: string, cronId: string, requestId: string): Promise<ScheduleRunNowResult> {
  return requestJson<{ data: ScheduleRunNowResult }>(`/api/v1/cron/${encodeURIComponent(cronId)}/run`, {
    method: 'POST',
    body: JSON.stringify({ request_id: requestId }),
    headers: { 'x-verevon-org-id': orgId },
  }).then((result) => result.data)
}

/**
 * Create a cron schedule. capability-core validates the cron expression and
 * seeds next_fire_at, so an invalid expression returns 400. Org id is derived
 * server-side. Returns the new schedule id.
 */
export function createCronSchedule(
  orgId: string,
  body: {
    name: string
    schedule_expr: string
    timezone?: string
    description?: string
    task_template?: CronTaskTemplate
    enabled?: boolean
    /** Optional explicit Space; the BFF resolves the active Personal Space for
     * the legacy Settings form until the Work cockpit supplies a picker. */
    space_ref?: string
  },
): Promise<{ id: string }> {
  return requestJson<{ id: string }>('/api/v1/cron', {
    method: 'POST',
    body: JSON.stringify(body),
    headers: { 'x-verevon-org-id': orgId },
  })
}

/** Update a schedule's enabled flag, description, or expression. */
export function updateCronSchedule(
  orgId: string,
  cronId: string,
  body: { enabled?: boolean; description?: string; schedule_expr?: string },
): Promise<unknown> {
  return requestJson<unknown>(`/api/v1/cron/${encodeURIComponent(cronId)}`, {
    method: 'POST',
    body: JSON.stringify(body),
    headers: { 'x-verevon-org-id': orgId },
  })
}

export function deleteCronSchedule(orgId: string, cronId: string): Promise<void> {
  return requestJson<void>(`/api/v1/cron/${encodeURIComponent(cronId)}`, {
    method: 'DELETE',
    headers: { 'x-verevon-org-id': orgId },
  })
}

/**
 * One fire of a schedule. Identifiers, closed tokens and counts only: the
 * answer is read from the scheduled thread named by `thread_id`. `status` is
 * the fire's own state (`pending`, `running`, `completed`, `failed`, `unknown`);
 * `outcome` is the run's closed result token when one was recorded.
 */
export interface CronScheduleFire {
  id: string
  fired_at: string
  status: string
  outcome?: string | null
  completed_at?: string | null
  run_id?: string | null
  thread_id?: string | null
  withheld_effects: number
}

/** The most recent fires of one schedule, newest first. */
export function listScheduleFires(orgId: string, cronId: string, limit = 5): Promise<CronScheduleFire[]> {
  return requestJson<{ data: CronScheduleFire[] }>(
    `/api/v1/cron/${encodeURIComponent(cronId)}/fires?limit=${limit}`,
    { headers: { 'x-verevon-org-id': orgId } },
  ).then((result) => result.data)
}
