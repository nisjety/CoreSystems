import { afterEach, describe, expect, it, vi } from 'vitest'
import { fetchCitedChunk, parseResearchProvenance, streamChat } from './chat-client'

// EP-2-S4: a citation frame may carry `quote` and `locator`, and a research report's artifact frame a
// `provenance` stamp. Each is handed on only when well formed; a frame without them reads exactly as before.

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

const citationFrame = (extra: Record<string, unknown> = {}) => frame('citation', {
  id: 'doc-1',
  title: 'Refusjonsrutine',
  url: 'notion',
  snippet: 'Refusjon innen 30 dager.',
  source_group_id: 'kb:notion',
  origin: 'knowledge',
  number: 1,
  ...extra,
})

const stamp = {
  question: 'Hva koster laksefôr?',
  sub_queries: ['laksefôr pris'],
  sources: [{ number: 1, url: 'https://example.no/a', title: 'A' }],
  model: 'verevon-research',
  coverage: { found: 3, read: 1, unread: 2, deadline_hit: false },
  generated_at: '2026-11-04T10:20:30Z',
  pipeline: 'deep_research_v1',
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('citation frame: the cited passage', () => {
  it('hands on the quote and the locator as sent', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([
      citationFrame({ quote: 'Refusjon gis innen 30 dager.', locator: { document_id: 'doc-1', chunk_index: 2 } }),
      frame('done', {}),
    ])))
    const onCitation = vi.fn()
    await streamChat({ content: 'refusjon?' }, { onCitation })
    expect(onCitation.mock.calls[0]?.[0]).toMatchObject({
      quote: 'Refusjon gis innen 30 dager.',
      locator: { document_id: 'doc-1', chunk_index: 2 },
    })
  })

  it('adds no passage keys to a frame without them', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([citationFrame(), frame('done', {})])))
    const onCitation = vi.fn()
    await streamChat({ content: 'refusjon?' }, { onCitation })
    const event = onCitation.mock.calls[0]?.[0] ?? {}
    expect('quote' in event).toBe(false)
    expect('locator' in event).toBe(false)
  })

  it('drops a malformed locator', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([
      citationFrame({ locator: { document_id: 'doc-1', chunk_index: -1 } }),
      frame('done', {}),
    ])))
    const onCitation = vi.fn()
    await streamChat({ content: 'refusjon?' }, { onCitation })
    expect('locator' in (onCitation.mock.calls[0]?.[0] ?? {})).toBe(false)
  })
})

describe('artifact frame: the provenance stamp', () => {
  it('hands on a complete stamp and nothing for a report without one', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(sseResponse([
      frame('artifact', { id: 'research-r', kind: 'document', title: 'Dyp research', content: '# R', version: 1, provenance: stamp }),
      frame('artifact', { id: 'doc', kind: 'document', title: 'T', content: 'c', version: 1 }),
      frame('done', {}),
    ])))
    const onArtifact = vi.fn()
    await streamChat({ content: 'research' }, { onArtifact })
    expect(onArtifact.mock.calls[0]?.[0]).toMatchObject({ provenance: stamp })
    expect('provenance' in (onArtifact.mock.calls[1]?.[0] ?? {})).toBe(false)
  })

  it('refuses a partial stamp rather than showing it half filled', () => {
    expect(parseResearchProvenance({ ...stamp, coverage: undefined })).toBeUndefined()
    expect(parseResearchProvenance({ ...stamp, pipeline: '' })).toBeUndefined()
    expect(parseResearchProvenance(stamp)).toEqual(stamp)
  })
})

describe('fetchCitedChunk', () => {
  const chunks = (body: unknown, status = 200) => vi.fn().mockResolvedValue(new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  }))

  it('asks for the document and picks the chunk the citation names', async () => {
    const fetchMock = chunks({
      chunks: [
        { document_id: 'doc-1', chunk_index: 1, text: 'Første.' },
        { document_id: 'doc-1', chunk_index: 2, text: 'Andre.' },
      ],
    })
    vi.stubGlobal('fetch', fetchMock)
    await expect(fetchCitedChunk({ document_id: 'doc-1', chunk_index: 2 })).resolves.toEqual({ status: 'ok', text: 'Andre.' })
    const [url, init] = fetchMock.mock.calls[0] ?? []
    expect(String(url)).toContain('/api/v1/knowledge/retrieve/chunks')
    expect(JSON.parse(String((init as RequestInit).body))).toEqual({ document_id: 'doc-1' })
  })

  it('reads an empty answer as no access: Data Plane fetches nothing the viewer may not read', async () => {
    vi.stubGlobal('fetch', chunks({ chunks: [], count: 0 }))
    await expect(fetchCitedChunk({ document_id: 'doc-1', chunk_index: 2 })).resolves.toEqual({ status: 'denied' })
  })

  it('reads a 403 as no access', async () => {
    vi.stubGlobal('fetch', chunks({ error: { code: 'forbidden', message: 'no', details: {} } }, 403))
    await expect(fetchCitedChunk({ document_id: 'doc-1', chunk_index: 2 })).resolves.toEqual({ status: 'denied' })
  })

  it('says the passage is missing when the document no longer has that chunk', async () => {
    vi.stubGlobal('fetch', chunks({ chunks: [{ document_id: 'doc-1', chunk_index: 0, text: 'Bare denne.' }] }))
    await expect(fetchCitedChunk({ document_id: 'doc-1', chunk_index: 2 })).resolves.toEqual({ status: 'missing' })
  })
})
