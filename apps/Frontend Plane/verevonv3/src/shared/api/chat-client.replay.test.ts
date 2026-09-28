/**
 * The chat client's half of a duplicate-free resume, and the metadata it
 * never reads.
 *
 * The gateway now numbers and buffers the tool phase, so a resume can replay a
 * frame the browser already applied. `acceptFrame` lets the caller refuse such
 * a frame before anything sees it, and a step without an id of its own carries
 * the id of the frame that brought it, so a replay can land on the same step.
 */

import { afterEach, describe, expect, it, vi } from 'vitest'
import { getThreadMessages, resumeStream, type ChatStepEvent, type ChatToolCallEvent } from './chat-client'

function sseResponse(frames: string[]): Response {
  const encoder = new TextEncoder()
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const frame of frames) controller.enqueue(encoder.encode(frame))
      controller.close()
    },
  })
  return new Response(stream, { status: 200, headers: { 'Content-Type': 'text/event-stream' } })
}

const sse = (event: string, data: Record<string, unknown>, id?: number) =>
  `${id !== undefined ? `id: ${id}\n` : ''}event: ${event}\ndata: ${JSON.stringify(data)}\n\n`

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('acceptFrame', () => {
  it('drops a refused frame before the cursor, the UI projection or any focused callback sees it', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => sseResponse([
      sse('tool_call', { id: 'call-1', name: 'web_search' }, 3),
      sse('tool_call', { id: 'call-2', name: 'fetch_url' }, 4),
      // A live-only frame carries no id; there is nothing to compare it by.
      sse('tool_call', { id: 'call-3', name: 'knowledge_search' }),
    ])))
    const offered: Array<string | undefined> = []
    const frameIds: string[] = []
    const uiEvents: string[] = []
    const calls: ChatToolCallEvent[] = []
    await resumeStream('req-1', {
      acceptFrame: (id) => {
        offered.push(id)
        return id !== '3'
      },
      onFrameId: (id) => frameIds.push(id),
      onUiEvent: (event) => uiEvents.push(event.type),
      onToolCall: (event) => calls.push(event),
    })

    expect(offered).toEqual(['3', '4', undefined])
    expect(frameIds).toEqual(['4'])
    expect(uiEvents).toHaveLength(2)
    expect(calls.map((call) => call.id)).toEqual(['call-2', 'call-3'])
  })
})

describe('step frame identity', () => {
  it('hands a step the id of the frame that carried it', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => sseResponse([
      sse('step_update', { title: 'Kontrollerer kildene', status: 'running' }, 7),
      sse('step_update', { id: 'draft-1:source-check', title: 'Kildegjennomgang', status: 'active' }, 8),
      sse('step_update', { title: 'Uten id', status: 'running' }),
    ])))
    const steps: ChatStepEvent[] = []
    await resumeStream('req-1', { onStep: (event) => steps.push(event) })

    expect(steps[0]).toMatchObject({ title: 'Kontrollerer kildene', frameId: '7' })
    expect(steps[0]?.id).toBeUndefined()
    expect(steps[1]).toMatchObject({ id: 'draft-1:source-check', frameId: '8' })
    expect(steps[2]).not.toHaveProperty('frameId')
  })
})

describe('assistant message metadata', () => {
  it('never surfaces head_summary, wherever the gateway puts it', async () => {
    const summary = 'Tidligere i samtalen: brukeren ba om et budsjett for Q3.'
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      messages: [
        { id: 'm1', role: 'user', content: 'Hei' },
        {
          id: 'm2',
          role: 'assistant',
          content: 'Svaret.',
          head_summary: { summary, through_id: 'm0', pinned_ids: [] },
          metadata: {
            head_summary: { summary, through_id: 'm0', pinned_ids: [] },
            native_compaction_summary: summary,
            source_scope: 'workspace',
          },
        },
      ],
    }), { status: 200, headers: { 'Content-Type': 'application/json' } })))

    const messages = await getThreadMessages('thread-1')

    expect(messages.map((message) => message.content)).toEqual(['Hei', 'Svaret.'])
    expect(JSON.stringify(messages)).not.toContain(summary)
    expect(JSON.stringify(messages)).not.toContain('head_summary')
  })
})
