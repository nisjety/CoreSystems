import { createSignal, For, Show } from 'solid-js'

import { hasErrorCode } from '@/shared/api/http'
import {
  cancelSpaceReminder,
  createSpaceReminder,
  listPendingSpaceReminders,
  type SpaceReminder,
} from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { useFormat } from '@/shared/preferences/use-format'
import { formatWhenWithTime } from '../lib/space-thread-presentation'
import './space-remind-menu.css'

const MINUTE_MS = 60 * 1000
const HOUR_MS = 60 * MINUTE_MS

type Preset = { readonly id: string; readonly no: string; readonly en: string; readonly at: (now: number) => number }

/** Tomorrow at 09:00 in the viewer's own time zone. */
function tomorrowMorning(now: number): number {
  const date = new Date(now)
  date.setDate(date.getDate() + 1)
  date.setHours(9, 0, 0, 0)
  return date.getTime()
}

/** Every choice is inside the server's window (five seconds to 366 days ahead). */
const PRESETS: readonly Preset[] = [
  { id: '20m', no: 'Om 20 minutter', en: 'In 20 minutes', at: (now) => now + 20 * MINUTE_MS },
  { id: '1h', no: 'Om 1 time', en: 'In 1 hour', at: (now) => now + HOUR_MS },
  { id: '3h', no: 'Om 3 timer', en: 'In 3 hours', at: (now) => now + 3 * HOUR_MS },
  { id: 'tomorrow', no: 'I morgen kl. 09', en: 'Tomorrow at 9', at: tomorrowMorning },
  { id: 'week', no: 'Om en uke', en: 'In a week', at: (now) => now + 7 * 24 * HOUR_MS },
]

/**
 * "Remind me" on one post or message of a room (WS11-S11). A reminder is a time and the ids of the
 * post: when it comes due it appears in your own attention inbox, with no note and no text of the
 * message stored. It is yours alone; nobody else sees it. Setting one needs the organization to
 * keep data, so under Zero Data Retention the server refuses with a typed 412 and the menu says so;
 * a pending reminder can always be cancelled. With the gateway flag off (404
 * `space_reminders_disabled`) the control is not rendered at all.
 */
export function SpaceRemindMenu(props: {
  readonly spaceRef: string
  readonly threadId: string
  readonly messageId?: string
  readonly class?: string
}) {
  const i18n = useI18n()
  const fmt = useFormat()
  const [open, setOpen] = createSignal(false)
  const [available, setAvailable] = createSignal(true)
  const [pending, setPending] = createSignal<readonly SpaceReminder[]>([])
  const [custom, setCustom] = createSignal('')
  const [busy, setBusy] = createSignal(false)
  const [message, setMessage] = createSignal('')
  const [error, setError] = createSignal('')

  const inputId = () => `space-remind-at-${props.threadId}-${props.messageId ?? 'thread'}`

  /** The caller's pending reminders for exactly this post. */
  const mine = () =>
    pending().filter(
      (row) =>
        row.spaceRef === props.spaceRef &&
        row.threadId === props.threadId &&
        (row.messageId ?? '') === (props.messageId ?? ''),
    )

  const when = (ms: number) => formatWhenWithTime(new Date(ms).toISOString(), fmt)

  const refresh = async () => {
    try {
      setPending(await listPendingSpaceReminders())
    } catch (err) {
      if (hasErrorCode(err, 'space_reminders_disabled')) setAvailable(false)
      // Any other failure leaves the list as it was; setting a reminder still works.
    }
  }

  const toggle = () => {
    const next = !open()
    setOpen(next)
    setError('')
    setMessage('')
    if (next) void refresh()
  }

  const remindAt = async (at: number) => {
    if (busy()) return
    setBusy(true)
    setError('')
    setMessage('')
    try {
      const created = await createSpaceReminder({
        spaceRef: props.spaceRef,
        threadId: props.threadId,
        messageId: props.messageId,
        remindAt: at,
      })
      setMessage(`${i18n.tr('Påminnelse satt til', 'Reminder set for')} ${when(created.remindAt)}.`)
      await refresh()
    } catch (err) {
      if (hasErrorCode(err, 'space_reminders_disabled')) {
        setAvailable(false)
      } else if (hasErrorCode(err, 'zdr_durable_mutation_forbidden')) {
        setError(
          i18n.tr(
            'Organisasjonen lagrer ikke innhold, så påminnelser er ikke tilgjengelige.',
            'Your organization keeps no data, so reminders are not available.',
          ),
        )
      } else if (hasErrorCode(err, 'too_many_reminders')) {
        setError(
          i18n.tr(
            'Du har like mange ventende påminnelser som tillatt. Avbryt en for å legge til en ny.',
            'You have the most pending reminders allowed. Cancel one to add another.',
          ),
        )
      } else if (hasErrorCode(err, 'invalid_reminder_time')) {
        setError(
          i18n.tr(
            'Velg et tidspunkt som ligger frem i tid, men innen et år.',
            'Choose a time in the future, within a year.',
          ),
        )
      } else {
        setError(
          translateApiError(err, i18n.tr, {
            no: 'Påminnelsen kunne ikke settes. Ingenting er endret.',
            en: 'The reminder could not be set. Nothing was changed.',
          }),
        )
      }
    } finally {
      setBusy(false)
    }
  }

  const submitCustom = (event: Event) => {
    event.preventDefault()
    // A datetime-local value is the viewer's own wall-clock time.
    const at = new Date(custom()).getTime()
    if (!Number.isFinite(at)) {
      setError(i18n.tr('Velg dato og klokkeslett.', 'Choose a date and time.'))
      return
    }
    void remindAt(at)
  }

  const cancel = async (reminderId: string) => {
    if (busy()) return
    setBusy(true)
    setError('')
    setMessage('')
    try {
      await cancelSpaceReminder(reminderId)
      setMessage(i18n.tr('Påminnelsen er avbrutt.', 'Reminder cancelled.'))
      await refresh()
    } catch (err) {
      if (hasErrorCode(err, 'reminder_not_found')) {
        // Already fired or cancelled elsewhere: the list is the truth, so re-read it.
        await refresh()
      } else {
        setError(
          translateApiError(err, i18n.tr, {
            no: 'Påminnelsen kunne ikke avbrytes.',
            en: 'The reminder could not be cancelled.',
          }),
        )
      }
    } finally {
      setBusy(false)
    }
  }

  return (
    <Show when={available()}>
      <div class={`verevon-space-remind-menu${props.class ? ` ${props.class}` : ''}`}>
        <button
          type="button"
          class="verevon-space-remind-menu__trigger"
          aria-expanded={open() ? 'true' : 'false'}
          aria-haspopup="true"
          onClick={toggle}
        >
          {i18n.tr('Påminn meg', 'Remind me')}
        </button>
        <Show when={open()}>
          <div class="verevon-space-remind-menu__panel" role="group" aria-label={i18n.tr('Påminnelse', 'Reminder')}>
            <p class="verevon-space-remind-menu__hint">
              {i18n.tr(
                'Du får en påminnelse i innboksen din. Den inneholder ikke teksten i meldingen.',
                'You get a reminder in your inbox. It does not contain the text of the message.',
              )}
            </p>
            <ul class="verevon-space-remind-menu__presets">
              <For each={PRESETS}>
                {(preset) => (
                  <li>
                    <button
                      type="button"
                      class="verevon-space-remind-menu__preset"
                      disabled={busy()}
                      onClick={() => void remindAt(preset.at(Date.now()))}
                    >
                      {i18n.tr(preset.no, preset.en)}
                    </button>
                  </li>
                )}
              </For>
            </ul>
            <form class="verevon-space-remind-menu__custom" onSubmit={submitCustom}>
              <label for={inputId()}>{i18n.tr('Velg tidspunkt', 'Pick a time')}</label>
              <input
                id={inputId()}
                type="datetime-local"
                value={custom()}
                onInput={(event) => setCustom(event.currentTarget.value)}
              />
              <button type="submit" class="verevon-space-remind-menu__preset" disabled={busy() || custom() === ''}>
                {i18n.tr('Sett', 'Set')}
              </button>
            </form>
            <Show when={mine().length > 0}>
              <ul
                class="verevon-space-remind-menu__pending"
                aria-label={i18n.tr('Ventende påminnelser', 'Pending reminders')}
              >
                <For each={mine()}>
                  {(row) => (
                    <li>
                      <span>{when(row.remindAt)}</span>
                      <button
                        type="button"
                        class="verevon-space-remind-menu__preset"
                        disabled={busy()}
                        onClick={() => void cancel(row.reminderId)}
                      >
                        {i18n.tr('Avbryt', 'Cancel')}
                      </button>
                    </li>
                  )}
                </For>
              </ul>
            </Show>
            <Show when={error()}>
              <p class="verevon-space-remind-menu__error" role="alert">
                {error()}
              </p>
            </Show>
            <Show when={message()}>
              <p class="verevon-space-remind-menu__hint" role="status">
                {message()}
              </p>
            </Show>
          </div>
        </Show>
      </div>
    </Show>
  )
}
