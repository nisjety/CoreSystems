// @vitest-environment jsdom
import { fireEvent, render, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// W2-U65-51: the crawl scope options and the scrape captures in the composer. The real
// knowledge client and transport run; only the workspace lookup and the network are stubbed.
vi.mock('@/shared/api/auth-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/auth-client')>()),
  getAuthSession: vi.fn(async () => ({ user: { id: 'user-1' } })),
  getSessionContext: vi.fn(async () => ({ orgs: [{ id: 'org-1' }] })),
}))

import { KnowledgeComposer } from './KnowledgeComposer'

const FLAG = 'VITE_KNOWLEDGE_SCRAPE_OPTIONS_ENABLED'

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { headers: { 'Content-Type': 'application/json' }, status })
}

interface Sent {
  url: string
  body: Record<string, unknown> | undefined
}

function stubGateway(): Sent[] {
  const sent: Sent[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url
      const body = typeof init?.body === 'string' ? (JSON.parse(init.body) as Record<string, unknown>) : undefined
      sent.push({ body, url })
      if (url.includes('/api/v1/knowledge/scrape-preview')) {
        return json(200, { data: { markdown: '# Hei', title: 'Hei', url: 'https://example.com/page' } })
      }
      if (url.endsWith('/api/v1/knowledge/scrape')) {
        return json(200, { data: { formats: { screenshot: 'art_01', pdf: 'art_02' }, markdown: '# Hei' } })
      }
      if (url.endsWith('/api/v1/knowledge/crawl')) {
        return json(200, { data: { id: 'run-1', status: 'running' } })
      }
      return json(404, { error: { code: 'not_found', message: 'not found' } })
    }),
  )
  return sent
}

async function mounted() {
  const view = render(() => <KnowledgeComposer />)
  const input = await waitFor(() => {
    const field = view.container.querySelector<HTMLInputElement>('.dashboard-knowledge-composer__input')
    expect(field).not.toBeNull()
    expect(field!.disabled).toBe(false)
    return field!
  })
  return { input, view }
}

async function submit(view: ReturnType<typeof render>, input: HTMLInputElement, value: string) {
  fireEvent.input(input, { target: { value } })
  await waitFor(() => expect(view.container.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(false))
  fireEvent.submit(view.container.querySelector('form')!)
}

const pick = (sent: Sent[], needle: string) => sent.filter((call) => call.url.includes(needle))
const scrapeCall = (sent: Sent[]) => sent.find((call) => call.url.endsWith('/api/v1/knowledge/scrape'))
const crawlCall = (sent: Sent[]) => sent.find((call) => call.url.endsWith('/api/v1/knowledge/crawl'))

function crawlModeButton(view: ReturnType<typeof render>): HTMLButtonElement {
  return [...view.container.querySelectorAll<HTMLButtonElement>('.dashboard-knowledge-composer__modes button')].find(
    (button) => button.textContent?.includes('Crawl'),
  )!
}

describe('KnowledgeComposer scrape options (W2-U65-51)', () => {
  beforeEach(() => {
    try {
      window.localStorage.clear()
    } catch {
      /* storage is optional */
    }
  })
  afterEach(() => {
    vi.unstubAllGlobals()
    vi.unstubAllEnvs()
  })

  it('flag off: no capture controls, the preview sends false captures and there is no /scrape call', async () => {
    const sent = stubGateway()
    const { input, view } = await mounted()
    expect(view.container.querySelector('[data-testid="scrape-capture-options"]')).toBeNull()
    await submit(view, input, 'https://example.com/page')
    await waitFor(() => expect(pick(sent, '/scrape-preview')).toHaveLength(1))
    const signals = pick(sent, '/scrape-preview')[0]!.body!.signals as Record<string, unknown>
    expect(signals.screenshot).toBe(false)
    expect(signals.pdf).toBe(false)
    expect(scrapeCall(sent)).toBeUndefined()
  })

  it('flag on, nothing ticked: still sends false and makes no /scrape call', async () => {
    vi.stubEnv(FLAG, 'true')
    const sent = stubGateway()
    const { input, view } = await mounted()
    expect(view.container.querySelector('[data-testid="scrape-capture-options"]')).not.toBeNull()
    await submit(view, input, 'https://example.com/page')
    await waitFor(() => expect(pick(sent, '/scrape-preview')).toHaveLength(1))
    const signals = pick(sent, '/scrape-preview')[0]!.body!.signals as Record<string, unknown>
    expect(signals.screenshot).toBe(false)
    expect(signals.pdf).toBe(false)
    expect(scrapeCall(sent)).toBeUndefined()
  })

  it('flag on, screenshot ticked: true for the screenshot only, and /scrape asks for the screenshot format', async () => {
    vi.stubEnv(FLAG, 'true')
    const sent = stubGateway()
    const { input, view } = await mounted()
    const boxes = view.container.querySelectorAll<HTMLInputElement>('[data-testid="scrape-capture-options"] input[type="checkbox"]')
    expect(boxes).toHaveLength(2)
    fireEvent.click(boxes[0]!)
    await submit(view, input, 'https://example.com/page')
    await waitFor(() => expect(scrapeCall(sent)).toBeDefined())
    const signals = pick(sent, '/scrape-preview')[0]!.body!.signals as Record<string, unknown>
    expect(signals.screenshot).toBe(true)
    expect(signals.pdf).toBe(false)
    const body = scrapeCall(sent)!.body!
    expect(body.formats).toEqual(['markdown', 'screenshot'])
    expect(body).not.toHaveProperty('capture')
    expect(body).not.toHaveProperty('ingest')
  })

  it('crawl, flag on: sends the typed scope, deduplicated, and the subdomain choice', async () => {
    vi.stubEnv(FLAG, 'true')
    const sent = stubGateway()
    const { input, view } = await mounted()
    fireEvent.click(crawlModeButton(view))
    const options = await waitFor(() => {
      const found = view.container.querySelector('.dashboard-knowledge-composer__crawl-options input[type="number"]')?.closest('div')
      expect(found).toBeTruthy()
      return found!
    })
    const texts = options.querySelectorAll<HTMLInputElement>('input[type="text"]')
    expect(texts).toHaveLength(2)
    fireEvent.input(texts[0]!, { target: { value: '/docs/**, /blog/**,/docs/**' } })
    fireEvent.input(texts[1]!, { target: { value: 're:/tag/.*' } })
    const numbers = options.querySelectorAll<HTMLInputElement>('input[type="number"]')
    fireEvent.input(numbers[1]!, { target: { value: '3' } })
    const boxes = options.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')
    fireEvent.click(boxes[1]!) // the agent toggle is [0], subdomains is [1]
    await submit(view, input, 'https://example.com')
    await waitFor(() => expect(crawlCall(sent)).toBeDefined())
    const body = crawlCall(sent)!.body!
    expect(body.includePaths).toEqual(['/docs/**', '/blog/**'])
    expect(body.excludePaths).toEqual(['re:/tag/.*'])
    expect(body.maxDepth).toBe(3)
    expect(body.allowSubdomains).toBe(true)
    expect(body).not.toHaveProperty('allowExternalLinks')
  })

  it('crawl, flag on but untouched: the request carries no scope keys', async () => {
    vi.stubEnv(FLAG, 'true')
    const sent = stubGateway()
    const { input, view } = await mounted()
    fireEvent.click(crawlModeButton(view))
    await submit(view, input, 'https://example.com')
    await waitFor(() => expect(crawlCall(sent)).toBeDefined())
    const body = crawlCall(sent)!.body!
    for (const key of ['includePaths', 'excludePaths', 'maxDepth', 'allowSubdomains', 'allowExternalLinks']) {
      expect(body).not.toHaveProperty(key)
    }
  })

  it('crawl, flag off: the scope controls are not rendered', async () => {
    stubGateway()
    const { view } = await mounted()
    fireEvent.click(crawlModeButton(view))
    await waitFor(() => expect(view.container.querySelector('.dashboard-knowledge-composer__crawl-options')).not.toBeNull())
    expect(view.container.querySelectorAll('.dashboard-knowledge-composer__crawl-options input[type="text"]')).toHaveLength(0)
  })
})
