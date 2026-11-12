import './research-run-status.css'
import { createMemo, createSignal, Show } from 'solid-js'
import { executeAction } from '@/shared/actions/action-client'
import type { ActionActor } from '@/shared/actions/types'
import { getResearchRun, type ResearchRunCancelAck, type ResearchRunStatusWord } from '@/shared/api/research-runs-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'
import { getSession } from '@/shared/session/session-store'

/**
 * The addressable research run of a deep-research answer (Q4-062b). The answer's `research_run` frame names the run
 * and its status word, and the chip shows that status in the person's language. While the run is running it offers
 * Stop, which sends `chat.research_run.cancel` through the action contract: the gateway asks the run's live stream to
 * end, and the stream then reports `cancelled`. Once the run has ended, the chip reads it once for its pages read and
 * why it stopped. Ids, status and counters only: nothing here shows answer text, a query or a URL.
 *
 * ChatMessages mounts it only behind `VITE_CHAT_RESEARCH_RUNS`, and never in a temporary chat or a foreign-origin or
 * Support read-only thread (`crawlPlanCardAllowed`, QF-20). It sits beside the answer's text, not inside it, so an
 * export or print of the answer does not carry it.
 */

type Tr = (noText: string, enText: string) => string

/** The label of each status word, in the person's language. */
function statusLabels(tr: Tr): Record<ResearchRunStatusWord, string> {
  return {
    running: tr('Research pågår', 'Research running'),
    completed: tr('Research fullført', 'Research completed'),
    stopped: tr('Research stoppet', 'Research stopped'),
    cancelled: tr('Research avbrutt', 'Research cancelled'),
    failed: tr('Research feilet', 'Research failed'),
    expired: tr('Research utløpt', 'Research expired'),
  }
}

/**
 * Why a run ended, for the words the run record gives (`ResearchStop` in model-gateway). A word the status already says
 * (`completed`, `cancelled`) adds nothing, and so does a word this build does not know.
 */
function stopReasonLabel(reason: string | null, tr: Tr): string | undefined {
  switch (reason) {
    case 'deadline':
      return tr('tidsgrensen ble nådd', 'the time limit was reached')
    case 'budget_usd':
      return tr('kostnadsgrensen ble nådd', 'the cost limit was reached')
    case 'cost_unpriced':
      return tr('kostnaden kunne ikke beregnes', 'the cost could not be worked out')
    case 'no_evidence':
      return tr('ingen kilder å skrive svaret fra', 'no sources to write the answer from')
    default:
      return undefined
  }
}

function pagesReadLabel(count: number, tr: Tr): string {
  return count === 1 ? tr('1 side lest', '1 page read') : tr(`${count} sider lest`, `${count} pages read`)
}

/** The chip for one research run: its status word, the person's Stop while it runs, and what it read once it ends. */
export function ResearchRunStatus(props: { run: { id: string; status: ResearchRunStatusWord } }) {
  const i18n = useI18n()
  const session = getSession()
  const [stopping, setStopping] = createSignal(false)
  const [stopRequested, setStopRequested] = createSignal(false)
  const [stopError, setStopError] = createSignal<string | null>(null)
  // The run's end as the gateway reads it back after a stop that reached no live stream (see `stop`). The stream sends
  // its own end, but a run that ended without a live stream never sends it, so the chip would otherwise stay 'running'.
  const [endedStatus, setEndedStatus] = createSignal<ResearchRunStatusWord | null>(null)
  const status = () => endedStatus() ?? props.run.status

  // Read once, when the run has ended: its pages read and why it stopped. A read that fails adds nothing to the chip.
  const [ended] = createResource(
    () => (status() === 'running' ? false : props.run.id),
    (id: string) => getResearchRun(id),
  )

  const detail = createMemo(() => {
    const info = ended()
    if (!info) return ''
    const parts = [pagesReadLabel(info.pagesRead, i18n.tr)]
    const reason = stopReasonLabel(info.stopReason, i18n.tr)
    if (reason) parts.push(reason)
    return parts.join(' · ')
  })

  const actorReady = () => Boolean(session.user?.id && session.activeOrg?.id)

  // The person's stop. The action names only the run: the gateway checks that the caller owns it, and a run of another
  // owner reads as not found. A refused stop puts the button back, with the reason beside it. A stop the run's live
  // stream did not receive (`stopRequested` false) claims nothing: the chip reads the run again and shows its end, if
  // the gateway holds one.
  const stop = async () => {
    if (!actorReady() || stopping() || stopRequested()) return
    const actor: ActionActor = {
      type: 'human',
      userId: session.user?.id ?? '',
      orgId: session.activeOrg?.id ?? '',
    }
    setStopping(true)
    setStopError(null)
    try {
      const execution = await executeAction('chat.research_run.cancel', actor, { researchRunId: props.run.id })
      if ((execution.result as Partial<ResearchRunCancelAck> | undefined)?.stopRequested === true) {
        setStopRequested(true)
      } else {
        const run = await getResearchRun(props.run.id).catch(() => null)
        if (run && run.status !== 'running') setEndedStatus(run.status)
      }
    } catch (err) {
      setStopError(translateApiError(err, i18n.tr))
    } finally {
      setStopping(false)
    }
  }

  return (
    <div class="verevon-research-run" data-testid="research-run-status" data-status={status()} role="status">
      <span class="verevon-research-run__label">{statusLabels(i18n.tr)[status()]}</span>
      <Show when={detail()}>
        <span class="verevon-research-run__detail">{detail()}</span>
      </Show>
      <Show when={status() === 'running'}>
        <button
          type="button"
          class="verevon-research-run__stop"
          disabled={stopping() || stopRequested() || !actorReady()}
          onClick={() => void stop()}
        >
          {stopping()
            ? i18n.tr('Stopper…', 'Stopping…')
            : stopRequested()
              ? i18n.tr('Stopp forespurt', 'Stop requested')
              : i18n.tr('Stopp research', 'Stop research')}
        </button>
      </Show>
      <Show when={stopError()}>
        <span class="verevon-research-run__error" role="alert">{stopError()}</span>
      </Show>
    </div>
  )
}
