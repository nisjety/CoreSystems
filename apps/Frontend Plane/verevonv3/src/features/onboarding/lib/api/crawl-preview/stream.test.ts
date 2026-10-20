import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError, SESSION_EXPIRED_EVENT } from '@/shared/api/http'
import { translateApiError } from '@/shared/i18n/errors'
import { streamCrawlPreview } from './stream'

const no = (norwegian: string) => norwegian

function jsonResponse(data: unknown, status: number, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(data), { status, headers: { 'Content-Type': 'application/json', ...headers } })
}

describe('streamCrawlPreview failures', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('sends an expired session back to sign-in, with the typed failure', async () => {
    const expired = vi.fn()
    window.addEventListener(SESSION_EXPIRED_EVENT, expired)
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ error: { code: 'unauthorized', message: 'Authentication required' } }, 401)))

    const failure = await streamCrawlPreview({ url: 'https://example.no' }, {}).catch((error: unknown) => error)
    expect(failure).toBeInstanceOf(ApiError)
    expect(failure).toMatchObject({ status: 401, code: 'unauthorized' })
    expect(expired).toHaveBeenCalledTimes(1)
    window.removeEventListener(SESSION_EXPIRED_EVENT, expired)
  })

  it('keeps the wait of a 429 so the page can say when to try again', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('rate limit exceeded', { status: 429, headers: { 'Retry-After': '20' } })))

    const failure = await streamCrawlPreview({ url: 'https://example.no' }, {}).catch((error: unknown) => error)
    expect(failure).toMatchObject({ status: 429, retryAfterSeconds: 20 })
    expect(translateApiError(failure, no, { no: 'Kunne ikke forhåndsvise nettsiden.', en: 'x' })).toBe(
      'Det kommer for mange forespørsler akkurat nå. Prøv igjen om 20 sekunder.',
    )
  })

  it('reports a failure with no JSON body by its status, never by the raw text', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('<html>Bad gateway</html>', { status: 502 })))

    const failure = await streamCrawlPreview({ url: 'https://example.no' }, {}).catch((error: unknown) => error)
    expect(failure).toMatchObject({ status: 502, code: null })
    expect(translateApiError(failure, no, { no: 'Kunne ikke forhåndsvise nettsiden.', en: 'x' })).toBe('Kunne ikke forhåndsvise nettsiden.')
  })

  it('hands a warning packet to the page with its code, which has copy', async () => {
    const body = 'event: warning\ndata: {"code":"empty_crawl","message":"Vi fikk ikke hentet innhold fra nettsiden."}\n\n'
    vi.stubGlobal('fetch', vi.fn(async () => new Response(body, { status: 200, headers: { 'Content-Type': 'text/event-stream' } })))
    const onWarning = vi.fn()

    await streamCrawlPreview({ url: 'https://example.no' }, { onWarning })
    expect(onWarning).toHaveBeenCalledWith({ code: 'empty_crawl', message: 'Vi fikk ikke hentet innhold fra nettsiden.' })
    expect(translateApiError({ code: 'empty_crawl' }, no)).toMatch(/Prøv på nytt, eller fortsett uten forhåndsvisning/)
  })
})
