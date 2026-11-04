/**
 * F-07, the progress half: a long wait has to say what it is doing, and it has
 * to stay silent when it does not know.
 */

import { describe, expect, it } from 'vitest'
import { deriveStreamActivity, streamActivityLabel } from './stream-activity'
import type { ChatToolCall, LifecycleNotice } from '@/features/chat/components/chat-types'

const call = (overrides: Partial<ChatToolCall> = {}): ChatToolCall => ({
  id: 'tc-1',
  name: 'web_search',
  status: 'running',
  ...overrides,
})

/** The two locales the app ships, as its translator reads them. */
const norwegian = { tr: (noText: string) => noText }
const english = { tr: (_noText: string, enText: string) => enText }

const retry = (overrides: Partial<Extract<LifecycleNotice, { kind: 'retry' }>> = {}): LifecycleNotice => ({
  kind: 'retry',
  tool: 'web_search',
  attempt: 2,
  maxAttempts: 3,
  reason: 'timeout',
  callId: 'tc-1',
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

  /** WS15: a retry is named from its notice, and only for the call the notice was matched to. */
  it('names a tool that is being retried, with the attempt about to run', () => {
    expect(deriveStreamActivity([call()], [retry()])).toEqual({
      kind: 'retrying',
      tool: 'Web search',
      attempt: 2,
      max: 3,
    })
  })

  it('does not read a later call of the same tool as retrying', () => {
    const activity = deriveStreamActivity(
      [call({ id: 'tc-1', status: 'done', output: '{}' }), call({ id: 'tc-2' })],
      [retry({ callId: 'tc-1' })],
    )
    expect(activity).toEqual({ kind: 'tool-running', tool: 'Web search' })
  })

  it('ignores a notice that names no call', () => {
    expect(deriveStreamActivity([call()], [retry({ callId: undefined })])).toEqual({
      kind: 'tool-running',
      tool: 'Web search',
    })
  })
})

describe('streamActivityLabel', () => {
  it('says what is running', () => {
    expect(streamActivityLabel({ kind: 'tool-running', tool: 'Web search' }, norwegian)).toBe(
      'Kjører Web search',
    )
    expect(streamActivityLabel({ kind: 'tool-running', tool: 'Web search' }, english)).toBe(
      'Running Web search',
    )
  })

  /**
   * Not "skriver svar": no answer token has been seen, so claiming the model is
   * writing would be exactly the fabricated phase this surface refuses. What is
   * known is that the tool came back and the answer has not.
   */
  it('reports a finished tool as a wait, not as a composing model', () => {
    const label = streamActivityLabel({ kind: 'tool-settled', tool: 'Web search', failed: false }, norwegian)
    expect(label).toBe('Web search fullført – venter på svar')
    expect(label).not.toContain('skriver')
    expect(streamActivityLabel({ kind: 'tool-settled', tool: 'Web search', failed: false }, english)).toBe(
      'Web search done – waiting for answer',
    )
  })

  it('says a tool failed when it did', () => {
    expect(streamActivityLabel({ kind: 'tool-settled', tool: 'Web search', failed: true }, norwegian)).toBe(
      'Web search feilet – venter på svar',
    )
    expect(streamActivityLabel({ kind: 'tool-settled', tool: 'Web search', failed: true }, english)).toBe(
      'Web search failed – waiting for answer',
    )
  })

  it('says a retry with its attempt, in both locales', () => {
    const activity = { kind: 'retrying' as const, tool: 'Web search', attempt: 2, max: 3 }
    expect(streamActivityLabel(activity, norwegian)).toBe('Prøver Web search på nytt (forsøk 2 av 3)')
    expect(streamActivityLabel(activity, english)).toBe('Retrying Web search (attempt 2 of 3)')
  })
})
