import { createSignal, Show } from 'solid-js'
import { Pause, Play, Square } from '@/shared/icons'
import {
  cancelIngestionRun,
  pauseIngestionRun,
  resumeIngestionRun,
  type RunItem,
} from '@/shared/api/ingestions-client'
import { ApiError } from '@/shared/api/api-error'
import { useI18n } from '@/shared/i18n'
import { translateApiError } from '@/shared/i18n/errors'
import { Button } from '@/shared/ui/Button'

/**
 * WS15.L5: pause, resume and cancel for one job row on the Ingestions page.
 *
 * The page mounts this only when the installation reports the `jobControl`
 * capability (GET /api/ingestions/schedules/capabilities), so with the edge's
 * QUARRY_EDGE__JOB_CONTROL flag off no control is drawn and none can answer 501.
 * Pause and resume are reversible and act at once; cancel is irreversible, so it
 * takes a second, explicit confirmation (the registry marks it requiresApproval).
 */

export type JobControlState = 'running' | 'paused' | 'none'

const RUNNING_STATUSES = ['running', 'queued', 'pending', 'active', 'in_progress']

/** Which controls a job offers. Only crawl jobs are proxied by the gateway today. */
export function jobControlState(run: Pick<RunItem, 'kind' | 'status' | 'desiredState'>): JobControlState {
  if (run.kind.toLowerCase() !== 'crawl') return 'none'
  const desired = run.desiredState?.toLowerCase()
  if (desired === 'cancelled' || desired === 'canceled') return 'none'
  const status = run.status.toLowerCase()
  if (status === 'paused' || desired === 'paused') return 'paused'
  return RUNNING_STATUSES.includes(status) ? 'running' : 'none'
}

type Pending = 'pause' | 'resume' | 'cancel' | null

export function RunJobControls(props: { run: RunItem; onChanged: () => void | Promise<void> }) {
  const i18n = useI18n()
  const [pending, setPending] = createSignal<Pending>(null)
  const [confirming, setConfirming] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)

  const state = () => jobControlState(props.run)

  function failureMessage(failure: unknown) {
    if (failure instanceof ApiError && failure.status === 501) {
      return i18n.tr(
        'Jobbkontroll er ikke aktivert på denne installasjonen.',
        'Job control is not enabled on this installation.',
      )
    }
    return translateApiError(failure, i18n.tr, {
      no: 'Handlingen mislyktes.',
      en: 'The action failed.',
    })
  }

  async function run(action: Exclude<Pending, null>) {
    if (pending()) return
    setError(null)
    setPending(action)
    try {
      if (action === 'pause') await pauseIngestionRun(props.run.id)
      else if (action === 'resume') await resumeIngestionRun(props.run.id)
      else await cancelIngestionRun(props.run.id)
      setConfirming(false)
      await props.onChanged()
    } catch (failure) {
      setError(failureMessage(failure))
    } finally {
      setPending(null)
    }
  }

  return (
    <Show when={state() !== 'none'}>
      {/* A click here must not select the row behind it. */}
      <div class="ingestions-job-controls" onClick={(event) => event.stopPropagation()}>
        <Show
          when={!confirming()}
          fallback={
            <div class="ingestions-actions-row" role="group" aria-label={i18n.tr('Bekreft avbryt', 'Confirm cancel')}>
              <span>{i18n.tr('Avbryte? Kan ikke gjenopptas.', 'Cancel? It cannot be resumed.')}</span>
              <Button size="xs" disabled={pending() !== null} onClick={() => void run('cancel')}>
                {i18n.tr('Ja, avbryt', 'Yes, cancel')}
              </Button>
              <Button size="xs" variant="ghost" disabled={pending() !== null} onClick={() => setConfirming(false)}>
                {i18n.tr('Behold', 'Keep running')}
              </Button>
            </div>
          }
        >
          <div class="ingestions-actions-row">
            <Show
              when={state() === 'running'}
              fallback={
                <Button size="xs" disabled={pending() !== null} onClick={() => void run('resume')}>
                  <Play class="size-4" strokeWidth={1.9} />
                  {i18n.tr('Gjenoppta', 'Resume')}
                </Button>
              }
            >
              <Button size="xs" disabled={pending() !== null} onClick={() => void run('pause')}>
                <Pause class="size-4" strokeWidth={1.9} />
                {i18n.tr('Pause', 'Pause')}
              </Button>
            </Show>
            <Button size="xs" variant="ghost" disabled={pending() !== null} onClick={() => setConfirming(true)}>
              <Square class="size-4" strokeWidth={1.9} />
              {i18n.tr('Avbryt', 'Cancel')}
            </Button>
          </div>
        </Show>
        <Show when={error()}>
          <p class="ingestions-job-controls__error" role="alert">
            {error()}
          </p>
        </Show>
      </div>
    </Show>
  )
}
