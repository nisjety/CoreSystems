import { afterEach, describe, expect, it, vi } from 'vitest'
import { streamChat } from './chat-client'

// The `attachment` frame carries the gateway's structural check of an Office or
// PDF file as an optional key. The client hands it on as sent and reads nothing
// else from it; a frame without the key reads exactly as it always did.

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

const frame = (event: string, data: unknown) => `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`

const DOCX = 'application/vnd.openxmlformats-officedocument.wordprocessingml.document'

describe('attachment event: the structural check', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('hands the check to onAttachment as the gateway sent it', async () => {
    const check = {
      name: 'deck.docx',
      kind: 'docx',
      level: 'failed',
      checks: [{ id: 'container', level: 'failed', message: 'the file is not a readable zip package' }],
      stats: {},
    }
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([
      frame('attachment', {
        id: 'call-1-file-1',
        name: 'deck.docx',
        type: DOCX,
        url: 'data:application/octet-stream;base64,AA',
        size: 4096,
        check,
      }),
      frame('done', {}),
    ])))
    const onAttachment = vi.fn()
    await streamChat({ content: 'make a docx' }, { onAttachment })
    expect(onAttachment).toHaveBeenCalledTimes(1)
    expect(onAttachment.mock.calls[0]?.[0]).toMatchObject({ name: 'deck.docx', size: 4096, check })
  })

  it('leaves check undefined when the frame carries none', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([
      frame('attachment', { id: 'img-1', name: 'generated-image.png', type: 'image/png', url: 'data:image/png;base64,AA', size: 2 }),
      frame('done', {}),
    ])))
    const onAttachment = vi.fn()
    await streamChat({ content: 'draw a cat' }, { onAttachment })
    expect(onAttachment.mock.calls[0]?.[0]).toMatchObject({ name: 'generated-image.png' })
    expect(onAttachment.mock.calls[0]?.[0]?.check).toBeUndefined()
  })
})
