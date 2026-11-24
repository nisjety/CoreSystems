import { createSignal, For, onSettled, Show } from 'solid-js'

import { hasErrorCode } from '@/shared/api/http'
import { cancelSpaceReminder, listPendingSpaceReminders, type SpaceReminder } from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { useFormat } from '@/shared/preferences/use-format'
import { spaceEntityHref } from '../lib/space-entity-link'
import { formatWhenWithTime } from '../lib/space-thread-presentation'
import './space-remind-menu.css'
import './space-status.css'

/**
 * Your pending reminders in every room (WS11-S11), in one place on the account page beside your
 * status. Each row says when it comes due, opens its post and can be cancelled. A reminder carries
 * ids and a time only, so the list shows no text. It is yours alone. With the gateway's reminders flag
 * off (404 `space_reminders_disabled`) nothing is drawn. A cancel answered `reminder_not_found` means
 * the reminder already fired or went elsewhere, so the list is read again and no cancellation is claimed.
 */
export function SpaceRemindersList() {
  const i18n = useI18n()
  const fmt = useFormat()
  const [available, setAvailable] = createSignal(true)
  const [loaded, setLoaded] = createSignal(false)
  const [rows, setRows] = createSignal<readonly SpaceReminder[]>([])
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [message, setMessage] = createSignal('')
  // A failed read and a failed cancel are kept apart: a successful re-read must not erase a cancel's refusal.
  const [loadError, setLoadError] = createSignal('')
  const [actionError, setActionError] = createSignal('')

  const refresh = async () => {
    try {
      setRows(await listPendingSpaceReminders())
      setLoadError('')
    } catch (err) {
      // Flag off, or a session with no active organization (403 `active_membership_required`): there
      // are no Space reminders to show, so nothing is drawn rather than a load error.
      if (hasErrorCode(err, 'space_reminders_disabled') || hasErrorCode(err, 'active_membership_required')) {
        setAvailable(false)
      } else {
        setLoadError(
          translateApiError(err, i18n.tr, {
            no: 'Påminnelsene kunne ikke hentes.',
            en: 'Your reminders could not be loaded.',
          }),
        )
      }
    }
  }

  onSettled(() => {
    void refresh().finally(() => setLoaded(true))
  })

  const cancel = async (reminderId: string) => {
    if (busyId() !== null) return
    setBusyId(reminderId)
    setActionError('')
    setMessage('')
    try {
      await cancelSpaceReminder(reminderId)
      setMessage(i18n.tr('Påminnelsen er avbrutt.', 'Reminder cancelled.'))
    } catch (err) {
      if (!hasErrorCode(err, 'reminder_not_found')) {
        setActionError(
          translateApiError(err, i18n.tr, {
            no: 'Påminnelsen kunne ikke avbrytes.',
            en: 'The reminder could not be cancelled.',
          }),
        )
      }
    } finally {
      setBusyId(null)
    }
    // Whatever the answer, the list is the truth: read it again.
    await refresh()
  }

  const when = (ms: number) => formatWhenWithTime(new Date(ms).toISOString(), fmt)

  const openHref = (row: SpaceReminder): string =>
    spaceEntityHref(
      row.messageId
        ? { kind: 'message', spaceRef: row.spaceRef, messageId: row.messageId, threadId: row.threadId }
        : { kind: 'thread', spaceRef: row.spaceRef, threadId: row.threadId },
    )

  return (
    <Show when={available() && loaded()}>
      <section class="verevon-space-status-menu verevon-space-reminders" aria-labelledby="space-reminders-heading">
        <p id="space-reminders-heading" class="verevon-space-remind-menu__hint">
          <strong>{i18n.tr('Påminnelser', 'Reminders')}</strong>
          {' · '}
          {i18n.tr(
            'Dine ventende påminnelser i alle rom. Teksten i innleggene vises ikke.',
            'Your pending reminders in every room. The text of the posts is not shown.',
          )}
        </p>
        <Show
          when={rows().length > 0}
          fallback={
            <p class="verevon-space-remind-menu__hint">
              {i18n.tr('Du har ingen ventende påminnelser.', 'You have no pending reminders.')}
            </p>
          }
        >
          <ul
            class="verevon-space-remind-menu__pending"
            aria-label={i18n.tr('Ventende påminnelser', 'Pending reminders')}
          >
            <For each={rows()}>
              {(row) => (
                <li>
                  <span>{when(row.remindAt)}</span>
                  <a class="verevon-space-remind-menu__preset" href={openHref(row)}>
                    {i18n.tr('Åpne innlegget', 'Open the post')}
                  </a>
                  <button
                    type="button"
                    class="verevon-space-remind-menu__preset"
                    disabled={busyId() !== null}
                    onClick={() => void cancel(row.reminderId)}
                  >
                    {i18n.tr('Avbryt', 'Cancel')}
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
        <Show when={actionError() || loadError()}>
          <p class="verevon-space-remind-menu__error" role="status">
            {actionError() || loadError()}
          </p>
        </Show>
        <Show when={message()}>
          <p class="verevon-space-remind-menu__hint" role="status">
            {message()}
          </p>
        </Show>
      </section>
    </Show>
  )
}
