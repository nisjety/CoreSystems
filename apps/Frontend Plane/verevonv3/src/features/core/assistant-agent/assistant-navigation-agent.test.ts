import { describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/http'
import { createAssistantNavigationAgent, parseAssistantTaskProposal } from './assistant-navigation-agent'

const proposal = '{"type":"assistant.navigation.guide","href":"/knowledge","mode":"point"}'

describe('model-powered navigation', () => {
  it('accepts only a complete, exact and allowlisted action', () => {
    expect(parseAssistantTaskProposal(proposal)).toEqual({ type: 'assistant.navigation.guide', href: '/knowledge', mode: 'point' })
    expect(parseAssistantTaskProposal('{"type":"unsupported"}')).toEqual({ type: 'unsupported' })
    for (const bad of ['```json\n' + proposal + '\n```',
      '{"type":"assistant.navigation.guide","href":"https://example.com","mode":"navigate"}',
      '{"type":"assistant.navigation.guide","href":"/knowledge","mode":"point","extra":true}',
      '{"type":"assistant.browser.current","url":"https://example.com"}',
      '{"type":"assistant.browser.search","query":"https://example.com"}',
      '{"type":"assistant.browser.search","query":"name@example.com"}',
      '[' + proposal + ',' + proposal + ']']) expect(parseAssistantTaskProposal(bad)).toBeNull()
  })

  it('searches with the proposed public keywords and opens the first valid ranked public result', async () => {
    const search = vi.fn(async () => ({ mode: 'search' as const, results: [
      { url: 'https://private.example.com/roadmap', provider: 'tantivy_local' },
      { url: 'javascript:alert(1)', provider: 'searxng' },
      { url: 'https://www.iana.org/domains', provider: 'searxng' },
      { url: 'https://www.icann.org', provider: 'brave' },
    ] }))
    const infer = vi.fn(async () => ({ content: '{"type":"assistant.browser.search","query":"IANA domain name services"}' }))
    const onBrowserTask = vi.fn(async () => {})
    const action = vi.fn(() => true)
    const agent = createAssistantNavigationAgent({ scope: () => 'user:org', onChange: vi.fn(), onAction: action, onBrowserTask, dependencies: { infer, search } })
    const goal = 'Open the IANA page about domain names and tell me the heading for my private work item'
    await agent.send(goal)
    expect(search).toHaveBeenCalledExactlyOnceWith('IANA domain name services', expect.any(AbortSignal))
    expect(onBrowserTask).toHaveBeenCalledExactlyOnceWith(goal, 'https://www.iana.org/domains')
    expect(action).not.toHaveBeenCalled()
    expect(agent.state().status).toBe('idle')
  })

  it('keeps a named-site request on that site and prefers its results page over its homepage', async () => {
    const goal = 'gå på finn og åpne alle leiligheter i oslo'
    const url = 'https://www.finn.no/realestate/homes/search.html?location=0.20061&property_type=3'
    const search = vi.fn(async () => ({ mode: 'search' as const, results: [
      { url: 'https://hybel.no/bolig-til-leie/Oslo', provider: 'brave' },
      { url: 'https://www.finn.no/', provider: 'searxng' },
      { url, provider: 'brave' },
    ] }))
    const infer = vi.fn(async () => ({ content: '{"type":"assistant.browser.search","query":"FINN Eiendom leiligheter Oslo"}' }))
    const onBrowserTask = vi.fn(async () => {})
    const agent = createAssistantNavigationAgent({ scope: () => 'user:org', onChange: vi.fn(), onAction: vi.fn(), onBrowserTask, dependencies: { infer, search } })
    await agent.send(goal)
    expect(onBrowserTask).toHaveBeenCalledExactlyOnceWith(goal, url)
    expect(search).toHaveBeenCalledTimes(1)
  })

  it('resolves the named site publicly before using a scoped results search', async () => {
    const goal = 'go to Example and open the public catalog'
    const other = { url: 'https://other.test/catalog', provider: 'brave' }
    const home = { url: 'https://www.example.com/', provider: 'searxng' }
    const catalog = { url: 'https://www.example.com/catalog', provider: 'brave' }
    const search = vi.fn(async (_query: string) => ({ mode: 'search' as const, results: [other] }))
      .mockResolvedValueOnce({ mode: 'search', results: [other] })
      .mockResolvedValueOnce({ mode: 'search', results: [home] })
      .mockResolvedValueOnce({ mode: 'search', results: [other, catalog] })
    const infer = vi.fn(async () => ({ content: '{"type":"assistant.browser.search","query":"Example public catalog"}' }))
    const onBrowserTask = vi.fn(async () => {})
    const agent = createAssistantNavigationAgent({ scope: () => 'user:org', onChange: vi.fn(), onAction: vi.fn(), onBrowserTask, dependencies: { infer, search } })
    await agent.send(goal)
    expect(search.mock.calls.map(([query]) => query)).toEqual(['Example public catalog', 'example official website', 'site:example.com Example public catalog'])
    expect(onBrowserTask).toHaveBeenCalledExactlyOnceWith(goal, catalog.url)
  })

  it.each([
    { status: 504, code: 'TIMEOUT', op: 'navigate', retries: true },
    { status: 504, code: 'TIMEOUT', op: 'launch', retries: false },
    { status: 403, code: 'browser_site_blocked', op: 'navigate', retries: false },
  ])('retries a named site from its homepage only after a navigation timeout ($status $op)', async ({ status, code, op, retries }) => {
    const goal = 'gå på finn og åpne alle leiligheter i oslo'
    const url = 'https://www.finn.no/realestate/homes/search.html'
    const failure = new ApiError('Browser startup failed', status, code, { details: { op } })
    const onBrowserTask = vi.fn(async () => {}).mockRejectedValueOnce(failure)
    const agent = createAssistantNavigationAgent({
      scope: () => 'user:org',
      onChange: vi.fn(),
      onAction: vi.fn(),
      onBrowserTask,
      dependencies: {
        infer: async () => ({ content: '{"type":"assistant.browser.search","query":"FINN leiligheter Oslo"}' }),
        search: async () => ({ mode: 'search', results: [{ url, provider: 'brave' }] }),
      },
    })

    if (retries) {
      await agent.send(goal)
      expect(onBrowserTask.mock.calls).toEqual([[goal, url], [goal, 'https://www.finn.no/']])
    } else {
      await expect(agent.send(goal)).rejects.toBe(failure)
      expect(onBrowserTask).toHaveBeenCalledExactlyOnceWith(goal, url)
    }
  })

  it('resolves a current-page task using the visible browser and does no web search', async () => {
    const infer = vi.fn(async () => ({ content: '{"type":"assistant.browser.current"}' }))
    const search = vi.fn()
    const currentBrowserTarget = vi.fn(async (): Promise<string | null> => 'https://www.iana.org/domains')
    const onBrowserTask = vi.fn(async () => {})
    const agent = createAssistantNavigationAgent({ scope: () => 'user:org', onChange: vi.fn(), onAction: vi.fn(), currentBrowserTarget, onBrowserTask, dependencies: { infer, search } })
    await agent.send('Hva står det på denne siden?')
    expect(onBrowserTask).toHaveBeenCalledExactlyOnceWith('Hva står det på denne siden?', 'https://www.iana.org/domains')
    expect(search).not.toHaveBeenCalled()
    currentBrowserTarget.mockResolvedValueOnce(null)
    await expect(agent.send('Hva står det på denne siden?')).rejects.toThrow('Åpne en side i Nett først')
    expect(onBrowserTask).toHaveBeenCalledOnce()
  })

  it('does not launch when search returns only workspace results or the org changes during search', async () => {
    for (const changeScope of [false, true]) {
      let scope = 'one'
      const search = vi.fn(async () => {
        if (changeScope) scope = 'two'
        return { mode: 'search' as const, results: [{ url: 'https://example.com', provider: changeScope ? 'searxng' : 'tantivy_local' }] }
      })
      const infer = vi.fn(async () => ({ content: '{"type":"assistant.browser.search","query":"Example page"}' }))
      const onBrowserTask = vi.fn(async () => {})
      const agent = createAssistantNavigationAgent({ scope: () => scope, onChange: vi.fn(), onAction: vi.fn(), onBrowserTask, dependencies: { infer, search } })
      await expect(agent.send('Open the example page')).rejects.toThrow()
      expect(onBrowserTask).not.toHaveBeenCalled()
    }
  })

  it('sends only the task to the stateless gateway and does not retry a refusal', async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: { code: 'zdr_unavailable', message: 'ZDR unavailable' } }), { status: 503, headers: { 'Content-Type': 'application/json' } }))
    vi.stubGlobal('fetch', fetch)
    try {
      const action = vi.fn(() => true)
      const agent = createAssistantNavigationAgent({ scope: () => 'user:org', onChange: vi.fn(), onAction: action })
      await expect(agent.send('Vis Kunnskap')).rejects.toThrow('ZDR unavailable')
      expect(fetch).toHaveBeenCalledOnce()
      const [url, init] = fetch.mock.calls[0]!
      expect(url).toContain('/api/v1/ai/assistant-navigation')
      expect(JSON.parse(init.body)).toEqual({ prompt: 'Vis Kunnskap' })
      expect(init.credentials).toBe('include')
      expect(action).not.toHaveBeenCalled()
    } finally { vi.unstubAllGlobals() }
  })

  it('unwraps the gateway data envelope before validating its proposal', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: { content: proposal } }), { status: 200, headers: { 'Content-Type': 'application/json' } })))
    try {
      const action = vi.fn(() => true)
      const agent = createAssistantNavigationAgent({ scope: () => 'user:org', onChange: vi.fn(), onAction: action })
      await agent.send('Vis Kunnskap')
      expect(action).toHaveBeenCalledWith({ type: 'assistant.navigation.guide', href: '/knowledge', mode: 'point' })
    } finally { vi.unstubAllGlobals() }
  })

  it('waits for the server response before dispatching an action', async () => {
    let release!: (value: { content: string }) => void
    const infer = vi.fn((_prompt: string, _signal: AbortSignal) => new Promise<{ content: string }>(resolve => { release = resolve }))
    const action = vi.fn(() => true)
    const agent = createAssistantNavigationAgent({ scope: () => 'user:org', onChange: vi.fn(), onAction: action, dependencies: { infer } })
    const pending = agent.send('Kan du vise meg Kunnskap?')
    expect(infer.mock.calls[0]![0]).toBe('Kan du vise meg Kunnskap?')
    expect(action).not.toHaveBeenCalled()
    release({ content: proposal })
    await pending
    expect(action).toHaveBeenCalledOnce()
    expect(agent.state().status).toBe('idle')
  })

  it('does not act on unsupported, invalid or failed model responses', async () => {
    for (const result of ['{"type":"unsupported"}', '{}', 'failure']) {
      const action = vi.fn(() => true)
      const infer = vi.fn(async () => { if (result === 'failure') throw new Error('Model unavailable'); return { content: result } })
      const agent = createAssistantNavigationAgent({ scope: () => 'user:org', onChange: vi.fn(), onAction: action, dependencies: { infer } })
      await expect(agent.send('Find something')).rejects.toThrow()
      expect(action).not.toHaveBeenCalled()
      expect(agent.state().status).toBe('error')
    }
  })

  it('never dispatches after cancellation or an identity change', async () => {
    for (const change of ['stop', 'scope']) {
      let release!: (value: { content: string }) => void
      let scope = 'one'
      const action = vi.fn(() => true)
      const infer = vi.fn((_prompt: string, _signal: AbortSignal) => new Promise<{ content: string }>(resolve => { release = resolve }))
      const agent = createAssistantNavigationAgent({ scope: () => scope, onChange: vi.fn(), onAction: action, dependencies: { infer } })
      const result = agent.send('Find knowledge')
      if (change === 'stop') agent.stop(); else scope = 'two'
      if (change === 'stop') expect(infer.mock.calls[0]![1].aborted).toBe(true)
      release({ content: proposal })
      await expect(result).rejects.toThrow()
      expect(action).not.toHaveBeenCalled()
    }
  })
})
