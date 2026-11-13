import { describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/api-error'
import type { RunQuestionResponse } from '@/shared/api/orchestration-client'
import type { SseEvent } from '@/shared/api/sse'
import { answerResumeQuestion, type QuestionResumeDeps } from './question-resume'

// W2-WS14-S5-i: the answer to a question that paused its run is sent ONCE through the question form of
// chat.run.continue. Each outcome branch, and that only question_answer_released yields `released` (the one
// case where the chat then resends the answer as a message).

const answer: RunQuestionResponse = { answers: [{ itemId: 'item-1', optionIds: ['opt-a'], text: '' }] }
const skip: RunQuestionResponse = { skip: true }
const input = (response: RunQuestionResponse = answer) => ({ runId: 'run-1', questionId: 'q-1', response })

function frame(data: unknown, event = 'error'): SseEvent {
  return { event, data: typeof data === 'string' ? data : JSON.stringify(data) } as SseEvent
}

/** Deps whose continue plays `frames` to the caller, then resolves, or rejects with `fail`. */
function deps(frames: SseEvent[] = [], fail?: unknown) {
  const onEvent = vi.fn<(event: SseEvent) => void>()
  let tokens = 0
  const continueQuestionRun = vi.fn<QuestionResumeDeps['continueQuestionRun']>(
    async (_runId, _questionId, _response, _claimToken, emit) => {
      for (const item of frames) emit(item)
      if (fail !== undefined) throw fail
    },
  )
  const newClaimToken = vi.fn(() => `claim-${++tokens}`)
  return { onEvent, continueQuestionRun, newClaimToken }
}

const questionFrame = (code: string) => frame({ code, message: 'no', question_continue: true })

describe('answerResumeQuestion', () => {
  it('continues: sends one continue with one fresh claim and passes the run frames through', async () => {
    const chunk = frame({ content: 'Thanks, continuing.' }, 'chunk')
    const d = deps([chunk])
    await expect(answerResumeQuestion(input(), d)).resolves.toEqual({ kind: 'continued' })
    expect(d.continueQuestionRun).toHaveBeenCalledTimes(1)
    expect(d.newClaimToken).toHaveBeenCalledTimes(1)
    expect(d.continueQuestionRun.mock.calls[0]!.slice(0, 4)).toEqual(['run-1', 'q-1', answer, 'claim-1'])
    expect(d.onEvent).toHaveBeenCalledWith(chunk)
  })

  it('continues a skip the same way', async () => {
    const d = deps()
    await expect(answerResumeQuestion(input(skip), d)).resolves.toEqual({ kind: 'continued' })
    expect(d.continueQuestionRun.mock.calls[0]![2]).toEqual(skip)
  })

  it('is released only by question_answer_released, and that frame is never shown as a run frame', async () => {
    const d = deps([questionFrame('question_answer_released')])
    await expect(answerResumeQuestion(input(), d)).resolves.toEqual({
      kind: 'released',
      code: 'question_answer_released',
    })
    expect(d.onEvent).not.toHaveBeenCalled()
  })

  it.each([
    'continuation_already_consumed',
    'question_expired',
    'question_not_found',
    'question_not_resumable',
    'not_resumable',
    'durable_mutation_forbidden',
    'resume_forbidden',
  ])('reads a %s question frame as gone, never released', async (code) => {
    const d = deps([questionFrame(code)])
    await expect(answerResumeQuestion(input(), d)).resolves.toEqual({ kind: 'gone', code })
  })

  it('reads a continue_in_progress question frame as retry', async () => {
    const d = deps([questionFrame('continue_in_progress')])
    await expect(answerResumeQuestion(input(), d)).resolves.toEqual({ kind: 'retry', code: 'continue_in_progress' })
  })

  it('reads an unlisted question frame as unknown, never released', async () => {
    const d = deps([questionFrame('something_new')])
    await expect(answerResumeQuestion(input(), d)).resolves.toEqual({ kind: 'unknown', code: 'something_new' })
  })

  it('does not read question_answer_released from an unmarked error frame', async () => {
    const unmarked = frame({ code: 'question_answer_released', message: 'no' })
    const d = deps([unmarked])
    const outcome = await answerResumeQuestion(input(), d)
    expect(outcome.kind).not.toBe('released')
    expect(d.onEvent).toHaveBeenCalledWith(unmarked)
  })

  it('keeps the first question outcome when the stream then fails', async () => {
    const d = deps([questionFrame('question_answer_released')], new TypeError('stream broke'))
    await expect(answerResumeQuestion(input(), d)).resolves.toEqual({
      kind: 'released',
      code: 'question_answer_released',
    })
  })

  it.each([
    ['question_expired', 410],
    ['question_not_found', 404],
    ['question_already_answered', 409],
    ['question_not_resumable', 409],
    ['not_resumable', 409],
    ['continue_disabled', 409],
  ])('reads a pre-stream %s (%i) as gone, with nothing resent', async (code, status) => {
    const d = deps([], new ApiError('refused', status, code))
    await expect(answerResumeQuestion(input(), d)).resolves.toEqual({ kind: 'gone', code })
    expect(d.continueQuestionRun).toHaveBeenCalledTimes(1)
  })

  it('reads a pre-stream invalid_answer (422) as retry', async () => {
    const d = deps([], new ApiError('refused', 422, 'invalid_answer'))
    await expect(answerResumeQuestion(input(), d)).resolves.toEqual({ kind: 'retry', code: 'invalid_answer' })
  })

  it('reads another listed pre-stream refusal as unknown with its code', async () => {
    const d = deps([], new ApiError('refused', 503, 'continue_unavailable'))
    await expect(answerResumeQuestion(input(), d)).resolves.toEqual({ kind: 'unknown', code: 'continue_unavailable' })
  })

  it.each([
    ['a network failure', new TypeError('Failed to fetch')],
    ['a gateway timeout', new ApiError('timeout', 504, 'gateway_timeout')],
    ['an upstream failure', new ApiError('bad gateway', 502, 'upstream_unavailable')],
    ['a listed code under the wrong status', new ApiError('refused', 500, 'question_expired')],
  ])('reads %s as unknown, never released', async (_label, error) => {
    const d = deps([], error)
    await expect(answerResumeQuestion(input(), d)).resolves.toEqual({ kind: 'unknown' })
    expect(d.continueQuestionRun).toHaveBeenCalledTimes(1)
  })

  it.each([
    ['no answers', { answers: [] } as RunQuestionResponse],
    ['five answers', { answers: Array.from({ length: 5 }, (_, i) => ({ itemId: `i${i}`, optionIds: [], text: '' })) }],
    ['an over-long text', { answers: [{ itemId: 'item-1', optionIds: [], text: 'x'.repeat(2001) }] }],
  ])('never sends a response the action contract refuses (%s)', async (_label, response) => {
    const d = deps()
    await expect(answerResumeQuestion(input(response), d)).resolves.toEqual({ kind: 'retry', code: 'invalid_answer' })
    expect(d.continueQuestionRun).not.toHaveBeenCalled()
  })
})
