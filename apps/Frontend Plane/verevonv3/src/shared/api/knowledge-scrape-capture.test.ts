import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  buildCaptureRequest,
  captureFormats,
  readScrapeCaptures,
  scrapeArtifactUrl,
  scrapeRequestBody,
  scrapeUrl,
} from './knowledge-client'

// WS13.7: the capture request, the artifact route URL and the defensive reader.
describe('scrape captures (WS13.7)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('turns a capture request into formats entries and keeps markdown for the text', () => {
    expect(captureFormats({ screenshot: true, pdf: true })).toEqual(['screenshot', 'pdf'])
    expect(captureFormats({ screenshot: { fullPage: true } })).toEqual([{ type: 'screenshot', fullPage: true }])
    expect(captureFormats(undefined)).toEqual([])
    expect(scrapeRequestBody({ url: 'https://example.com/', capture: { pdf: true } })).toEqual({
      url: 'https://example.com/',
      formats: ['markdown', 'pdf'],
    })
    expect(scrapeRequestBody({ url: 'https://example.com/' })).toEqual({ url: 'https://example.com/' })
  })

  it('scrapeUrl sends the capture as formats and never sends a capture key', async () => {
    const fetchMock = vi.fn(
      async (_input: RequestInfo | URL, _init?: RequestInit) =>
        new Response(JSON.stringify({ data: {} }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        }),
    )
    vi.stubGlobal('fetch', fetchMock)

    await scrapeUrl('org-1', { url: 'https://example.com/', capture: { screenshot: true } })

    const init = fetchMock.mock.calls[0]?.[1]
    const sent = JSON.parse(String(init?.body))
    expect(sent.formats).toEqual(['markdown', 'screenshot'])
    expect(sent).not.toHaveProperty('capture')
  })

  it('builds the gateway artifact route for an artifact id', () => {
    expect(scrapeArtifactUrl('art_01AB')).toBe('/api/v1/knowledge/scrape-artifacts/art_01AB')
  })

  it('reads the artifact references from the top level or from data', () => {
    expect(readScrapeCaptures({ formats: { screenshot: 'art_01', pdf: 'art_02' } })).toEqual({
      pdfUrl: '/api/v1/knowledge/scrape-artifacts/art_02',
      screenshotUrl: '/api/v1/knowledge/scrape-artifacts/art_01',
    })
    expect(readScrapeCaptures({ data: { formats: { screenshot: 'art_01' } } })?.screenshotUrl).toBe(
      '/api/v1/knowledge/scrape-artifacts/art_01',
    )
  })

  it('reads an inline ZDR screenshot and ignores values of the wrong shape', () => {
    expect(
      readScrapeCaptures({ screenshot: { content_type: 'image/png', data_base64: 'iVBORw0K', bytes: 8 } }),
    ).toEqual({ screenshotInline: { contentType: 'image/png', dataBase64: 'iVBORw0K' } })
    expect(readScrapeCaptures({ formats: { screenshot: 'not-an-artifact' }, screenshot: 'raw' })).toBeUndefined()
    expect(readScrapeCaptures(null)).toBeUndefined()
  })
})

// W2-U65-51: a capture is requested only when the user chose it.
describe('capture toggles (W2-U65-51)', () => {
  it('returns no request unless a toggle is true, and never carries a false', () => {
    expect(buildCaptureRequest({})).toBeUndefined()
    expect(buildCaptureRequest({ pdf: false, screenshot: false })).toBeUndefined()
    expect(buildCaptureRequest({ screenshot: true })).toEqual({ screenshot: true })
    expect(buildCaptureRequest({ pdf: true, screenshot: false })).toEqual({ pdf: true })
    expect(buildCaptureRequest({ pdf: true, screenshot: true })).toEqual({ pdf: true, screenshot: true })
  })

  it('a request built from the toggles becomes formats entries, none when unticked', () => {
    expect(captureFormats(buildCaptureRequest({ pdf: true, screenshot: true }))).toEqual(['screenshot', 'pdf'])
    expect(captureFormats(buildCaptureRequest({ screenshot: false }))).toEqual([])
    expect(scrapeRequestBody({ capture: buildCaptureRequest({}), url: 'https://example.com/' })).toEqual({
      url: 'https://example.com/',
    })
  })
})
