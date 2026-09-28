import { CalendarClock, X } from '@/shared/icons'
import { createMemo, createSignal, For, Show } from 'solid-js'

import {
  createScheduleFromChat,
  DEFAULT_SCHEDULE_TIMEZONE,
  SCHEDULE_PRESETS,
  type ScheduleFromChatInput,
} from '@/shared/api/schedule-from-chat'
import { translateApiError, useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'

/**
 * "Planlegg dette" — the chat-facing create-schedule dialog (chat-wave2
 * "schedule" spec §2.1). Opened from the message overflow menu
 * ("Planlegg dette" on an assistant turn) or the `/schedule` composer
 * command; both seed `seedTitle`/`seedGoal` from the request being turned
 * into a recurring job.
 *
 * Deliberately no workflow picker: every schedule created here always omits
 * `workflow_type`, which resolves to `InteractiveRunSupervision` — the ONLY
 * workflow type that can ever fire successfully for a scoped scheduled run
 * (`workflow_dispatcher.go`'s `reauthorizeScheduleFire` rejects every other
 * type once a fire is prepared; see `schedule-from-chat.ts`'s module doc and
 * the matching fix in `CronSchedulesSection.tsx`).
 *
 * "Kun utkast (standard)" is not a real choice here yet — draft-only is the
 * only option, per the owner ruling that unattended/scheduled runs are
 * draft-only for Wave 2 (`ScheduledRunSupervision` hard-fails the whole run
 * the instant a step needs approval; there is no pause/resume path for an
 * effectful tool call in a scheduled run today). A future "allow actions,
 * with approval" option is explicitly Wave-3 scope, not hidden here but never
 * offered as a live choice.
 */
export interface ScheduleDialogProps {
  /**
   * No internal `open`/`<Show>` gating here on purpose: every caller already
   * mounts this component behind its OWN `<Show when={someOpenSignal()}>`
   * (matching this codebase's `VoiceModeDialog`/`HistoryPanel`/
   * `SettingsPanel` convention) — a second, redundant visibility check inside
   * this component reading a signal accessor passed as a plain prop function
   * did not track reactively and left the dialog permanently invisible.
   */
  readonly onClose: () => void
  readonly seedTitle: string
  readonly seedGoal: string
  readonly threadOriginHint: 'composer' | 'message-action'
  readonly onCreated?: (id: string) => void
}

function firstPresetId(): string {
  const first = SCHEDULE_PRESETS[0]
  return first ? first.id : 'custom'
}

export function ScheduleDialog(props: ScheduleDialogProps) {
  const i18n = useI18n()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id?.trim() ?? '')

  const [name, setName] = createSignal(props.seedTitle)
  const [presetId, setPresetId] = createSignal(firstPresetId())
  const [customExpr, setCustomExpr] = createSignal('')
  const [timezoneAdvanced, setTimezoneAdvanced] = createSignal(false)
  const [timezone, setTimezone] = createSignal(DEFAULT_SCHEDULE_TIMEZONE)
  const [submitting, setSubmitting] = createSignal(false)
  const [errorMessage, setErrorMessage] = createSignal<string | undefined>(undefined)

  const selectedPreset = createMemo(
    () => SCHEDULE_PRESETS.find((preset) => preset.id === presetId()) ?? SCHEDULE_PRESETS[0],
  )
  const isCustom = createMemo(() => selectedPreset()?.cronExpr === null)
  const scheduleExpr = createMemo(() => (isCustom() ? customExpr().trim() : (selectedPreset()?.cronExpr ?? '')))

  function reset(): void {
    setName(props.seedTitle)
    setPresetId(firstPresetId())
    setCustomExpr('')
    setTimezoneAdvanced(false)
    setTimezone(DEFAULT_SCHEDULE_TIMEZONE)
    setErrorMessage(undefined)
  }

  function close(): void {
    if (submitting()) return
    reset()
    props.onClose()
  }

  async function submit(): Promise<void> {
    const id = orgId()
    const trimmedName = name().trim()
    const trimmedExpr = scheduleExpr()
    if (!id || !trimmedName || !trimmedExpr || submitting()) {
      if (!trimmedName) {
        setErrorMessage(i18n.tr('Navn er påkrevd.', 'Name is required.'))
      } else if (!trimmedExpr) {
        setErrorMessage(i18n.tr('Cron-uttrykk er påkrevd.', 'A cron expression is required.'))
      }
      return
    }
    setSubmitting(true)
    setErrorMessage(undefined)
    try {
      const input: ScheduleFromChatInput = {
        orgId: id,
        name: trimmedName,
        scheduleExpr: trimmedExpr,
        timezone: timezone().trim() || DEFAULT_SCHEDULE_TIMEZONE,
        goalText: props.seedGoal,
        threadOriginHint: props.threadOriginHint,
      }
      const { id: scheduleId } = await createScheduleFromChat(input)
      reset()
      props.onCreated?.(scheduleId)
      props.onClose()
    } catch (err) {
      setErrorMessage(
        translateApiError(err, i18n.tr, {
          no: 'Kunne ikke opprette planen.',
          en: 'Could not create the schedule.',
        }),
      )
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div
      class="verevon-schedule-dialog"
      role="presentation"
      onClick={(event) => {
        if (event.target === event.currentTarget) close()
      }}
    >
      <section
        class="verevon-schedule-dialog__dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="schedule-dialog-title"
      >
        <header>
          <h3 id="schedule-dialog-title">
            <CalendarClock size={16} aria-hidden="true" /> {i18n.tr('Planlegg denne forespørselen', 'Schedule this request')}
          </h3>
          <button type="button" onClick={close} aria-label={i18n.tr('Lukk', 'Close')}>
            <X size={16} />
          </button>
        </header>

        <label class="verevon-schedule-dialog__field">
          <span>{i18n.tr('Navn', 'Name')}</span>
          <input
            value={name()}
            maxlength={120}
            placeholder={i18n.tr('f.eks. Daglig oppsummering', 'e.g. Daily summary')}
            onInput={(event) => setName(event.currentTarget.value)}
            disabled={submitting()}
          />
        </label>

        <label class="verevon-schedule-dialog__field">
          <span>{i18n.tr('Gjentakelse', 'Recurrence')}</span>
          <select
            value={presetId()}
            onChange={(event) => setPresetId(event.currentTarget.value)}
            disabled={submitting()}
          >
            <For each={SCHEDULE_PRESETS}>
              {(preset) => <option value={preset.id}>{i18n.tr(preset.labelNo, preset.labelEn)}</option>}
            </For>
          </select>
        </label>

        <Show when={isCustom()}>
          <label class="verevon-schedule-dialog__field">
            <span>{i18n.tr('Cron-uttrykk', 'Cron expression')}</span>
            <input
              value={customExpr()}
              placeholder={i18n.tr('min time dag måned ukedag — f.eks. 0 9 * * 1', 'min hour day month weekday — e.g. 0 9 * * 1')}
              onInput={(event) => setCustomExpr(event.currentTarget.value)}
              disabled={submitting()}
            />
            <small>{i18n.tr('5 felt. Eksempel: «0 9 * * 1» = hver mandag kl. 09:00.', '5 fields. Example: "0 9 * * 1" = every Monday at 09:00.')}</small>
          </label>
        </Show>

        <div class="verevon-schedule-dialog__field">
          <span>{i18n.tr('Tidssone', 'Timezone')}</span>
          <Show
            when={timezoneAdvanced()}
            fallback={
              <div class="verevon-schedule-dialog__timezone-fixed">
                <span>{timezone()}</span>
                <button type="button" onClick={() => setTimezoneAdvanced(true)} disabled={submitting()}>
                  {i18n.tr('Endre', 'Change')}
                </button>
              </div>
            }
          >
            <input
              value={timezone()}
              placeholder={i18n.tr('Europe/Oslo', 'Europe/Oslo')}
              onInput={(event) => setTimezone(event.currentTarget.value)}
              disabled={submitting()}
            />
          </Show>
        </div>

        <div class="verevon-schedule-dialog__field">
          <span>{i18n.tr('Hva kjøringen kan gjøre', 'What the run may do')}</span>
          <div class="verevon-schedule-dialog__policy" role="radiogroup" aria-label={i18n.tr('Hva kjøringen kan gjøre', 'What the run may do')}>
            <label class="verevon-schedule-dialog__policy-option">
              <input type="radio" name="schedule-policy" checked />
              {i18n.tr('Kun utkast (standard)', 'Draft only (default)')}
            </label>
            <label class="verevon-schedule-dialog__policy-option verevon-schedule-dialog__policy-option--disabled">
              <input type="radio" name="schedule-policy" disabled />
              {i18n.tr('Tillat handlinger, med godkjenning', 'Allow actions, with approval')}
              <small>
                {i18n.tr(
                  'Kommer senere — planlagte kjøringer kan ikke stoppe opp og vente på godkjenning ennå.',
                  'Coming later — scheduled runs cannot yet pause and wait for approval.',
                )}
              </small>
            </label>
          </div>
        </div>

        <p class="verevon-schedule-dialog__note">
          {i18n.tr('Kjører i din personlige arbeidsflate.', 'Runs in your personal workspace.')}
        </p>

        <Show when={errorMessage()}>
          {(message) => <p class="verevon-schedule-dialog__error" role="alert">{message()}</p>}
        </Show>

        <footer>
          <p class="verevon-schedule-dialog__note">
            {i18n.tr(
              'Pause stopper fremtidige kjøringer; en kjøring som allerede er i gang fullføres.',
              'Pausing stops future runs; a run already in progress still finishes.',
            )}
          </p>
          <button
            type="button"
            class="verevon-schedule-dialog__submit"
            disabled={!name().trim() || !scheduleExpr() || submitting()}
            onClick={() => void submit()}
          >
            {submitting() ? i18n.tr('Oppretter …', 'Creating…') : i18n.tr('Opprett plan', 'Create schedule')}
          </button>
        </footer>
      </section>
    </div>
  )
}
