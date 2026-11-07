import { createSignal, For, Show } from 'solid-js'
import { ThumbsDown, ThumbsUp } from '@/shared/icons'
import { describeFeedbackFailure, submitRunFeedback, type ChatFeedbackRating } from '@/shared/api/chat-client'
import { useI18n } from '@/shared/i18n'
import { feedbackReasonLabels } from '../lib/feedback-reasons'
import './ResultFeedback.css'

/**
 * Thumbs and reason chips for a durable run's result: an agent run's answer and a
 * scheduled run's fire. A chat answer is rated through its turn (ChatMessages). These
 * results have only a run id, so the rating goes by run id.
 *
 * Behind `VITE_AGENT_RESULT_FEEDBACK`, read when the control renders. Off, the default
 * and never set in this build, it renders nothing. A reason is offered only once a
 * thumbs-down is stored, so a refused rating leaves no note to send. Under Zero Data
 * Retention the gateway refuses the rating (412), and the control says so.
 */

/** Only the literal `true` turns the control on, as for the other switches. */
export function resultFeedbackEnabled(): boolean {
  return import.meta.env.VITE_AGENT_RESULT_FEEDBACK === 'true'
}

export function ResultFeedback(props: { runId: string }) {
  const i18n = useI18n()
  const [rating, setRating] = createSignal<ChatFeedbackRating | null>(null)
  const [reason, setReason] = createSignal<string | null>(null)
  const [reasonsOpen, setReasonsOpen] = createSignal(false)
  const [pending, setPending] = createSignal(false)
  const [notice, setNotice] = createSignal<string | null>(null)

  /** Stores one rating for the run. False, with the reason shown, when it did not stick. */
  const store = async (next: ChatFeedbackRating, note?: string): Promise<boolean> => {
    if (pending()) return false
    setPending(true)
    setNotice(null)
    try {
      await submitRunFeedback(props.runId, next, note)
      return true
    } catch (error: unknown) {
      setNotice(describeFeedbackFailure(error, i18n.tr))
      return false
    } finally {
      setPending(false)
    }
  }

  const rateUp = async () => {
    if (rating() === 'positive') return
    if (!(await store('positive'))) return
    setRating('positive')
    setReason(null)
    setReasonsOpen(false)
  }

  const rateDown = async () => {
    if (rating() === 'negative') {
      setReasonsOpen((open) => !open)
      return
    }
    if (!(await store('negative'))) return
    setRating('negative')
    setReason(null)
    setReasonsOpen(true)
  }

  const pickReason = async (label: string) => {
    if (!(await store('negative', label))) return
    setReason(label)
    setReasonsOpen(false)
  }

  return (
    <Show when={resultFeedbackEnabled()}>
      <div class="verevon-result-feedback" role="group" aria-label={i18n.tr('Vurder svaret', 'Rate this answer')}>
        <button
          type="button"
          class={{ 'verevon-chat-action-button': true, 'verevon-chat-action-button--active': rating() === 'positive' }}
          aria-pressed={rating() === 'positive' ? 'true' : 'false'}
          aria-label={i18n.tr('Godt svar', 'Good response')}
          title={i18n.tr('Godt svar', 'Good response')}
          disabled={pending()}
          onClick={() => void rateUp()}
        >
          <ThumbsUp size={14} />
        </button>
        <button
          type="button"
          class={{ 'verevon-chat-action-button': true, 'verevon-chat-action-button--active': rating() === 'negative' }}
          aria-pressed={rating() === 'negative' ? 'true' : 'false'}
          aria-label={i18n.tr('Dårlig svar', 'Bad response')}
          title={i18n.tr('Dårlig svar', 'Bad response')}
          disabled={pending()}
          onClick={() => void rateDown()}
        >
          <ThumbsDown size={14} />
        </button>
        <Show when={reasonsOpen()}>
          <div class="verevon-chat-feedback-reasons" role="group" aria-label={i18n.tr('Hva var galt?', 'What was wrong?')}>
            <span>{i18n.tr('Hva var galt?', 'What was wrong?')}</span>
            <For each={feedbackReasonLabels(i18n)}>
              {(label) => (
                <button
                  type="button"
                  class="verevon-chat-feedback-reasons__chip"
                  disabled={pending()}
                  onClick={() => void pickReason(label)}
                >
                  {label}
                </button>
              )}
            </For>
          </div>
        </Show>
        <Show when={reason()}>
          {(label) => (
            <p class="verevon-chat-feedback-reasons__note" role="status">
              {i18n.tr('Takk — merket som', 'Thanks — flagged as')} “{label()}”.
            </p>
          )}
        </Show>
        <Show when={notice()}>
          {(message) => (
            <p class="verevon-chat-feedback-reasons__note" role="alert">
              {message()}
            </p>
          )}
        </Show>
      </div>
    </Show>
  )
}
