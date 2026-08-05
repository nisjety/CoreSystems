import { A } from '@solidjs/router'
import { BarChart3, Bot, ExternalLink, Sparkles } from 'lucide-solid'
import { createMemo, createResource, For, Show } from 'solid-js'
import { listAiActions, type AiAction } from '@/shared/api/inbox-client'
import { translateApiError, useI18n } from '@/shared/i18n'
import { deriveReviewDecisionTiming, formatReviewTiming } from '@/features/support/lib/ai-review-outcomes'

function kindLabel(action: Pick<AiAction, 'kind'>, tr: (noText: string, enText: string) => string): string {
  switch (action.kind) {
    case 'draft.reply': return tr('Foreslått svar', 'Proposed reply')
    case 'internal.note': return tr('Foreslått internt notat', 'Proposed internal note')
    case 'ticket.update': return tr('Foreslått saksoppdatering', 'Proposed ticket update')
    case 'ticket.classification':
    case 'ticket_classification': return tr('Foreslått support-sak', 'Proposed support ticket')
    case 'incident.create': return tr('Foreslått hendelse', 'Proposed incident')
    case 'problem.create': return tr('Foreslått problem', 'Proposed Problem')
    default: return action.kind.replace(/[._]/g, ' ')
  }
}

function proposalPreview(action: AiAction): string {
  const payload = action.payload ?? {}
  const body = payload.body_text
  if (typeof body === 'string' && body.trim()) return body.trim().slice(0, 180)
  const title = payload.title
  if (typeof title === 'string' && title.trim()) return title.trim().slice(0, 180)
  const fields = payload.suggested_fields
  if (fields && typeof fields === 'object' && !Array.isArray(fields)) {
    return Object.entries(fields as Record<string, unknown>)
      .filter(([, value]) => typeof value === 'string' || typeof value === 'number')
      .slice(0, 3)
      .map(([key, value]) => `${key.replace(/_/g, ' ')}: ${value}`)
      .join(' · ')
  }
  const summary = payload.summary
  return typeof summary === 'string' ? summary.trim().slice(0, 180) : ''
}

type ReviewOutcomes = {
  approvedOrExecuted: number
  declined: number
  failedAfterApproval: number
  awaitingDecision: number
}

type ProposalMixEntry = {
  kind: string
  count: number
}

function reviewOutcomes(actions: readonly AiAction[]): ReviewOutcomes {
  return actions.reduce<ReviewOutcomes>((totals, action) => {
    switch (action.status) {
      case 'approved':
      case 'executed':
        return { ...totals, approvedOrExecuted: totals.approvedOrExecuted + 1 }
      case 'rejected':
        return { ...totals, declined: totals.declined + 1 }
      case 'failed':
        return { ...totals, failedAfterApproval: totals.failedAfterApproval + 1 }
      case 'suggested':
      case 'suggest_ticket':
        return { ...totals, awaitingDecision: totals.awaitingDecision + 1 }
      default:
        return totals
    }
  }, { approvedOrExecuted: 0, declined: 0, failedAfterApproval: 0, awaitingDecision: 0 })
}

/** A bounded, ledger-derived mix of the proposals this organization actually
 * reviewed. This is deliberately a count, not an inferred quality score. */
function proposalMix(actions: readonly AiAction[]): readonly ProposalMixEntry[] {
  const countByKind = actions.reduce<Readonly<Record<string, number>>>((totals, action) => ({
    ...totals,
    [action.kind]: (totals[action.kind] ?? 0) + 1,
  }), {})

  return Object.entries(countByKind)
    .map(([kind, count]) => ({ kind, count }))
    .sort((left, right) => right.count - left.count || left.kind.localeCompare(right.kind))
}

/** Cross-conversation review intake. It deliberately links into the source
 * conversation instead of approving from a context-free queue, ensuring the
 * reviewer sees the exact payload, evidence, and customer thread first. */
export function AiReviewQueue() {
  const i18n = useI18n()
  const [actions] = createResource(() => listAiActions({ status: 'review', limit: 100 }))
  const [recentActions] = createResource(() => listAiActions({ status: 'all', limit: 100 }))
  const pending = createMemo(() => actions() ?? [])
  const outcomes = createMemo(() => reviewOutcomes(recentActions() ?? []))
  const actionMix = createMemo(() => proposalMix(recentActions() ?? []))
  const decisionTiming = createMemo(() => deriveReviewDecisionTiming(recentActions() ?? []))

  return (
    <main class="verevon-ai-review-queue" aria-label={i18n.tr('AI-gjennomgang', 'AI review')}>
      <header class="verevon-ai-review-queue__header">
        <div>
          <div class="verevon-ai-review-queue__eyebrow"><Sparkles class="size-4" /> {i18n.tr('AI-gjennomgang', 'AI review')}</div>
          <h1>{i18n.tr('Forslag som trenger beslutning', 'Proposals that need a decision')}</h1>
          <p>{i18n.tr('Åpne hvert forslag i den opprinnelige samtalen før du godkjenner eller avviser det.', 'Open each proposal in its source conversation before approving or rejecting it.')}</p>
        </div>
      </header>

      <Show when={!recentActions.loading && !recentActions.error}>
        <section class="verevon-ai-review-outcomes" aria-label={i18n.tr('Nylige AI-gjennomgangsutfall', 'Recent AI review outcomes')}>
          <div class="verevon-ai-review-outcomes__header">
            <BarChart3 class="size-4" />
            <div>
              <h2>{i18n.tr('Nylige AI-gjennomgangsutfall', 'Recent AI review outcomes')}</h2>
              <p>{i18n.tr('Basert på de siste 100 handlingene i denne organisasjonens handlingslogg.', 'Based on the latest 100 actions in this organization’s action ledger.')}</p>
            </div>
          </div>
          <dl class="verevon-ai-review-outcomes__metrics">
            <div><dt>{i18n.tr('Godkjent eller utført', 'Approved or executed')}</dt><dd>{outcomes().approvedOrExecuted}</dd></div>
            <div><dt>{i18n.tr('Avvist', 'Declined')}</dt><dd>{outcomes().declined}</dd></div>
            <div><dt>{i18n.tr('Feilet etter godkjenning', 'Failed after approval')}</dt><dd>{outcomes().failedAfterApproval}</dd></div>
            <div><dt>{i18n.tr('Venter på beslutning', 'Awaiting decision')}</dt><dd>{outcomes().awaitingDecision}</dd></div>
          </dl>
        </section>
        <Show when={actionMix().length > 0}>
          <section class="verevon-ai-review-outcomes verevon-ai-review-action-mix" aria-label={i18n.tr('Forslagsmiks', 'Proposal mix')}>
            <div class="verevon-ai-review-outcomes__header">
              <BarChart3 class="size-4" />
              <div>
                <h2>{i18n.tr('Forslagsmiks', 'Proposal mix')}</h2>
                <p>{i18n.tr('Typer forslag blant de samme 100 registrerte AI-handlingene.', 'Proposal types among the same 100 recorded AI actions.')}</p>
              </div>
            </div>
            <dl class="verevon-ai-review-outcomes__metrics">
              <For each={actionMix()}>
                {(entry) => <div><dt>{kindLabel(entry, i18n.tr)}</dt><dd>{entry.count}</dd></div>}
              </For>
            </dl>
          </section>
        </Show>
        <section class="verevon-ai-review-outcomes verevon-ai-review-decision-timing" aria-label={i18n.tr('Tid til registrert beslutning', 'Recorded decision timing')}>
          <div class="verevon-ai-review-outcomes__header">
            <BarChart3 class="size-4" />
            <div>
              <h2>{i18n.tr('Tid til registrert beslutning', 'Recorded decision timing')}</h2>
              <p>{i18n.tr('Median fra forslag ble opprettet til en registrert beslutning eller et utfall.', 'Median from proposal creation to a recorded decision or outcome.')}</p>
            </div>
          </div>
          <dl class="verevon-ai-review-outcomes__metrics">
            <div><dt>{i18n.tr('Median tid', 'Median time')}</dt><dd>{formatReviewTiming(decisionTiming().medianMilliseconds)}</dd></div>
            <div><dt>{i18n.tr('Målte handlinger', 'Measured actions')}</dt><dd>{decisionTiming().measuredActions}</dd></div>
          </dl>
          <p class="verevon-ai-review-decision-timing__disclosure">{i18n.tr('Måler bare tidspunkt i handlingsloggen. Det er ikke leveringstid, løsningstid eller en kvalitetsvurdering.', 'Uses only action-ledger timestamps. It is not delivery time, resolution time, or a quality score.')}</p>
        </section>
      </Show>
      <Show when={!recentActions.loading && recentActions.error}>
        <p class="verevon-ai-review-outcomes__unavailable">{i18n.tr('Nylige AI-utfall er utilgjengelige. Dette påvirker ikke gjennomgangskøen.', 'Recent AI outcomes are unavailable. This does not affect the review queue.')}</p>
      </Show>

      <Show when={!actions.loading} fallback={<p class="verevon-ai-review-queue__state">{i18n.tr('Laster forslag …', 'Loading proposals…')}</p>}>
        <Show when={!actions.error} fallback={<p role="alert" class="verevon-ai-review-queue__state verevon-ai-review-queue__state--error">{translateApiError(actions.error, i18n.tr, { no: 'Kunne ikke laste gjennomgangskøen. Prøv igjen.', en: 'Could not load the review queue. Please try again.' })}</p>}>
          <Show when={pending().length > 0} fallback={<div class="verevon-ai-review-queue__empty"><Bot class="size-6" /><h2>{i18n.tr('Ingenting venter på gjennomgang', 'Nothing is waiting for review')}</h2><p>{i18n.tr('Nye AI-forslag vises her når de trenger en menneskelig beslutning.', 'New AI proposals appear here when they need a human decision.')}</p></div>}>
            <ul class="verevon-ai-review-queue__list">
              <For each={pending()}>
                {(action) => (
                  <li class="verevon-ai-review-queue__item" data-ai-action-id={action.id}>
                    <div>
                      <strong>{kindLabel(action, i18n.tr)}</strong>
                      <Show when={proposalPreview(action)}>{(preview) => <p>{preview()}</p>}</Show>
                      <small>{i18n.tr('Samtale', 'Conversation')} · {action.conversation_id}</small>
                    </div>
                    <A href={`/support?view=all&conversation_id=${encodeURIComponent(action.conversation_id)}`} class="verevon-ai-review-queue__open-link">
                      {i18n.tr('Gjennomgå i kontekst', 'Review in context')} <ExternalLink class="size-4" />
                    </A>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </Show>
      </Show>
    </main>
  )
}
