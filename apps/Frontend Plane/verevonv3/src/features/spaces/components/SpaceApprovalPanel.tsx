import { Loader2, ShieldCheck } from '@/shared/icons'
import { createEffect, createSignal, For, Show } from 'solid-js'

import type { Approval, ApprovalDecision } from '@/shared/api/orchestration-client'
import { announce } from '@/shared/a11y/announcer'
import { useI18n } from '@/shared/i18n'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  describeApproval,
  readPendingApprovals,
  settleApproval,
  type ApprovalOutcome,
} from '../lib/space-approvals'

/**
 * The decision a paused run is waiting for, shown where the waiting happened.
 *
 * The room already knew a run was `awaiting_approval` — the pulse rail counted
 * it — but the only place to act was the Agent Run Console, on another page.
 * Counting something a person must do and then sending them elsewhere to do it
 * is the "assistant in a side panel" shape `space-defenition.md` argues
 * against, with the room on the losing side of it.
 *
 * # Two verbs, not four
 *
 * Every reference product offers some variant of *allow once / allow for this
 * session / always allow / deny*. Model Plane records one decision for one
 * approval and has no scope to remember a standing answer, so a persistent
 * "always allow" here would be a button that quietly does the same thing as
 * "allow once" while promising more. Approve and Deny are what the mechanism
 * actually supports, so they are what the room offers.
 *
 * # The card says what approving does
 *
 * A grant records authority; it does not resume an approval-gated run. What
 * follows depends on the approval, and the server states it (`resumeClass`,
 * then `continuation` once granted): an action the worker can run is run as
 * the requester, anything else lets the agent retry that exact action once.
 * The copy below follows those fields and never promises the run continues.
 *
 * # It does not pretend
 *
 * A run belongs to the member who started it. Another member can see that a
 * decision is outstanding and still not be the one who may make it, so a
 * refusal renders as a sentence about who decides — not as an error, and not
 * by hiding the card, which would make the room look idle while it waits.
 */
export interface SpaceApprovalPanelProps {
  /** The paused run. */
  readonly runId: string
  /** Called once a decision settles, so the room can re-read its projection. */
  readonly onSettled?: () => void
}

export function SpaceApprovalPanel(props: SpaceApprovalPanelProps) {
  const i18n = useI18n()
  const [reloadKey, setReloadKey] = createSignal(0)
  const [pending, { refetch }] = createResource(
    () => `${props.runId}\u0000${reloadKey()}`,
    async (key) => {
      const runId = key.split('\u0000')[0] ?? ''
      return readPendingApprovals(runId)
    },
  )
  const [decidingId, setDecidingId] = createSignal<string | undefined>(undefined)
  const [outcome, setOutcome] = createSignal<ApprovalOutcome | undefined>(undefined)

  const approvals = () => (pending.error ? [] : pending()?.approvals ?? [])
  const refused = () => (pending.error ? false : pending()?.refused === true)

  // Tell a screen-reader user that a decision is waiting for them, once per run. The room's own
  // narration uses the same key, so the two collapse into a single announcement.
  let announcedFor: string | undefined
  createEffect(
    () => ({ runId: props.runId, waiting: !pending.loading && !pending.error && !refused() && approvals().length > 0 }),
    ({ runId, waiting }) => {
      if (!waiting || announcedFor === runId) return
      announcedFor = runId
      announce(i18n.tr('Venter på din godkjenning', 'Waiting for your approval'), {
        politeness: 'assertive',
        key: `approval:${runId}`,
        coalesceMs: 10_000,
      })
    },
  )

  async function decide(approval: Approval, decision: ApprovalDecision) {
    if (decidingId()) return
    setDecidingId(approval.id)
    setOutcome(undefined)
    try {
      const settled = await settleApproval({
        approvalId: approval.id,
        runId: props.runId,
        decision,
      })
      setOutcome(settled)
      // Re-read rather than assume: the card leaves only when the server says
      // it is gone.
      setReloadKey((value) => value + 1)
      await refetch().catch(() => undefined)
      props.onSettled?.()
    } finally {
      setDecidingId(undefined)
    }
  }

  return (
    <section class="verevon-room-approval" aria-labelledby={`space-approval-${props.runId}`}>
      <div class="verevon-room-approval__heading">
        <span class="verevon-room-approval__icon" aria-hidden="true">
          <ShieldCheck size={15} />
        </span>
        <h4 id={`space-approval-${props.runId}`}>
          {i18n.tr('Venter på din godkjenning', 'Waiting for your approval')}
        </h4>
      </div>

      <Show when={pending.loading}>
        <p class="verevon-space-inline-status" role="status">
          {i18n.tr('Henter forespørselen …', 'Loading the request…')}
        </p>
      </Show>

      {/* An unreachable Model Plane is not "nothing to approve". */}
      <Show when={pending.error}>
        <p class="verevon-space-projection-error" role="alert">
          {i18n.tr(
            'Vi fikk ikke hentet hva som venter på godkjenning. Kjøringen står fortsatt på pause.',
            'We could not load what is waiting for approval. The run is still paused.',
          )}
        </p>
      </Show>

      <Show when={refused()}>
        <p class="verevon-room-approval__note">
          {i18n.tr(
            'Venter på medlemmet som startet kjøringen. Du ser at den venter, men kan ikke svare for dem.',
            'Waiting for the member who started the run. You can see that it is waiting, but you cannot answer for them.',
          )}
        </p>
      </Show>

      <Show when={!pending.loading && !pending.error && !refused() && approvals().length === 0}>
        <p class="verevon-room-approval__note">
          {i18n.tr(
            'Ingenting venter på godkjenning lenger.',
            'Nothing is waiting for approval any more.',
          )}
        </p>
      </Show>

      <ul class="verevon-room-approval__list">
        <For each={approvals()}>
          {(approval) => (
            <li class="verevon-room-approval__item">
              <Show when={approval.actionLabel?.trim()}>
                {(label) => <p class="verevon-room-approval__detail">{label()}</p>}
              </Show>
              <p class="verevon-room-approval__detail">{describeApproval(approval, i18n.tr)}</p>
              <Show when={approval.requestedBy?.trim()}>
                {(requestedBy) => (
                  <p class="verevon-room-approval__requester">
                    {i18n.tr('Bedt om av ', 'Requested by ')}
                    {requestedBy()}
                  </p>
                )}
              </Show>
              <div class="verevon-room-approval__actions">
                <button
                  type="button"
                  class="verevon-room-approval__approve"
                  disabled={decidingId() !== undefined}
                  onClick={() => void decide(approval, 'approve')}
                >
                  <Show
                    when={decidingId() === approval.id}
                    fallback={i18n.tr('Godkjenn', 'Approve')}
                  >
                    <Loader2 size={14} aria-hidden="true" />
                    {i18n.tr('Registrerer …', 'Recording…')}
                  </Show>
                </button>
                <button
                  type="button"
                  class="verevon-room-approval__deny"
                  disabled={decidingId() !== undefined}
                  onClick={() => void decide(approval, 'reject')}
                >
                  {i18n.tr('Avslå', 'Deny')}
                </button>
              </div>
              {/* Said once, next to the buttons, because denying stops the run
                  rather than nudging the agent to try something else. What approving
                  does is chosen by the server's `resumeClass`, never by static copy. */}
              <p class="verevon-room-approval__consequence">
                {consequenceMessage(approval, i18n.tr)}
              </p>
            </li>
          )}
        </For>
      </ul>

      <Show when={outcome()}>
        {(settled) => (
          <p
            class={
              settled() === 'unconfirmed'
                ? 'verevon-space-projection-error'
                : 'verevon-room-approval__note'
            }
            role={settled() === 'unconfirmed' ? 'alert' : 'status'}
          >
            {outcomeMessage(settled(), i18n.tr)}
          </p>
        )}
      </Show>
    </section>
  )
}

/**
 * What approving this card does, from the server-stated resume class. An approval with no stated
 * class says only what is certain: the decision is recorded and denying stops the run.
 */
function consequenceMessage(
  approval: Approval,
  tr: (no: string, en: string) => string,
): string {
  switch (approval.resumeClass) {
    case 'action_continuation':
      return tr(
        'Godkjenning kjører denne handlingen som medlemmet som ba om den. Agenten fortsetter ikke av seg selv. Avslag stopper kjøringen.',
        'Approving runs this action as the member who asked for it. The agent does not continue by itself. Denying stops the run.',
      )
    case 'none':
      return tr(
        'Godkjenning lar agenten prøve akkurat denne handlingen på nytt én gang. Avslag stopper kjøringen.',
        'Approving lets the agent retry this exact action once. Denying stops the run.',
      )
    case 'live_poll':
      return tr(
        'Godkjenning lar det kjørende nettleser-steget gå videre. Avslag stopper det.',
        'Approving lets the running browser step go ahead. Denying stops it.',
      )
    default:
      return tr(
        'Godkjenning registrerer avgjørelsen din. Avslag stopper kjøringen.',
        'Approving records your decision. Denying stops the run.',
      )
  }
}

function outcomeMessage(
  outcome: ApprovalOutcome,
  tr: (no: string, en: string) => string,
): string {
  switch (outcome) {
    case 'granted':
      return tr('Godkjent. Avgjørelsen er registrert.', 'Approved. The decision is recorded.')
    case 'granted_action':
      return tr(
        'Godkjent. Handlingen kjøres som medlemmet som ba om den. Agenten fortsetter ikke av seg selv.',
        'Approved. The action runs as the member who asked for it. The agent does not continue by itself.',
      )
    case 'granted_retry':
      return tr(
        'Godkjent. Ingenting kjøres nå: agenten kan prøve akkurat denne handlingen på nytt én gang.',
        'Approved. Nothing runs now: the agent may retry this exact action once.',
      )
    case 'granted_live':
      return tr(
        'Godkjent. Det kjørende nettleser-steget går videre.',
        'Approved. The running browser step goes ahead.',
      )
    case 'granted_continue':
      return tr(
        'Godkjent. Handlingen brukes når kjøringen fortsetter.',
        'Approved. The action is applied when the run is continued.',
      )
    case 'denied':
      return tr('Avslått. Kjøringen er stoppet.', 'Denied. The run has been stopped.')
    case 'already_decided':
      return tr(
        'Denne var allerede avgjort — sannsynligvis av noen andre.',
        'This one was already decided — most likely by someone else.',
      )
    case 'refused':
      return tr(
        'Du kan ikke avgjøre denne. Den venter på medlemmet som startet kjøringen.',
        'You cannot decide this one. It is waiting for the member who started the run.',
      )
    case 'unconfirmed':
    default:
      // Deliberately does not invite a retry: the decision may have landed, and
      // deciding twice is worse than waiting to read the real state.
      return tr(
        'Vi fikk ikke bekreftet om avgjørelsen ble registrert. Sjekk statusen før du prøver igjen.',
        'We could not confirm whether the decision was recorded. Check the status before trying again.',
      )
  }
}
