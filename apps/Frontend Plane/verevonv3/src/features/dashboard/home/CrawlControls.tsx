import { createSignal, Show } from 'solid-js'
import { ApiError } from '@/shared/api/api-error'
import { getScheduleCapabilities } from '@/shared/api/ingestions-client'
import {
  cancelCrawl,
  getCrawlStatus,
  pauseCrawl,
  resumeCrawl,
  type CrawlControlResult,
} from '@/shared/api/knowledge-client'
import { Pause, Play, Square } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { translateApiError } from '@/shared/i18n/errors'
import { createResource } from '@/shared/lib/create-resource-compat'
import { Button } from '@/shared/ui/Button'

/**
 * WS15.L5: pause, resume and cancel for one crawl job on the Knowledge composer.
 *
 * Drawn only when the installation reports the `jobControl` capability (GET
 * /api/ingestions/schedules/capabilities). With the edge's QUARRY_EDGE__JOB_CONTROL flag off no
 * control is drawn, so none can answer 501. Pause and resume are reversible and act at once.
 * Cancel is irreversible, so it takes a second, explicit confirmation (the registry marks
 * `ingestions.cancel_run` requiresApproval).
 *
 * The buttons follow the job's phase, as the edge derives it: pause only while the job runs,
 * resume only while it is paused, and cancel for any job that is not finished or already
 * being cancelled.
 */

type CrawlControlVerb = 'pause' | 'resume' | 'cancel'

const TERMINAL_JOB_STATUSES = new Set([
  'completed',
  'complete',
  'failed',
  'error',
  'succeeded',
  'cancelled',
  'timed_out',
  'skipped',
])

// Once the installation has said yes it does not change while the page is open, so the answer is kept. A no is
// asked again the next time a card mounts: the capability read swallows a failed request into the all-off answer.
let jobControlKnown = false

/** Whether this installation honours pause, resume and cancel (the `jobControl` capability). */
function readJobControlCapability(): Promise<boolean> {
  if (jobControlKnown) return Promise.resolve(true)
  return getScheduleCapabilities().then((capabilities) => {
    jobControlKnown = capabilities.jobControl
    return capabilities.jobControl
  })
}

/** The phase a job reads as from its stored status. `pausing`, `resuming` and `cancelling` come only
 * from a control answer, never from the stored status. */
function phaseOf(status: string): string {
  const value = status.trim().toLowerCase()
  if (value === 'accepted' || value === 'queued' || value === 'pending') return 'queued'
  if (value === 'paused') return 'paused'
  return 'running'
}

/** The words for a phase that is changing or waiting. A running or paused job needs none: its row already says so. */
function phaseLabel(phase: string, i18n: ReturnType<typeof useI18n>): string | null {
  switch (phase) {
    case 'queued':
      return i18n.tr('Venter i kø', 'Queued')
    case 'pausing':
      return i18n.tr('Pauser ...', 'Pausing ...')
    case 'resuming':
      return i18n.tr('Gjenopptar ...', 'Resuming ...')
    case 'cancelling':
      return i18n.tr('Avbryter ...', 'Cancelling ...')
    default:
      return null
  }
}

export function CrawlControls(props: {
  orgId: string
  jobId: string
  /** The job's stored status, as the composer last read it. */
  status: string
  /** Called with the status a control answer reports, or with a fresh read after a failed action. */
  onStatusChange: (status: string) => void
}) {
  const i18n = useI18n()
  const [jobControl] = createResource(readJobControlCapability)
  const [busy, setBusy] = createSignal<CrawlControlVerb | null>(null)
  const [confirming, setConfirming] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)
  // The phase a control answer reported while its change is still pending. It holds only while the
  // status it was read against is unchanged: once the status moves, the stored status speaks again.
  const [pendingPhase, setPendingPhase] = createSignal<{ phase: string; from: string } | null>(null)

  const phase = (): string => {
    const pending = pendingPhase()
    return pending && pending.from === props.status ? pending.phase : phaseOf(props.status)
  }
  const canPause = () => phase() === 'running'
  const canResume = () => phase() === 'paused'
  const canCancel = () => phase() !== 'cancelling'
  const finished = () => TERMINAL_JOB_STATUSES.has(props.status.trim().toLowerCase())

  function failureMessage(failure: unknown): string {
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

  // After a failed action the job may have moved on (a 409): read its status again, so the buttons match the server.
  async function rereadStatus() {
    try {
      const current = await getCrawlStatus(props.orgId, props.jobId)
      if (current.status) props.onStatusChange(current.status)
    } catch {
      // Keep the status the composer shows; its next poll reads the job again.
    }
  }

  async function run(verb: CrawlControlVerb) {
    if (busy()) return
    const from = props.status
    setError(null)
    setBusy(verb)
    try {
      const result: CrawlControlResult = verb === 'pause'
        ? await pauseCrawl(props.orgId, props.jobId)
        : verb === 'resume'
          ? await resumeCrawl(props.orgId, props.jobId)
          : await cancelCrawl(props.orgId, props.jobId)
      setConfirming(false)
      setPendingPhase(result.control.pending && result.phase ? { phase: result.phase, from } : null)
      if (result.status) props.onStatusChange(result.status)
    } catch (failure) {
      setError(failureMessage(failure))
      void rereadStatus()
    } finally {
      setBusy(null)
    }
  }

  return (
    <Show when={jobControl() === true && !finished()}>
      <>
        <span
          class="ingestions-actions-row"
          role="group"
          aria-label={i18n.tr('Jobbkontroll', 'Job control')}
          style={{ 'justify-content': 'flex-start', 'margin-top': '6px' }}
        >
          <Show
            when={!confirming()}
            fallback={
              <>
                <span class="dashboard-knowledge-composer__job-detail">
                  {i18n.tr('Avbryte? Kan ikke gjenopptas.', 'Cancel? It cannot be resumed.')}
                </span>
                <Button size="xs" disabled={busy() !== null} onClick={() => void run('cancel')}>
                  {i18n.tr('Ja, avbryt', 'Yes, cancel')}
                </Button>
                <Button size="xs" variant="ghost" disabled={busy() !== null} onClick={() => setConfirming(false)}>
                  {i18n.tr('Behold', 'Keep running')}
                </Button>
              </>
            }
          >
            <Show when={phaseLabel(phase(), i18n)}>
              {(label) => <span class="dashboard-knowledge-composer__job-detail">{label()}</span>}
            </Show>
            <Show when={canPause()}>
              <Button size="xs" disabled={busy() !== null} onClick={() => void run('pause')}>
                <Pause class="size-4" strokeWidth={1.9} />
                {i18n.tr('Pause', 'Pause')}
              </Button>
            </Show>
            <Show when={canResume()}>
              <Button size="xs" disabled={busy() !== null} onClick={() => void run('resume')}>
                <Play class="size-4" strokeWidth={1.9} />
                {i18n.tr('Gjenoppta', 'Resume')}
              </Button>
            </Show>
            <Show when={canCancel()}>
              <Button size="xs" variant="ghost" disabled={busy() !== null} onClick={() => setConfirming(true)}>
                <Square class="size-4" strokeWidth={1.9} />
                {i18n.tr('Avbryt', 'Cancel')}
              </Button>
            </Show>
          </Show>
        </span>
        <Show when={error()}>
          {(message) => <span class="dashboard-knowledge-composer__error" role="alert">{message()}</span>}
        </Show>
      </>
    </Show>
  )
}
