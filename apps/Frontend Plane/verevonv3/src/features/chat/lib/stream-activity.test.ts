/**
 * F-07, the progress half: a long wait has to say what it is doing, and it has
 * to stay silent when it does not know.
 */

import { describe, expect, it } from 'vitest'
import { deriveStreamActivity, streamActivityLabel } from './stream-activity'
import type { ChatToolCall } from '@/features/chat/components/chat-types'

const call = (overrides: Partial<ChatToolCall> = {}): ChatToolCall => ({
  id: 'tc-1',
  name: 'web_search',
  status: 'running',
  ...overrides,
})

describe('deriveStreamActivity', () => {
  /**
   * The turn F-07 was filed against. The subscription route streams an answer
   * and nothing else — no tool call ever arrives — so there is no activity to
   * name, and inventing one ("Analyserer …", a moving bar) would be a claim
   * about work nobody observed. Elapsed time alone is the honest display.
   */
  it('reports nothing when the stream has reported nothing', () => {
    expect(deriveStreamActivity(undefined)).toBeNull()
    expect(deriveStreamActivity([])).toBeNull()
  })

  it('names the tool that is still out', () => {
    expect(deriveStreamActivity([call()])).toEqual({ kind: 'tool-running', tool: 'Web search' })
  })

  it('treats a call with no status at all as still out', () => {
    // A tool call is only ever settled by its result event; a missing status is
    // an older frame, not a finished call. Same rule `StepsPill` applies.
    expect(deriveStreamActivity([call({ status: undefined })])).toMatchObject({
      kind: 'tool-running',
    })
  })

  it('names the newest running tool when several are out at once', () => {
    const activity = deriveStreamActivity([
      call({ id: 'tc-1', name: 'web_search', status: 'done', output: '{}' }),
      call({ id: 'tc-2', name: 'knowledge_search' }),
      call({ id: 'tc-3', name: 'yr_weather' }),
    ])
    expect(activity).toEqual({ kind: 'tool-running', tool: 'Yr weather' })
  })

  it('falls back to the last settled call once nothing is out', () => {
    const activity = deriveStreamActivity([
      call({ id: 'tc-1', name: 'web_search', status: 'done' }),
      call({ id: 'tc-2', name: 'knowledge_search', status: 'ok' }),
    ])
    // `ok` is one of the status strings a backend result carries; "settled" is
    // the presence of a result, not membership of an end-state list.
    expect(activity).toEqual({ kind: 'tool-settled', tool: 'Knowledge search', failed: false })
  })

  it('keeps a failure visible rather than implying progress', () => {
    expect(deriveStreamActivity([call({ status: 'error', error: 'timeout' })])).toEqual({
      kind: 'tool-settled',
      tool: 'Web search',
      failed: true,
    })
  })
})

describe('streamActivityLabel', () => {
  it('says what is running', () => {
    expect(streamActivityLabel({ kind: 'tool-running', tool: 'Web search' })).toBe(
      'Kjører Web search',
    )
  })

  /**
   * Not "skriver svar": no answer token has been seen, so claiming the model is
   * writing would be exactly the fabricated phase this surface refuses. What is
   * known is that the tool came back and the answer has not.
   */
  it('reports a finished tool as a wait, not as a composing model', () => {
    const label = streamActivityLabel({ kind: 'tool-settled', tool: 'Web search', failed: false })
    expect(label).toBe('Web search fullført – venter på svar')
    expect(label).not.toContain('skriver')
  })

  it('says a tool failed when it did', () => {
    expect(streamActivityLabel({ kind: 'tool-settled', tool: 'Web search', failed: true })).toBe(
      'Web search feilet – venter på svar',
    )
  })
})
