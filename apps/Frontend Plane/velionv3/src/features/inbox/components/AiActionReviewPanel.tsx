import { Check, Sparkles, X } from 'lucide-solid'
import { createResource, createSignal, For, Show } from 'solid-js'
import {
  listAiActions,
  reviewAiAction,
  type AiAction,
  type AiActionFieldEdits,
} from '@/shared/api/inbox-client'
import { translateApiError, useI18n } from '@/shared/i18n'

const EDITABLE_FIELD_KEYS = ['category', 'intent', 'priority'] as const
type EditableFieldKey = (typeof EDITABLE_FIELD_KEYS)[number]

function fieldLabel(key: EditableFieldKey, tr: (noText: string, enText: string) => string): string {
  switch (key) {
    case 'category':
      return tr('Kategori', 'Category')
    case 'intent':
      return tr('Hensikt', 'Intent')
    case 'priority':
      return tr('Prioritet', 'Priority')
  }
}

function isTicketClassification(action: AiAction): boolean {
  return action.kind === 'ticket_classification' || action.kind === 'ticket.classification'
}

/** conversation-core stores the AI's suggested ticket fields under
 * `suggested_fields` (RecordTicketClassification). `proposed_ticket` is kept
 * as a forward-compat fallback. */
function suggestedFieldsOf(action: AiAction): Record<string, unknown> {
  const payload = action.payload ?? {}
  return (payload.suggested_fields ?? payload.proposed_ticket ?? {}) as Record<string, unknown>
}

function confidenceOf(action: AiAction): number | null {
  const payload = action.payload ?? {}
  return typeof payload.confidence === 'number' ? Math.round(payload.confidence * 100) : null
}

/** Only the fields the upstream actually suggested as non-empty strings —
 * never fabricates an editable input for a field the AI never proposed. */
function editableKeysOf(action: AiAction): EditableFieldKey[] {
  const suggested = suggestedFieldsOf(action)
  return EDITABLE_FIELD_KEYS.filter((key) => {
    const value = suggested[key]
    return typeof value === 'string' && value.trim() !== ''
  })
}

function defaultEditForAction(action: AiAction): AiActionFieldEdits {
  const suggested = suggestedFieldsOf(action)
  const edits: AiActionFieldEdits = {}
  for (const key of editableKeysOf(action)) {
    edits[key] = String(suggested[key])
  }
  return edits
}

/** Label + detail for kinds with nothing editable (e.g. draft.reply), which
 * carry no suggested_fields. */
function summarizeOther(
  action: AiAction,
  tr: (noText: string, enText: string) => string,
): { label: string; detail: string } {
  const label = action.kind.replace(/[._]/g, ' ')
  const confidence = confidenceOf(action)
  const detail = confidence !== null ? tr(`${confidence}% konfidens`, `${confidence}% confidence`) : ''
  return { label, detail }
}

/** HITL review queue for a single conversation: model-proposed actions awaiting a
 * human decision. Renders real items from conversation-core, an explicit empty
 * state when there are none, and lets the reviewer edit the AI's suggested ticket
 * fields inline before approving — nothing executes until that explicit approve,
 * and the (possibly edited) fields travel with the decision. The live executor
 * (cc-go consumer) then promotes + routes the ticket, so the copy reads "Applied". */
export function AiActionReviewPanel(props: { conversationId: string | undefined }) {
  const i18n = useI18n()
  const [actions, { refetch }] = createResource(
    () => props.conversationId,
    // Pending ticket.classification actions carry the classification outcome as
    // their status ("suggest_ticket"); that is the awaiting-human-review state.
    (conversationId) => listAiActions({ conversationId, status: 'suggest_ticket' }),
  )
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [recorded, setRecorded] = createSignal<string | null>(null)
  const [error, setError] = createSignal<string | null>(null)
  const [edits, setEdits] = createSignal<Record<string, AiActionFieldEdits>>({})

  function fieldsFor(action: AiAction): AiActionFieldEdits {
    return edits()[action.id] ?? defaultEditForAction(action)
  }

  function setField(action: AiAction, key: EditableFieldKey, value: string) {
    setEdits((prev) => ({ ...prev, [action.id]: { ...fieldsFor(action), [key]: value } }))
  }

  async function decide(action: AiAction, decision: 'approve' | 'reject') {
    setError(null)
    setRecorded(null)
    setBusyId(action.id)
    try {
      // Reject stays a bare decision — only an approve carries the (possibly
      // edited) suggested fields through, and only for kinds that have any.
      const editedFields =
        decision === 'approve' && isTicketClassification(action) ? fieldsFor(action) : undefined
      await reviewAiAction(action.id, decision, { editedFields })
      setRecorded(
        decision === 'approve'
          ? i18n.tr('Utført — den foreslåtte saken ble forfremmet og rutet.', 'Applied — the suggested ticket was promoted and routed.')
          : i18n.tr('Avgjørelse registrert — forslaget ble avvist.', 'Decision recorded — suggestion dismissed.'),
      )
      await refetch()
    } catch (err) {
      setError(translateApiError(err, i18n.tr, { no: 'Kunne ikke registrere avgjørelsen. Prøv igjen.', en: 'Could not record the decision. Please retry.' }))
    } finally {
      setBusyId(null)
    }
  }

  return (
    <Show when={props.conversationId}>
      <section class="velion-ai-review" aria-label={i18n.tr('AI-forslag som venter på gjennomgang', 'AI suggestions awaiting review')}>
        <header class="velion-ai-review__head">
          <Sparkles size={14} aria-hidden="true" />
          <span>{i18n.tr('AI-forslag', 'AI suggestions')}</span>
        </header>

        <Show
          when={!actions.loading}
          fallback={<p class="velion-ai-review__muted">{i18n.tr('Laster forslag …', 'Loading suggestions…')}</p>}
        >
          <Show
            when={(actions() ?? []).length > 0}
            fallback={
              <p class="velion-ai-review__muted">{i18n.tr('Ingen AI-forslag venter på gjennomgang for denne samtalen.', 'No AI suggestions awaiting review for this conversation.')}</p>
            }
          >
            <ul class="velion-ai-review__list">
              <For each={actions()}>
                {(action) => {
                  const editableKeys = editableKeysOf(action)
                  const confidence = confidenceOf(action)
                  const other = summarizeOther(action, i18n.tr)
                  return (
                    <li class="velion-ai-review__item">
                      <div class="velion-ai-review__body">
                        <span class="velion-ai-review__kind">
                          {isTicketClassification(action) ? i18n.tr('Foreslått support-sak', 'Proposed support ticket') : other.label}
                        </span>
                        <Show
                          when={editableKeys.length > 0}
                          fallback={
                            <Show when={other.detail}>
                              <span class="velion-ai-review__detail">{other.detail}</span>
                            </Show>
                          }
                        >
                          <div class="velion-ai-review__fields">
                            <For each={editableKeys}>
                              {(key) => (
                                <label class="velion-ai-review__field">
                                  <span class="velion-ai-review__field-label">{fieldLabel(key, i18n.tr)}</span>
                                  <input
                                    type="text"
                                    class="velion-ai-review__field-input"
                                    aria-label={i18n.tr(`${fieldLabel(key, i18n.tr)} (redigerbar)`, `${fieldLabel(key, i18n.tr)} (editable)`)}
                                    value={fieldsFor(action)[key] ?? ''}
                                    disabled={busyId() !== null}
                                    onInput={(event) => setField(action, key, event.currentTarget.value)}
                                  />
                                </label>
                              )}
                            </For>
                          </div>
                          <Show when={confidence !== null}>
                            <span class="velion-ai-review__detail">
                              {i18n.tr(
                                `${confidence}% konfidens — rediger feltene over før du godkjenner hvis nødvendig`,
                                `${confidence}% confidence — edit the fields above before approving if needed`,
                              )}
                            </span>
                          </Show>
                        </Show>
                      </div>
                      <div class="velion-ai-review__actions">
                        <button
                          type="button"
                          class="velion-ai-review__btn velion-ai-review__btn--approve"
                          disabled={busyId() !== null}
                          onClick={() => decide(action, 'approve')}
                        >
                          <Check size={14} aria-hidden="true" /> {i18n.tr('Godkjenn', 'Approve')}
                        </button>
                        <button
                          type="button"
                          class="velion-ai-review__btn velion-ai-review__btn--reject"
                          disabled={busyId() !== null}
                          onClick={() => decide(action, 'reject')}
                        >
                          <X size={14} aria-hidden="true" /> {i18n.tr('Avvis', 'Reject')}
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
          {i18n.tr(
            'Å godkjenne forfremmer og ruter den foreslåtte saken med feltene vist over; å avvise forkaster den. Hver avgjørelse blir revidert.',
            'Approving promotes and routes the suggested ticket with the fields shown above; rejecting dismisses it. Every decision is audited.',
          )}
        </p>
      </section>
    </Show>
  )
}
