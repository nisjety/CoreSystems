import { Show, createEffect, createSignal } from 'solid-js'

import { ApiError } from '@/shared/api/api-error'
import { stopSpaceRun } from '@/shared/api/spaces-client/run-stop'
import { useI18n } from '@/shared/i18n'
import './space-browser.css'

export interface SpaceBrowserRunStopProps {
  readonly spaceRef: string
  readonly runId: string
}

type StopState = 'idle' | 'pending' | 'sent' | 'forbidden' | 'failed'

/**
 * Stop on a live room browser run (WS20), for a room manager or owner. It is the same operation as Stop on an agent run
 * in the timeline (`POST /api/v1/spaces/{ref}/runs/{run}/stop`, WS09-S8): the caller offers it only when the listing says
 * manager Stop is on and the viewer's room role is manager or owner. Presentation only: Control decides and Session Core
 * verifies on every call, and a refusal is shown as one. The run's own starter has no stop route yet.
 *
 * One stop attempt is one key per run, so a retry after a failure names the same attempt and the server de-duplicates.
 * The end of the run arrives with the card's own poll; until then the button says the stop was sent.
 */
export function SpaceBrowserRunStop(props: SpaceBrowserRunStopProps) {
  const i18n = useI18n()
  const [state, setState] = createSignal<StopState>('idle')
  const keys = new Map<string, string>()
  const keyFor = (runId: string): string => {
    const held = keys.get(runId)
    if (held) return held
    const fresh = `stop-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`
    keys.set(runId, fresh)
    return fresh
  }

  // A different run never inherits the previous run's "sent".
  createEffect(
    () => props.runId,
    () => {
      setState('idle')
    },
  )

  const requestStop = async () => {
    if (state() === 'pending' || state() === 'sent') return
    setState('pending')
    try {
      await stopSpaceRun(props.spaceRef, props.runId, keyFor(props.runId))
      setState('sent')
    } catch (error) {
      const code = error instanceof ApiError ? error.code : ''
      setState(code === 'space_run_control_not_authorized' ? 'forbidden' : 'failed')
    }
  }

  return (
    <div class="verevon-space-browser-stop">
      <button
        type="button"
        class="verevon-space-browser-stop__button"
        disabled={state() === 'pending' || state() === 'sent'}
        onClick={() => {
          void requestStop()
        }}
      >
        {i18n.tr('Stopp kjøringen', 'Stop this run')}
      </button>
      <Show when={state() === 'sent'}>
        <span role="status">{i18n.tr('Stopp sendt …', 'Stop sent…')}</span>
      </Show>
      <Show when={state() === 'forbidden'}>
        <span role="alert">
          {i18n.tr(
            'Organisasjonen din har ikke slått på stopping av andre medlemmers kjøringer.',
            "Your organisation has not enabled stopping other members' runs.",
          )}
        </span>
      </Show>
      <Show when={state() === 'failed'}>
        <span role="alert">{i18n.tr('Kunne ikke stoppe kjøringen. Prøv igjen.', 'Could not stop the run. Try again.')}</span>
      </Show>
    </div>
  )
}
