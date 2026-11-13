// The chat side of a question that paused its run (WS14 S5). The person answered, or skipped, a question a
// run put to them while it waited (`awaiting_input`). This sends that response ONCE through the question form
// of `chat.run.continue`, so the same run goes on with it. It never sends the continue again on its own: a
// refused or ambiguous continue is read here and reported, and the run is followed by its id (see the contract).
// Nothing here is a grant: an answer is information, and no approval is read or spent.

import { getActionDescriptor } from '@/shared/actions/action-registry'
import {
  chatRunContinueHttpRefusalOf,
  chatRunContinueQuestionRefusalOf,
  type ChatRunContinueQuestionFailure,
} from '@/shared/actions/chat-run-continue-contract'
import type { RunQuestionResponse } from '@/shared/api/orchestration-client'
import type { SseEvent } from '@/shared/api/sse'

/**
 * What became of the response.
 *  - `continued`: the run took it, and its frames are the thread's to show.
 *  - `released`: the run did not take the answer, so the answer is sent as the next message. Only
 *    `question_answer_released` says so.
 *  - `gone`: the question or the run cannot take it any more. Nothing is resent.
 *  - `retry`: a continue for this question is in flight, or the answer was refused as invalid. The card stays.
 *  - `unknown`: the outcome is not known. The run is read by its id, and nothing is resent.
 */
export type QuestionResumeOutcome =
  | { readonly kind: 'continued' }
  | { readonly kind: 'released'; readonly code: string }
  | { readonly kind: 'gone'; readonly code: string }
  | { readonly kind: 'retry'; readonly code: string }
  | { readonly kind: 'unknown'; readonly code?: string }

export type QuestionResumeInput = {
  readonly runId: string
  readonly questionId: string
  readonly response: RunQuestionResponse
}

export type QuestionResumeDeps = {
  continueQuestionRun: (
    runId: string,
    questionId: string,
    response: RunQuestionResponse,
    claimToken: string,
    onEvent: (event: SseEvent) => void,
  ) => Promise<void>
  newClaimToken: () => string
  /** Every frame of the continuation except a question refusal, which only this module reads. */
  onEvent: (event: SseEvent) => void
}

/** The pre-stream refusals that mean the question or the run cannot take the answer any more. */
const GONE_HTTP_CODES: readonly string[] = [
  'question_expired',
  'question_not_found',
  'question_already_answered',
  'question_not_resumable',
  'not_resumable',
  'continue_disabled',
]

function frameOf(data: string): unknown {
  try {
    return JSON.parse(data) as unknown
  } catch {
    return null
  }
}

function outcomeOfQuestionFrame(failure: ChatRunContinueQuestionFailure): QuestionResumeOutcome {
  if (failure.kind === 'released') return { kind: 'released', code: failure.code }
  if (failure.kind === 'gone') return { kind: 'gone', code: failure.code }
  if (failure.kind === 'retry') return { kind: 'retry', code: failure.code }
  return { kind: 'unknown', code: failure.code }
}

/**
 * A failed request. A listed refusal is read by its code (the answer was never taken, or the run cannot take
 * it). Anything else, a transport failure included, does not say whether the answer was taken, so the run is
 * read by its id ([chatRunContinueTransportFailureOf] reads that case, with the same outcome).
 */
function outcomeOfFailure(error: unknown): QuestionResumeOutcome {
  const refusal = chatRunContinueHttpRefusalOf(error)
  if (refusal) {
    if (refusal.code === 'invalid_answer') return { kind: 'retry', code: refusal.code }
    if (GONE_HTTP_CODES.includes(refusal.code)) return { kind: 'gone', code: refusal.code }
    return { kind: 'unknown', code: refusal.code }
  }
  return { kind: 'unknown' }
}

/**
 * Sends the person's answer, or skip, to the run that asked. One continue is sent, and nothing else. A
 * response the action contract refuses is never sent: it reads as `retry` with `invalid_answer`, so the card
 * stays and the person is told the answer was not saved.
 */
export async function answerResumeQuestion(
  input: QuestionResumeInput,
  deps: QuestionResumeDeps,
): Promise<QuestionResumeOutcome> {
  const { runId, questionId, response } = input
  const claimToken = deps.newClaimToken()
  const checked = getActionDescriptor('chat.run.continue')?.inputSchema.safeParse(
    'skip' in response
      ? { runId, claimToken, questionId, questionSkip: true }
      : { runId, claimToken, questionId, answers: response.answers },
  )
  if (!checked?.success) return { kind: 'retry', code: 'invalid_answer' }

  const seen: { outcome: QuestionResumeOutcome | null } = { outcome: null }
  try {
    await deps.continueQuestionRun(runId, questionId, response, claimToken, (event) => {
      // The question refusal ends the continuation and is read here alone: it is never shown as a run frame.
      const failure = chatRunContinueQuestionRefusalOf(frameOf(event.data))
      if (failure) {
        seen.outcome ??= outcomeOfQuestionFrame(failure)
        return
      }
      deps.onEvent(event)
    })
  } catch (error) {
    return seen.outcome ?? outcomeOfFailure(error)
  }
  return seen.outcome ?? { kind: 'continued' }
}
