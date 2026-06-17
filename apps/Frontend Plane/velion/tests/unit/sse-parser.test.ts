import { describe, expect, it } from 'vitest'

import { parseJsonSseData, parseSseChunk } from '@/lib/sse/parser'
import { encodeSseFrame } from '@/lib/sse/server'

describe('SSE parser', () => {
  it('preserves partial frames across chunks', () => {
    const first = parseSseChunk('', 'id: 1\nevent: message\ndata: {"a"')
    expect(first.events).toHaveLength(0)
    expect(first.buffer).toContain('{"a"')

    const second = parseSseChunk(first.buffer, ':1}\n\n')
    expect(second.events).toHaveLength(1)
    expect(second.events[0]).toMatchObject({ id: '1', event: 'message', data: '{"a":1}' })
    expect(parseJsonSseData<{ a: number }>(second.events[0])?.a).toBe(1)
  })

  it('round-trips multiline data frames', () => {
    const frame = encodeSseFrame('line one\nline two', { id: 'abc', event: 'note' })
    const parsed = parseSseChunk('', frame)

    expect(parsed.events[0]).toEqual({
      id: 'abc',
      event: 'note',
      data: 'line one\nline two',
    })
  })
})
