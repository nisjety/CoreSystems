import { createSignal, For, onSettled, Show } from 'solid-js'

import { hasErrorCode } from '@/shared/api/http'
import {
  getOwnSpaceStatus,
  setOwnSpaceStatus,
  SPACE_STATUS_STATES,
  SPACE_STATUS_TEXT_MAX,
  type SpaceMemberStatus,
  type SpaceMemberStatusState,
} from '@/shared/api/spaces-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { useFormat } from '@/shared/preferences/use-format'
import { formatWhenWithTime } from '../lib/space-thread-presentation'
import { SpaceMemberStatusChip, statusStateLabel } from './SpaceStatusChip'
import './space-status.css'

const HOUR_MS = 60 * 60 * 1000

/** How long a status lasts. The server accepts at most seven days; every choice here is inside it. */
const DURATIONS: readonly { readonly id: string; readonly ms: number | null; readonly no: string; readonly en: string }[] = [
  { id: 'none', ms: null, no: 'Til jeg endrer den', en: 'Until I change it' },
  { id: '1h', ms: HOUR_MS, no: '1 time', en: '1 hour' },
  { id: '4h', ms: 4 * HOUR_MS, no: '4 timer', en: '4 hours' },
  { id: '24h', ms: 24 * HOUR_MS, no: '24 timer', en: '24 hours' },
  { id: '3d', ms: 72 * HOUR_MS, no: '3 dager', en: '3 days' },
]

/**
 * Your status in this organization (WS11-S10): one of four states, an optional short text and an
 * optional expiry. It is shown next to your name in the rooms of this organization only, never in
 * another one; `dnd` mutes out-of-app notifications and nothing else. The text is plain text and is
 * never given to a model. Under Zero Data Retention the server refuses text (a typed 412) and
 * accepts the state alone. With the gateway flag off (404 `space_status_disabled`) the control is
 * not rendered at all.
 */
export function SpaceStatusMenu() {
  const i18n = useI18n()
  const fmt = useFormat()
  const [available, setAvailable] = createSignal(true)
  const [loaded, setLoaded] = createSignal(false)
  const [current, setCurrent] = createSignal<SpaceMemberStatus | null>(null)
  const [state, setState] = createSignal<SpaceMemberStatusState>('available')
  const [text, setText] = createSignal('')
  const [duration, setDuration] = createSignal('none')
  const [busy, setBusy] = createSignal(false)
  const [message, setMessage] = createSignal('')
  const [error, setError] = createSignal('')

  const adopt = (status: SpaceMemberStatus | null) => {
    setCurrent(status)
    setState(status?.state ?? 'available')
    setText(status?.text ?? '')
    setDuration('none')
  }

  onSettled(() => {
    getOwnSpaceStatus()
      .then(adopt)
      .catch((err: unknown) => {
        if (hasErrorCode(err, 'space_status_disabled')) setAvailable(false)
        else {
          setError(
            translateApiError(err, i18n.tr, { no: 'Kunne ikke hente statusen din.', en: 'Could not load your status.' }),
          )
        }
      })
      .finally(() => setLoaded(true))
  })

  const save = async (next: { state: SpaceMemberStatusState; text: string; expiresInMs: number | null }) => {
    if (busy()) return
    setBusy(true)
    setError('')
    setMessage('')
    try {
      const saved = await setOwnSpaceStatus({
        state: next.state,
        text: next.text,
        expiresAt: next.expiresInMs === null ? undefined : Date.now() + next.expiresInMs,
      })
      adopt(saved)
      setMessage(i18n.tr('Statusen er lagret.', 'Status saved.'))
    } catch (err) {
      if (hasErrorCode(err, 'zdr_durable_mutation_forbidden')) {
        setError(
          i18n.tr(
            'Organisasjonen lagrer ikke innhold, så statustekst er ikke tilgjengelig. Du kan fortsatt velge en tilstand.',
            'Your organization keeps no data, so status text is not available. You can still choose a state.',
          ),
        )
      } else {
        setError(
          translateApiError(err, i18n.tr, {
            no: 'Statusen kunne ikke lagres. Ingenting er endret.',
            en: 'The status could not be saved. Nothing was changed.',
          }),
        )
      }
    } finally {
      setBusy(false)
    }
  }

  const submit = (event: Event) => {
    event.preventDefault()
    const chosen = DURATIONS.find((item) => item.id === duration())
    void save({ state: state(), text: text(), expiresInMs: chosen?.ms ?? null })
  }

  // The server has no clear call: "available" with no text and no expiry is the cleared state.
  const clear = () => void save({ state: 'available', text: '', expiresInMs: null })

  return (
    <Show when={available() && loaded()}>
      <section id="space-status" class="verevon-settings-section">
      <form class="verevon-space-status-menu" onSubmit={submit} aria-labelledby="space-status-heading">
        <p id="space-status-heading" class="verevon-space-status-menu__hint">
          <strong>{i18n.tr('Status i Space', 'Space status')}</strong>
          {' · '}
          {i18n.tr(
            'Vises ved navnet ditt i rommene i denne organisasjonen. «Ikke forstyrr» demper bare varsler utenfor appen.',
            'Shown next to your name in rooms of this organization. "Do not disturb" only mutes notifications outside the app.',
          )}
        </p>

        <Show when={current()}>
          {(status) => (
            <p class="verevon-space-status-menu__hint">
              {i18n.tr('Nå:', 'Now:')}{' '}
              <SpaceMemberStatusChip
                status={{ state: status().state, text: status().text, expires_at: status().expiresAt }}
              />
              <Show when={status().expiresAt}>
                {(at) => (
                  <>
                    {' · '}
                    {i18n.tr('utløper', 'expires')} {formatWhenWithTime(new Date(at()).toISOString(), fmt)}
                  </>
                )}
              </Show>
            </p>
          )}
        </Show>

        <fieldset class="verevon-space-status-menu__states">
          <legend>{i18n.tr('Tilstand', 'State')}</legend>
          <For each={SPACE_STATUS_STATES}>
            {(option) => (
              <label class="verevon-space-status-menu__state">
                <input
                  type="radio"
                  name="space-status-state"
                  value={option}
                  checked={state() === option}
                  onChange={() => setState(option)}
                />
                {statusStateLabel(option, i18n.tr)}
              </label>
            )}
          </For>
        </fieldset>

        <div class="verevon-space-status-menu__row">
          <label for="space-status-text">{i18n.tr('Tekst (valgfri)', 'Text (optional)')}</label>
          <input
            id="space-status-text"
            class="verevon-settings-input"
            type="text"
            maxlength={SPACE_STATUS_TEXT_MAX}
            autocomplete="off"
            value={text()}
            onInput={(event) => setText(event.currentTarget.value)}
          />
        </div>

        <div class="verevon-space-status-menu__row">
          <label for="space-status-duration">{i18n.tr('Varighet', 'Duration')}</label>
          <select
            id="space-status-duration"
            class="verevon-settings-input"
            value={duration()}
            onChange={(event) => setDuration(event.currentTarget.value)}
          >
            <For each={DURATIONS}>{(item) => <option value={item.id}>{i18n.tr(item.no, item.en)}</option>}</For>
          </select>
        </div>

        <Show when={error()}>
          <p class="verevon-space-status-menu__error" role="alert">{error()}</p>
        </Show>
        <Show when={message()}>
          <p class="verevon-space-status-menu__hint" role="status">{message()}</p>
        </Show>

        <div class="verevon-space-status-menu__actions">
          <button type="submit" class="verevon-space-secondary-action" disabled={busy()}>
            {i18n.tr('Lagre status', 'Save status')}
          </button>
          <button
            type="button"
            class="verevon-space-secondary-action"
            disabled={busy() || current() === null}
            onClick={clear}
          >
            {i18n.tr('Fjern status', 'Clear status')}
          </button>
        </div>
      </form>
      </section>
    </Show>
  )
}
