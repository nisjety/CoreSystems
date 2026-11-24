import { Show, createEffect, createSignal } from 'solid-js'

import { ApiError } from '@/shared/api/api-error'
import { stopSpaceBrowserRunAsStarter } from '@/shared/api/spaces-client/browser-runs'
import { stopSpaceRun } from '@/shared/api/spaces-client/run-stop'
import { useI18n } from '@/shared/i18n'
import './space-browser.css'

export interface SpaceBrowserRunStopProps {
  readonly spaceRef: string
  readonly runId: string
  /**
   * The viewer started this run (D21h): Stop goes to the starter's own route, whatever their room role and with no
   * manager switch. Otherwise it is the room manager's Stop (WS09-S8).
   */
  readonly asStarter?: boolean
}

type StopState = 'idle' | 'pending' | 'sent' | 'forbidden' | 'not_starter' | 'zdr' | 'failed'

/**
 * Stop on a live room browser run (WS20). Two routes, one button:
 * - the run's starter (D21h): `POST /api/v1/spaces/{ref}/browser-runs/{run}/stop`. Model-gateway checks that the caller
 *   started the run, so a refusal there is shown as one.
 * - a room manager or owner (WS09-S8): `POST /api/v1/spaces/{ref}/runs/{run}/stop`, offered only when the listing says
 *   manager Stop is on.
 * Presentation only: Control decides on every call. A manager stop attempt is one key per run, so a retry after a
 * failure names the same attempt; the starter's stop carries its own per-run key server side. The end of the run
 * arrives with the card's own poll; until then the button says the stop was sent.
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

  // Stop stays open only before an attempt and after a failure a retry can fix. A refusal (the organisation, not the
  // starter, the retention posture) is answered the same way on every retry, so it closes the control.
  const canPress = () => state() === 'idle' || state() === 'failed'

  const requestStop = async () => {
    if (!canPress()) return
    setState('pending')
    try {
      if (props.asStarter === true) {
        await stopSpaceBrowserRunAsStarter(props.spaceRef, props.runId)
      } else {
        await stopSpaceRun(props.spaceRef, props.runId, keyFor(props.runId))
      }
      setState('sent')
    } catch (error) {
      const code = error instanceof ApiError ? error.code : ''
      if (code === 'space_run_control_not_authorized') setState('forbidden')
      else if (code === 'browser_run_not_yours' || code === 'space_browser_stop_not_authorized') setState('not_starter')
      else if (
        code === 'zdr_feature_not_supported'
        || code === 'zdr_durable_mutation_forbidden'
        || code === 'zdr_waiver_route_mismatch'
      )
        setState('zdr')
      else setState('failed')
    }
  }

  return (
    <div class="verevon-space-browser-stop">
      <button
        type="button"
        class="verevon-space-browser-stop__button"
        disabled={!canPress()}
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
      <Show when={state() === 'not_starter'}>
        <span role="alert">
          {i18n.tr(
            'Bare den som startet kjøringen kan stoppe den her. En leder i rommet kan også stoppe den.',
            'Only the person who started the run can stop it here. A room manager can also stop it.',
          )}
        </span>
      </Show>
      <Show when={state() === 'zdr'}>
        <span role="alert">
          {i18n.tr(
            'Med nulllagring av data kan kjøringen ikke stoppes herfra ennå. Ingenting er endret.',
            'With zero data retention the run cannot be stopped from here yet. Nothing was changed.',
          )}
        </span>
      </Show>
      <Show when={state() === 'failed'}>
        <span role="alert">{i18n.tr('Kunne ikke stoppe kjøringen. Prøv igjen.', 'Could not stop the run. Try again.')}</span>
      </Show>
    </div>
  )
}
