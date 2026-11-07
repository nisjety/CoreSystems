import { afterEach, describe, expect, it, vi } from 'vitest'
import { exportThread } from './chat-client'
import { ApiError } from './http'

// The export is a file, not a JSON answer: the client reads it whole as text and keeps the headers the gateway
// wrote. A refusal keeps its typed envelope, so the menu can say why the conversation may not leave.

afterEach(() => {
  vi.unstubAllGlobals()
})

function file(body: string, headers: Record<string, string>): Response {
  return new Response(body, { status: 200, headers })
}

function refusal(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

describe('exportThread', () => {
  it('asks for the chosen format and profile of the thread, with the thread id encoded', async () => {
    const fetchMock = vi.fn().mockResolvedValue(file('{}', { 'Content-Type': 'application/json; charset=utf-8' }))
    vi.stubGlobal('fetch', fetchMock)

    await exportThread('thread 1', 'markdown', 'redacted')

    expect(fetchMock).toHaveBeenCalledTimes(1)
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(url).toContain('/api/v1/chat/threads/thread%201/export?format=markdown&profile=redacted')
    expect(init.credentials).toBe('include')
  })

  it('returns the file as text, with the media type and the save name the gateway gave it', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        file('# Plan\n', {
          'Content-Type': 'text/markdown; charset=utf-8',
          'Content-Disposition': 'attachment; filename="verevon-thread-thread-1.md"',
        }),
      ),
    )

    await expect(exportThread('thread-1', 'markdown', 'full')).resolves.toEqual({
      body: '# Plan\n',
      mediaType: 'text/markdown; charset=utf-8',
      fileName: 'verevon-thread-thread-1.md',
    })
  })

  it('falls back to a save name of its own when the gateway names none', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(file('{}', { 'Content-Type': 'application/json' })))

    await expect(exportThread('thread-1', 'json', 'full')).resolves.toMatchObject({ fileName: 'verevon-thread.json' })
  })

  it('rejects with the typed refusal and its reason, so the menu can say why', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        refusal(412, {
          error: {
            code: 'thread_not_exportable',
            message: 'a run here was kept outside retention',
            details: { reason: 'non_persistent_run' },
          },
        }),
      ),
    )

    const failure = await exportThread('thread-1', 'json', 'full').catch((error: unknown) => error)

    expect(failure).toBeInstanceOf(ApiError)
    expect(failure).toMatchObject({
      status: 412,
      code: 'thread_not_exportable',
      details: { reason: 'non_persistent_run' },
    })
  })

  it('keeps the feature that a zero-retention refusal names', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        refusal(412, {
          error: {
            code: 'zdr_feature_not_supported',
            message: 'not with zero data retention',
            feature: 'conversation_export',
          },
        }),
      ),
    )

    await expect(exportThread('thread-1', 'json', 'full')).rejects.toMatchObject({
      code: 'zdr_feature_not_supported',
      feature: 'conversation_export',
    })
  })

  it('answers a refusal that is not JSON with the status alone', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('<html>gateway down</html>', { status: 502 })))

    await expect(exportThread('thread-1', 'json', 'full')).rejects.toMatchObject({ status: 502, code: null })
  })

  it('refuses a choice the action contract does not name, before anything is sent', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    await expect(exportThread('thread-1', 'pdf' as never, 'full')).rejects.toThrow()
    await expect(exportThread('   ', 'json', 'full')).rejects.toThrow()
    expect(fetchMock).not.toHaveBeenCalled()
  })
})
