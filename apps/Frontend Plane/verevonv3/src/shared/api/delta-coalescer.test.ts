// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { streamChat, type ChatStreamHandlers } from './chat-client'
import { createDeltaCoalescer, type DeltaRun } from './delta-coalescer'

describe('createDeltaCoalescer', () => {
  let frames: Map<number, FrameRequestCallback>
  let nextFrame: number

  /** Runs the animation frames queued so far, as a display frame does. */
  const runFrame = () => {
    const due = [...frames.values()]
    frames.clear()
    for (const callback of due) callback(0)
  }

  beforeEach(() => {
    frames = new Map()
    nextFrame = 1
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
      const id = nextFrame++
      frames.set(id, callback)
      return id
    })
    vi.stubGlobal('cancelAnimationFrame', (id: number) => {
      frames.delete(id)
    })
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  it('merges the deltas of one frame into a single emit', () => {
    const emitted: DeltaRun[] = []
    const coalescer = createDeltaCoalescer((run) => {
      emitted.push(run)
    })

    coalescer.push('message', 'r1', 'Hel')
    coalescer.push('message', 'r1', 'lo')
    expect(emitted).toEqual([])

    runFrame()
    expect(emitted).toEqual([{ kind: 'message', requestId: 'r1', text: 'Hello' }])
    coalescer.close()
  })

  it('keeps the order when the kind or the request changes', () => {
    const emitted: string[] = []
    const coalescer = createDeltaCoalescer((run) => {
      emitted.push(`${run.kind}:${run.requestId ?? '-'}:${run.text}`)
    })

    coalescer.push('message', 'r1', 'a')
    coalescer.push('reasoning', undefined, 'b')
    coalescer.push('message', 'r1', 'c')
    expect(emitted).toEqual(['message:r1:a', 'reasoning:-:b'])

    coalescer.push('message', 'r2', 'd')
    expect(emitted).toEqual(['message:r1:a', 'reasoning:-:b', 'message:r1:c'])

    runFrame()
    expect(emitted).toEqual(['message:r1:a', 'reasoning:-:b', 'message:r1:c', 'message:r2:d'])
    coalescer.close()
  })

  it('flush emits at once and cancels the frame that would have emitted it', () => {
    const emitted: DeltaRun[] = []
    const coalescer = createDeltaCoalescer((run) => {
      emitted.push(run)
    })

    coalescer.push('message', 'r1', 'x')
    coalescer.flush()
    expect(emitted).toEqual([{ kind: 'message', requestId: 'r1', text: 'x' }])
    expect(frames.size).toBe(0)

    runFrame()
    coalescer.flush()
    expect(emitted).toHaveLength(1)
    coalescer.close()
  })

  it('emits after 100 ms when no frame runs, as in a hidden tab', () => {
    const emitted: DeltaRun[] = []
    const coalescer = createDeltaCoalescer((run) => {
      emitted.push(run)
    })

    coalescer.push('message', 'r1', 'x')
    vi.advanceTimersByTime(99)
    expect(emitted).toEqual([])

    vi.advanceTimersByTime(1)
    expect(emitted).toEqual([{ kind: 'message', requestId: 'r1', text: 'x' }])
    coalescer.close()
  })

  it('emits nothing when nothing is pending', () => {
    const emit = vi.fn()
    const coalescer = createDeltaCoalescer(emit)

    coalescer.flush()
    runFrame()
    vi.advanceTimersByTime(100)
    expect(emit).not.toHaveBeenCalled()
    coalescer.close()
  })

  it('emits pending text synchronously when the signal aborts', () => {
    const stop = new AbortController()
    const emit = vi.fn()
    const coalescer = createDeltaCoalescer(emit, stop.signal)

    coalescer.push('message', 'r1', 'Hel')
    stop.abort()
    expect(emit).toHaveBeenCalledWith({ kind: 'message', requestId: 'r1', text: 'Hel' })
    coalescer.close()
  })

  it('close emits what is pending and stops listening to the signal', () => {
    const stop = new AbortController()
    const removeListener = vi.spyOn(stop.signal, 'removeEventListener')
    const emit = vi.fn()
    const coalescer = createDeltaCoalescer(emit, stop.signal)

    coalescer.push('message', 'r1', 'x')
    coalescer.close()
    expect(emit).toHaveBeenCalledTimes(1)
    expect(removeListener).toHaveBeenCalledWith('abort', expect.any(Function))
  })

  it('still emits an empty reasoning delta, as the unbatched path does', () => {
    const emit = vi.fn()
    const coalescer = createDeltaCoalescer(emit)

    coalescer.push('reasoning', undefined, '')
    coalescer.flush()
    expect(emit).toHaveBeenCalledWith({ kind: 'reasoning', requestId: undefined, text: '' })
    coalescer.close()
  })
})

describe('streamChat with VITE_CHAT_DELTA_FRAME_BATCH', () => {
  const encoder = new TextEncoder()

  type Call = { name: string; text?: string; requestId?: string }

  const frame = (event: string, data: Record<string, unknown>) =>
    `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`

  const sseResponse = (chunks: string[]) =>
    new Response(
      new ReadableStream<Uint8Array>({
        start(controller) {
          for (const chunk of chunks) controller.enqueue(encoder.encode(chunk))
          controller.close()
        },
      }),
      { status: 200, headers: { 'Content-Type': 'text/event-stream' } },
    )

  /** Records every handler call in order. UI events keep their type only: `at` is a clock reading. */
  const recordingHandlers = () => {
    const calls: Call[] = []
    const uiEvents: string[] = []
    const handlers: ChatStreamHandlers = {
      onUiEvent: (event) => {
        uiEvents.push(event.type)
      },
      onMessage: ({ content, requestId }) => {
        calls.push({ name: 'message', text: content, requestId })
      },
      onReasoning: ({ delta }) => {
        calls.push({ name: 'reasoning', text: delta })
      },
      onToolCall: ({ name }) => {
        calls.push({ name: `tool_call:${name}` })
      },
      onToolResult: ({ id }) => {
        calls.push({ name: `tool_result:${id}` })
      },
      onCitation: ({ id }) => {
        calls.push({ name: `citation:${id}` })
      },
      onDone: ({ requestId }) => {
        calls.push({ name: 'done', requestId })
      },
    }
    return { handlers, calls, uiEvents }
  }

  const script = [
    frame('connected', { request_id: 'r1', thread_id: 't1' }),
    frame('chunk', { request_id: 'r1', delta: 'Hel' }),
    frame('chunk', { request_id: 'r1', delta: 'lo' }),
    frame('reasoning_delta', { request_id: 'r1', delta: 'think' }),
    frame('chunk', { request_id: 'r1', delta: ' wor' }),
    frame('tool_call', { request_id: 'r1', id: 't1', name: 'search', args: { q: 'x' } }),
    frame('chunk', { request_id: 'r1', delta: 'ld' }),
    frame('tool_result', { request_id: 'r1', id: 't1', output: 'ok' }),
    frame('citation', { request_id: 'r1', id: 'c1', title: 'Kilde', url: 'https://example.com/kilde' }),
    frame('chunk', { request_id: 'r1', delta: '!' }),
    frame('done', { request_id: 'r1', model_used: 'm' }),
  ]

  afterEach(() => {
    vi.unstubAllEnvs()
    vi.unstubAllGlobals()
  })

  it('leaves every delta unmerged while the switch is off', async () => {
    vi.stubEnv('VITE_CHAT_DELTA_FRAME_BATCH', 'false')
    vi.stubGlobal('fetch', vi.fn(async () => sseResponse(script)))
    const { handlers, calls } = recordingHandlers()

    await streamChat({ content: 'hi' }, handlers)

    expect(calls).toEqual([
      { name: 'message', text: 'Hel', requestId: 'r1' },
      { name: 'message', text: 'lo', requestId: 'r1' },
      { name: 'reasoning', text: 'think' },
      { name: 'message', text: ' wor', requestId: 'r1' },
      { name: 'tool_call:search' },
      { name: 'message', text: 'ld', requestId: 'r1' },
      { name: 'tool_result:t1' },
      { name: 'citation:c1' },
      { name: 'message', text: '!', requestId: 'r1' },
      { name: 'done', requestId: 'r1' },
    ])
  })

  it('merges text per run and keeps tool, reasoning and terminal order when on', async () => {
    vi.stubEnv('VITE_CHAT_DELTA_FRAME_BATCH', 'true')
    vi.stubGlobal('fetch', vi.fn(async () => sseResponse(script)))
    const { handlers, calls } = recordingHandlers()

    await streamChat({ content: 'hi' }, handlers)

    expect(calls).toEqual([
      { name: 'message', text: 'Hello', requestId: 'r1' },
      { name: 'reasoning', text: 'think' },
      { name: 'message', text: ' wor', requestId: 'r1' },
      { name: 'tool_call:search' },
      { name: 'message', text: 'ld', requestId: 'r1' },
      { name: 'tool_result:t1' },
      { name: 'citation:c1' },
      { name: 'message', text: '!', requestId: 'r1' },
      { name: 'done', requestId: 'r1' },
    ])
  })

  it('sees UI events in the same order whichever way the text is delivered', async () => {
    vi.stubEnv('VITE_CHAT_DELTA_FRAME_BATCH', 'false')
    vi.stubGlobal('fetch', vi.fn(async () => sseResponse(script)))
    const off = recordingHandlers()
    await streamChat({ content: 'hi' }, off.handlers)

    vi.stubEnv('VITE_CHAT_DELTA_FRAME_BATCH', 'true')
    vi.stubGlobal('fetch', vi.fn(async () => sseResponse(script)))
    const on = recordingHandlers()
    await streamChat({ content: 'hi' }, on.handlers)

    expect(on.uiEvents).toEqual(off.uiEvents)
    expect(on.uiEvents.length).toBeGreaterThan(0)
  })

  it.each(['1', 'TRUE', ' true', 'yes'])('stays off for VITE_CHAT_DELTA_FRAME_BATCH=%j', async (value) => {
    vi.stubEnv('VITE_CHAT_DELTA_FRAME_BATCH', value)
    vi.stubGlobal('fetch', vi.fn(async () => sseResponse([frame('chunk', { delta: 'a' }), frame('chunk', { delta: 'b' })])))
    const { handlers, calls } = recordingHandlers()

    await streamChat({ content: 'hi' }, handlers)

    expect(calls).toEqual([
      { name: 'message', text: 'a', requestId: undefined },
      { name: 'message', text: 'b', requestId: undefined },
    ])
  })

  it('delivers the last text when the stream closes without a terminal frame', async () => {
    vi.stubEnv('VITE_CHAT_DELTA_FRAME_BATCH', 'true')
    vi.stubGlobal('fetch', vi.fn(async () => sseResponse([frame('chunk', { delta: 'a' }), frame('chunk', { delta: 'b' })])))
    const { handlers, calls } = recordingHandlers()

    await streamChat({ content: 'hi' }, handlers)

    expect(calls).toEqual([{ name: 'message', text: 'ab', requestId: undefined }])
  })

  it('puts the text received before a Stop into the transcript as the abort fires', async () => {
    vi.stubEnv('VITE_CHAT_DELTA_FRAME_BATCH', 'true')
    let live!: ReadableStreamDefaultController<Uint8Array>
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, init?: RequestInit) => {
        // What a real fetch does on abort: the body errors with AbortError.
        init?.signal?.addEventListener('abort', () => {
          live.error(new DOMException('Aborted', 'AbortError'))
        })
        return new Response(
          new ReadableStream<Uint8Array>({
            start(controller) {
              live = controller
            },
          }),
          { status: 200, headers: { 'Content-Type': 'text/event-stream' } },
        )
      }),
    )
    const { handlers, calls } = recordingHandlers()
    const stop = new AbortController()

    const finished = streamChat({ content: 'hi' }, handlers, stop.signal)
    live.enqueue(encoder.encode(frame('chunk', { delta: 'Hel' }) + frame('chunk', { delta: 'lo' })))
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(calls).toEqual([])

    stop.abort()
    expect(calls).toEqual([{ name: 'message', text: 'Hello', requestId: undefined }])
    await finished
  })
})
