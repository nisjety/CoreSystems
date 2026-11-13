import { afterEach, describe, expect, it, vi } from 'vitest'
import { getActionDescriptor } from './action-registry'
import { LEARN_NOTE_MAX_CHARS, requestLearnFromThread } from '@/shared/api/chat-actions-client'

// W3-EP-11-S7-a: the /learn note is bounded at 500 characters everywhere, counted as the server counts them.
// The BFF (learning.rs: note.trim().chars().count()), model-gateway (chars().count()) and capability-core
// (len([]rune(TrimSpace(note)))) count Unicode code points after trimming, and so does the composer's intercept
// (Array.from(note).length). The action contract says it is "bounded as the server bounds them", so it must
// accept and refuse the same notes.

const input = (note: string) => ({ threadId: 'thread-1', note })

function contract() {
  const descriptor = getActionDescriptor('learning.learn_from_thread')
  if (!descriptor) throw new Error('learning.learn_from_thread is not registered')
  return descriptor.inputSchema
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('the /learn note bound', () => {
  it('is 500, as on the server', () => {
    expect(LEARN_NOTE_MAX_CHARS).toBe(500)
  })

  it('accepts 500 plain characters and refuses 501', () => {
    expect(contract().safeParse(input('a'.repeat(500))).success).toBe(true)
    expect(contract().safeParse(input('a'.repeat(501))).success).toBe(false)
  })

  it('bounds after trimming, as the server does', () => {
    expect(contract().safeParse(input(`  ${'a'.repeat(500)}  `)).success).toBe(true)
  })

  it('counts Norwegian letters as one character each', () => {
    expect(contract().safeParse(input('å'.repeat(500))).success).toBe(true)
  })

  it('counts a character outside the Basic Multilingual Plane as one, as the server does', () => {
    // 500 code points, 1000 UTF-16 code units: the BFF, model-gateway and capability-core all accept this note,
    // and the composer's intercept lets it through.
    const note = '😀'.repeat(500)
    expect(Array.from(note).length).toBe(500)
    expect(contract().safeParse(input(note)).success).toBe(true)
    expect(contract().safeParse(input('😀'.repeat(501))).success).toBe(false)
  })

  it('sends the note trimmed, and no note key for a blank one', async () => {
    const fetchMock = vi
      .fn()
      .mockImplementation(async () => new Response(JSON.stringify({ data: { state: 'queued' } }), { status: 202 }))
    vi.stubGlobal('fetch', fetchMock)
    await requestLearnFromThread('thread-1', '  keep the tone  ')
    await requestLearnFromThread('thread-1', '   ')
    expect(JSON.parse(fetchMock.mock.calls[0]![1].body as string)).toEqual({ threadId: 'thread-1', note: 'keep the tone' })
    expect(JSON.parse(fetchMock.mock.calls[1]![1].body as string)).toEqual({ threadId: 'thread-1' })
  })
})
