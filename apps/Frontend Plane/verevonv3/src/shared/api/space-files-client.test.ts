import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from './http'
import {
  deleteSpaceFile,
  getSpaceFileUpload,
  listSpaceFiles,
  readSpaceFileText,
  uploadSpaceFiles,
} from './space-files-client'

// WS14-S9: the room file library over the gateway routes. The fetch is stubbed below the client, so
// the real transport (headers, the multipart body, typed errors) is what each assertion reads.

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify({ data }), { status, headers: { 'Content-Type': 'application/json' } })
}

function errorResponse(status: number, code: string): Response {
  return new Response(JSON.stringify({ error: { code, message: code, details: {} } }), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('listSpaceFiles (WS14-S9)', () => {
  it('lists a page with the default limit and keeps the cursor and the stated gaps', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({
      files: [{
        document_id: 'doc_1',
        file_name: 'plan.pdf',
        mime_type: 'application/pdf',
        size_bytes: 2048,
        sha256: 'a'.repeat(64),
        extension: 'pdf',
        status: 'ready',
        text_chars: 1200,
        uploaded_by: 'user_1',
        uploaded_at: '2026-11-01T10:00:00Z',
      }],
      next_cursor: 'cursor-2',
      unavailable: [{ section: 'files', code: 'files_upstream_unavailable', reason: 'retry later' }],
    }))
    vi.stubGlobal('fetch', fetchMock)

    const page = await listSpaceFiles('space 1')

    expect(String((fetchMock.mock.calls[0] as unknown as [string])[0])).toContain('/api/v1/spaces/space%201/files?limit=50')
    expect(page.files).toEqual([{
      document_id: 'doc_1',
      file_name: 'plan.pdf',
      mime_type: 'application/pdf',
      size_bytes: 2048,
      sha256: 'a'.repeat(64),
      extension: 'pdf',
      status: 'ready',
      text_chars: 1200,
      uploaded_by: 'user_1',
      uploaded_at: '2026-11-01T10:00:00Z',
    }])
    expect(page.nextCursor).toBe('cursor-2')
    expect(page.unavailable).toEqual([{ section: 'files', code: 'files_upstream_unavailable', reason: 'retry later' }])
  })

  it('sends the limit and the cursor it is given, and reads a last page as a null cursor', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ files: [] }))
    vi.stubGlobal('fetch', fetchMock)

    const page = await listSpaceFiles('room-1', { limit: 10, cursor: 'cursor-9' })

    const url = new URL(String((fetchMock.mock.calls[0] as unknown as [string])[0]), 'http://localhost')
    expect(url.pathname).toBe('/api/v1/spaces/room-1/files')
    expect(url.searchParams.get('limit')).toBe('10')
    expect(url.searchParams.get('cursor')).toBe('cursor-9')
    expect(page).toEqual({ files: [], nextCursor: null, unavailable: [] })
  })

  it('drops rows without a document id, reads a bad number as absent, and defaults an empty gap', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({
      files: [
        { file_name: 'no-id.txt' },
        'junk',
        null,
        { document_id: 'doc_2', size_bytes: -5, text_chars: '12', file_name: '' },
      ],
      unavailable: [{}],
    })))

    const page = await listSpaceFiles('room-1')

    expect(page.files).toEqual([{
      document_id: 'doc_2',
      file_name: null,
      mime_type: null,
      size_bytes: null,
      sha256: null,
      extension: null,
      status: null,
      text_chars: null,
      uploaded_by: null,
      uploaded_at: null,
    }])
    expect(page.unavailable).toEqual([{ section: 'files', code: 'files_upstream_unavailable', reason: '' }])
  })
})

describe('uploadSpaceFiles (WS14-S9)', () => {
  it('posts the files as the multipart field files with the Idempotency-Key and no Content-Type of its own', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({
      upload_id: 'up_1',
      deduplicated: false,
      files: [{ file_name: 'a.txt', status: 'queued' }, { file_name: 'b.md' }],
    }))
    vi.stubGlobal('fetch', fetchMock)
    const first = new File(['alpha'], 'a.txt', { type: 'text/plain' })
    const second = new File(['beta'], 'b.md', { type: 'text/markdown' })

    const accepted = await uploadSpaceFiles('space_1', [first, second], 'upload-key-0001')

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toContain('/api/v1/spaces/space_1/files')
    expect(init.method).toBe('POST')
    const headers = init.headers as Headers
    expect(headers.get('Idempotency-Key')).toBe('upload-key-0001')
    expect(headers.get('Content-Type')).toBeNull()
    const form = init.body as FormData
    expect(form).toBeInstanceOf(FormData)
    expect(form.getAll('files').map((file) => (file as File).name)).toEqual(['a.txt', 'b.md'])
    expect(accepted).toEqual({
      uploadId: 'up_1',
      deduplicated: false,
      files: [
        { fileName: 'a.txt', status: 'queued' },
        { fileName: 'b.md', status: 'queued' },
      ],
    })
  })

  it('reads a replay of the same key as deduplicated, and sends a key of its own when none is given', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ upload_id: 'up_1', deduplicated: true, files: [] }))
    vi.stubGlobal('fetch', fetchMock)
    const file = new File(['alpha'], 'a.txt', { type: 'text/plain' })

    const replay = await uploadSpaceFiles('space_1', [file], 'upload-key-0002')
    expect(replay).toEqual({ uploadId: 'up_1', deduplicated: true, files: [] })

    await uploadSpaceFiles('space_1', [file])
    const [, retryInit] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    const [, freshInit] = fetchMock.mock.calls[1] as unknown as [string, RequestInit]
    expect((retryInit.headers as Headers).get('Idempotency-Key')).toBe('upload-key-0002')
    const fresh = (freshInit.headers as Headers).get('Idempotency-Key') ?? ''
    expect(fresh).toMatch(/^upload-/)
    expect(fresh.length).toBeGreaterThanOrEqual(8)
    expect(fresh.length).toBeLessThanOrEqual(128)
  })
})

describe('getSpaceFileUpload (WS14-S9)', () => {
  it('maps the progress and keeps only the item rows it can read', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({
      upload_id: 'up_1',
      status: 'completed_with_errors',
      total: 3,
      processed: 3,
      failed: 1,
      items: [
        { source_name: 'a.txt', status: 'completed', document_id: 'doc_1' },
        { source_name: 'b.bin', status: 'failed', error_code: 'unsupported_type' },
        'junk',
        null,
      ],
    }))
    vi.stubGlobal('fetch', fetchMock)

    const progress = await getSpaceFileUpload('space_1', 'up_1')

    expect(String((fetchMock.mock.calls[0] as unknown as [string])[0])).toContain('/api/v1/spaces/space_1/files/uploads/up_1')
    expect(progress).toEqual({
      uploadId: 'up_1',
      status: 'completed_with_errors',
      total: 3,
      processed: 3,
      failed: 1,
      items: [
        { sourceName: 'a.txt', status: 'completed', documentId: 'doc_1' },
        { sourceName: 'b.bin', status: 'failed', errorCode: 'unsupported_type' },
      ],
    })
  })
})

describe('readSpaceFileText (WS14-S9)', () => {
  it('reads the text of one file under its encoded id, with the cap', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({
      document_id: 'doc/1',
      file_name: 'plan.txt',
      text: 'Hei',
      text_chars: 3,
      truncated: true,
    }))
    vi.stubGlobal('fetch', fetchMock)

    const text = await readSpaceFileText('space_1', 'doc/1', { maxChars: 500 })

    expect(String((fetchMock.mock.calls[0] as unknown as [string])[0])).toContain('/api/v1/spaces/space_1/files/doc%2F1/text?max_chars=500')
    expect(text).toEqual({ documentId: 'doc/1', fileName: 'plan.txt', text: 'Hei', textChars: 3, truncated: true })
  })

  it('reads a text that is not a string as empty, and a truncated flag that is not true as false', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ text: 5, truncated: 'yes' })))

    const text = await readSpaceFileText('space_1', 'doc_1')

    expect(text).toEqual({ documentId: 'doc_1', fileName: null, text: '', textChars: null, truncated: false })
  })
})

describe('deleteSpaceFile (WS14-S9)', () => {
  it('deletes one file by its encoded id and resolves on 204', async () => {
    const fetchMock = vi.fn(async () => new Response(null, { status: 204 }))
    vi.stubGlobal('fetch', fetchMock)

    await expect(deleteSpaceFile('space_1', 'doc_1')).resolves.toBeUndefined()

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toContain('/api/v1/spaces/space_1/files/doc_1')
    expect(init.method).toBe('DELETE')
  })

  it('lets a refused delete reach the caller as the typed ApiError', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => errorResponse(404, 'not_found')))

    const failure = await deleteSpaceFile('space_1', 'doc_1').catch((error: unknown) => error)

    expect(failure).toBeInstanceOf(ApiError)
    expect(failure).toMatchObject({ status: 404, code: 'not_found' })
  })
})
