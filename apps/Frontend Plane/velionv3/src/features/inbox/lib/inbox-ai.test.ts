import { afterEach, describe, expect, it, vi } from 'vitest'
import { runAssist } from './inbox-ai'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('runAssist', () => {
  it('sends organization-scoped conversation context with ZDR disabled by default', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({
        data: {
          content: 'I can help with the missing delivery.',
          model_used: 'velion-balance',
          sources: [],
        },
      }), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    await expect(runAssist('org-aquatiq', 'draft', [
      { agent: false, from: 'Maya', body: '  My package is missing.  ' },
      { agent: true, body: 'We are checking the delivery scan.' },
    ], { customer: 'Maya Solberg' })).resolves.toMatchObject({
      text: 'I can help with the missing delivery.',
      model: 'velion-balance',
    })

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    const headers = new Headers(init.headers)
    const body = JSON.parse(String(init.body)) as Record<string, unknown>

    expect(url).toBe('/api/v1/chat/invoke')
    expect(init.method).toBe('POST')
    expect(headers.get('x-velion-org-id')).toBe('org-aquatiq')
    expect(body.zdr).toBe(false)
    expect(body.content).toContain('the customer (Maya Solberg)')
    expect(body.content).toContain('Maya: My package is missing.')
    expect(body.content).toContain('Agent (us): We are checking the delivery scan.')
  })
})
