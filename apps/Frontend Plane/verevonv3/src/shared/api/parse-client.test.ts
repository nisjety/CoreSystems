import { afterEach, describe, expect, it, vi } from 'vitest'
import { documentKindOf, parseDocument } from '@/shared/api/parse-client'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('documentKindOf', () => {
  it('reads the kind from the file name, ignoring case and blanks', () => {
    expect(documentKindOf('Faktura.PDF')).toBe('pdf')
    expect(documentKindOf(' avtale.docx ')).toBe('docx')
    expect(documentKindOf('bilde.png')).toBeNull()
  })
})

describe('parseDocument', () => {
  it('sends the file as its raw bytes under the declared type, and returns the data', async () => {
    const fetchMock = vi.fn(async (_url: string, _init?: RequestInit) =>
      new Response(
        JSON.stringify({
          data: { markdown: '# Hei', kind: 'pdf', bytes: 9, structure: 'plain', truncated: false, pages: 1, warnings: [] },
          meta: { request_id: 'req_1' },
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      ),
    )
    vi.stubGlobal('fetch', fetchMock)
    const file = new File(['%PDF-1.4'], 'a.pdf', { type: 'application/pdf' })

    const doc = await parseDocument(file, 'pdf')

    expect(doc.markdown).toBe('# Hei')
    expect(doc.pages).toBe(1)
    const [url, init] = fetchMock.mock.calls[0] ?? []
    expect(url).toMatch(/\/api\/ingestions\/parse$/)
    expect(init?.method).toBe('POST')
    expect(new Headers(init?.headers).get('Content-Type')).toBe('application/pdf')
    expect(init?.body).toBe(file)
  })

  it('sends the DOCX type for a DOCX', async () => {
    const fetchMock = vi.fn(async (_url: string, _init?: RequestInit) =>
      new Response(JSON.stringify({ data: { markdown: '', kind: 'docx', bytes: 0, structure: 'plain', truncated: false, warnings: [] } }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    await parseDocument(new File(['PK'], 'a.docx'), 'docx')

    const [, init] = fetchMock.mock.calls[0] ?? []
    expect(new Headers(init?.headers).get('Content-Type')).toBe(
      'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    )
  })

  it('carries the busy answer and its Retry-After header to the caller', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async (_url: string, _init?: RequestInit) =>
          new Response(JSON.stringify({ error: { code: 'RATE_LIMITED', message: 'busy' } }), {
            status: 429,
            headers: { 'Content-Type': 'application/json', 'Retry-After': '5' },
          }),
      ),
    )

    await expect(parseDocument(new File(['x'], 'a.pdf'), 'pdf')).rejects.toMatchObject({
      status: 429,
      code: 'RATE_LIMITED',
      retryAfterSeconds: 5,
    })
  })
})
