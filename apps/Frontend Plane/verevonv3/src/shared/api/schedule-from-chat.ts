import { createCronSchedule, listCronSchedules, type CronSchedule } from './cron-client'

/**
 * Thin wrapper over `cron-client.ts` for the chat-facing "Planlegg dette" /
 * `/schedule` surface (chat-wave2 "schedule" spec §2.1) — no new HTTP surface,
 * just a narrower shape than the admin Settings form's raw `CronTaskTemplate`.
 *
 * `workflow_type` is deliberately never set here. Of the seven workflow types
 * the admin Settings form (`CronSchedulesSection.tsx`) offers, only the
 * default (`InteractiveRunSupervision`) can ever fire successfully for a
 * scoped scheduled run — `workflow_dispatcher.go`'s `reauthorizeScheduleFire`
 * hard-rejects every other type once a schedule fire is prepared. Omitting
 * the field lets capability-core's dispatcher fall back to that default, and
 * lets the dispatcher derive `{goal, policy}` straight from `title`/
 * `description` — every other workflow's input decoder rejects that shape.
 */
export interface ScheduleFromChatInput {
  orgId: string
  /** "Navn" — short, user-editable schedule name. */
  name: string
  /** Built from a cadence preset, or a raw cron expression for the "Egendefinert" option. */
  scheduleExpr: string
  /** Default 'Europe/Oslo' (owner ruling 2026-09-28 #4) — the dialog supplies this, not this function. */
  timezone: string
  /** The exact request to rerun — the composer draft, or a resolved past user turn. */
  goalText: string
  threadOriginHint: 'composer' | 'message-action'
}

export function createScheduleFromChat(input: ScheduleFromChatInput): Promise<{ id: string }> {
  const name = input.name.trim()
  return createCronSchedule(input.orgId, {
    name,
    schedule_expr: input.scheduleExpr.trim(),
    timezone: input.timezone.trim() || 'Europe/Oslo',
    task_template: {
      kind: 'cron',
      title: name,
      // taskGoal() (capability-core) renders title + "\n\n" + description as
      // the derived goal when workflow_input is absent — see the module doc.
      description: input.goalText,
    },
    enabled: true,
    // space_ref omitted — resolves to the caller's personal Space server-side
    // (spaces.rs::inject_personal_schedule_create_context).
  })
}

/**
 * The signed-in user's own schedules, filtered client-side by
 * `creator_subject_id` — capability-core's `GET /v1/cron` already scopes to
 * the org, and there is no server-side "mine only" filter today, so this is
 * the cheapest correct filter without a new endpoint (chat-wave2 "schedule"
 * spec §2.1). Relies on `creator_subject_id` and `myUserId` being the same
 * identity space as `AuthenticatedUser.user_id` server-side — true today, but
 * unverified against every future identity-provider change (spec §5 risk).
 */
export function listMySchedules(orgId: string, myUserId: string): Promise<CronSchedule[]> {
  return listCronSchedules(orgId).then((result) =>
    result.schedules.filter((schedule) => schedule.creator_subject_id === myUserId),
  )
}

/**
 * Four Norwegian cadence presets (chat-wave2 "schedule" spec §2.1, owner
 * ruling 2026-09-28 #5), plus the always-available "Egendefinert" escape
 * hatch to a raw cron expression. Kept in one place so `ScheduleDialog`'s
 * `<select>` and `ScheduledRunsPanel`'s reverse-mapping label
 * (`cronExprToLabel`) can never drift from each other.
 */
export interface SchedulePreset {
  readonly id: string
  readonly labelNo: string
  readonly labelEn: string
  /** `null` for the "Egendefinert" preset, which reveals a raw cron-expression field instead. */
  readonly cronExpr: string | null
}

export const SCHEDULE_PRESETS: readonly SchedulePreset[] = [
  { id: 'daily-09', labelNo: 'Hver dag kl. 09:00', labelEn: 'Every day at 09:00', cronExpr: '0 9 * * *' },
  { id: 'weekday-08', labelNo: 'Hver ukedag kl. 08:00', labelEn: 'Every weekday at 08:00', cronExpr: '0 8 * * 1-5' },
  { id: 'weekly-monday-09', labelNo: 'Hver mandag kl. 09:00', labelEn: 'Every Monday at 09:00', cronExpr: '0 9 * * 1' },
  { id: 'monthly-1st-09', labelNo: 'Den 1. i hver måned kl. 09:00', labelEn: 'The 1st of every month at 09:00', cronExpr: '0 9 1 * *' },
  { id: 'custom', labelNo: 'Egendefinert (cron-uttrykk)', labelEn: 'Custom (cron expression)', cronExpr: null },
] as const

export const DEFAULT_SCHEDULE_TIMEZONE = 'Europe/Oslo'

/**
 * Reverse-maps a stored `schedule_expr` back to a preset label for display
 * (`ScheduledRunsPanel`), falling back to the raw expression for anything
 * that does not match one of the four fixed presets exactly — including a
 * schedule created via the admin Settings form's free-text field, or a
 * grammar this cron parser accepts that these four literal strings do not
 * happen to spell (spec §5: presets and capability-core's cron grammar have
 * no shared source of truth, so this deliberately never tries to be a real
 * cron parser — porting `capability-core/internal/cron/cron.go` to
 * TypeScript would be its own drift risk).
 */
export function cronExprToLabel(scheduleExpr: string, no: boolean): string {
  const trimmed = scheduleExpr.trim()
  const preset = SCHEDULE_PRESETS.find((candidate) => candidate.cronExpr === trimmed)
  if (preset) return no ? preset.labelNo : preset.labelEn
  return trimmed
}
