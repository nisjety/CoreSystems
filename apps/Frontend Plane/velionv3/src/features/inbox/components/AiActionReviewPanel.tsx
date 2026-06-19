import { Check, Sparkles, X } from 'lucide-solid'
import { createResource, createSignal, For, Show } from 'solid-js'
import { listAiActions, reviewAiAction, type AiAction } from '@/shared/api/inbox-client'

/** Human-readable summary of a proposed action. Only renders fields the upstream
 * actually provided — never fabricates a confidence, category, or priority. */
function summarize(action: AiAction): { label: string; detail: string } {
  const payload = action.payload ?? {}
  const proposed = (payload.proposed_ticket ?? {}) as Record<string, unknown>
  const label =
    action.kind === 'ticket_classification' || action.kind === 'ticket.classification'
      ? 'Proposed support ticket'
      : action.kind.replace(/[._]/g, ' ')

  const category = typeof proposed.category === 'string' ? proposed.category : ''
  const intent = typeof proposed.intent === 'string' ? proposed.intent : ''
  const priority = typeof proposed.priority === 'string' ? proposed.priority : ''
  const confidence = typeof payload.confidence === 'number' ? Math.round(payload.confidence * 100) : null

  const parts = [category, intent && intent !== category ? intent : '', priority ? `${priority} priority` : '']
    .filter(Boolean)
    .join(' · ')
  const detail = [parts, confidence !== null ? `${confidence}% confidence` : ''].filter(Boolean).join(' — ')
  return { label, detail }
}

/** HITL review queue for a single conversation: model-proposed actions awaiting a
 * human decision. Renders real items from conversation-core, an explicit empty
 * state when there are none, and records (never executes) the human's decision. */
export function AiActionReviewPanel(props: { conversationId: string | undefined }) {
  const [actions, { refetch }] = createResource(
    () => props.conversationId,
    (conversationId) => listAiActions({ conversationId, status: 'suggested' }),
  )
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [recorded, setRecorded] = createSignal<string | null>(null)
  const [error, setError] = createSignal<string | null>(null)

  async function decide(action: AiAction, decision: 'approve' | 'reject') {
    setError(null)
    setRecorded(null)
    setBusyId(action.id)
    try {
      await reviewAiAction(action.id, decision)
      setRecorded(`Decision recorded — ${decision === 'approve' ? 'approved' : 'rejected'}.`)
      await refetch()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not record the decision. Please retry.')
    } finally {
      setBusyId(null)
    }
  }

  return (
    <Show when={props.conversationId}>
      <section class="velion-ai-review" aria-label="AI suggestions awaiting review">
        <header class="velion-ai-review__head">
          <Sparkles size={14} aria-hidden="true" />
          <span>AI suggestions</span>
        </header>

        <Show
          when={!actions.loading}
          fallback={<p class="velion-ai-review__muted">Loading suggestions…</p>}
        >
          <Show
            when={(actions() ?? []).length > 0}
            fallback={
              <p class="velion-ai-review__muted">No AI suggestions awaiting review for this conversation.</p>
            }
          >
            <ul class="velion-ai-review__list">
              <For each={actions()}>
                {(action) => {
                  const view = summarize(action)
                  return (
                    <li class="velion-ai-review__item">
                      <div class="velion-ai-review__body">
                        <span class="velion-ai-review__kind">{view.label}</span>
                        <Show when={view.detail}>
                          <span class="velion-ai-review__detail">{view.detail}</span>
                        </Show>
                      </div>
                      <div class="velion-ai-review__actions">
                        <button
                          type="button"
                          class="velion-ai-review__btn velion-ai-review__btn--approve"
                          disabled={busyId() !== null}
                          onClick={() => decide(action, 'approve')}
                        >
                          <Check size={14} aria-hidden="true" /> Approve
                        </button>
                        <button
                          type="button"
                          class="velion-ai-review__btn velion-ai-review__btn--reject"
                          disabled={busyId() !== null}
                          onClick={() => decide(action, 'reject')}
                        >
                          <X size={14} aria-hidden="true" /> Reject
                        </button>
                      </div>
                    </li>
                  )
                }}
              </For>
            </ul>
          </Show>
        </Show>

        <Show when={recorded()}>
          <p class="velion-ai-review__recorded" role="status">{recorded()}</p>
        </Show>
        <Show when={error()}>
          <p class="velion-ai-review__error" role="alert">{error()}</p>
        </Show>
        <p class="velion-ai-review__note">
          Decisions are recorded for audit. Approved actions are not auto-executed yet.
        </p>
      </section>
    </Show>
  )
}
