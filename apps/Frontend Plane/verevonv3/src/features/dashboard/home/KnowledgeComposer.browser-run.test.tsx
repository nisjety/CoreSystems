// @vitest-environment jsdom
import { fireEvent, render, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createSignal, flush } from 'solid-js'

// The workspace lookup is the only thing mocked. The composer goes through the real
// clients and the real transport, so these tests cover the whole path from the AI-loop
// button and the blocked notice to the request the gateway receives.
vi.mock('@/shared/api/auth-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/auth-client')>()),
  getAuthSession: vi.fn(async () => ({ user: { id: 'user-1' } })),
  getSessionContext: vi.fn(async () => ({ orgs: [{ id: 'org-1' }] })),
}))

import { ERROR_COPY } from '@/shared/i18n/errors'
import { AssistantBrowserContext } from '@/features/core/assistant-agent/assistant-browser-context'
import type { AssistantAgentView } from '@/features/core/components/GlobalAssistant'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'
import { KnowledgeComposer } from './KnowledgeComposer'

type Call = { body: unknown; headers: Headers; method: string; path: string }

function json(status: number, body: unknown, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), { headers: { 'Content-Type': 'application/json', ...headers }, status })
}

const STRONG_CHALLENGE = { block_reason: 'waf_interstitial', kind: 'waf', recovery: 'available', strength: 'strong' }

function liveSession(challenge?: unknown, id = 'sess-1') {
  return {
    data: {
      observation: { challenge, run_id: id, step: 0, title: 'Example', url: 'https://example.test/' },
      session: {
        capabilities: ['navigate'],
        control: { mode: 'agent_control' },
        id,
        profile: { scope: 'run_scoped', storage: 'isolated' },
        renderMode: 'chromium',
        status: 'live',
        tabs: [],
        title: 'Example',
        url: 'https://example.test/',
        viewport: { height: 800, width: 1280 },
      },
    },
  }
}

/** A gateway that serves a live browser preview and records every call. `aiRuns` and
 * `execute` script the answers to the AI-run start and to the action call, in order
 * (the last one repeats). Everything else the composer asks for answers 404. */
function gateway(options: { aiRuns?: Array<() => Response>; challenge?: unknown; execute?: Array<() => Response>; browserAction?: () => Response; browserOwner?: () => Response; browserRefused?: boolean; browserSession?: () => Promise<Response>; browserControl?: () => Response; controlRefused?: boolean; settledTabUrl?: string }) {
  const calls: Call[] = []
  const next = (script: Array<() => Response> | undefined, fallback: () => Response, index: number) =>
    (script?.[Math.min(index, (script?.length ?? 1) - 1)] ?? fallback)()
  let aiRunIndex = 0
  let executeIndex = 0
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url
      const path = url.replace(/^https?:\/\/[^/]+/, '')
      const method = init?.method ?? 'GET'
      let body: unknown = null
      try {
        body = typeof init?.body === 'string' ? JSON.parse(init.body) : null
      } catch {
        body = null
      }
      calls.push({ body, headers: new Headers(init?.headers), method, path })
      if (path.endsWith('/api/v1/knowledge/scrape-preview')) {
        return json(200, { data: { description: '', markdown: '# Example', source: 'inline', title: 'Example', url: 'https://example.test/' } })
      }
      if (method === 'POST' && path.endsWith('/api/v1/browser/sessions')) return options.browserSession ? options.browserSession() : options.browserRefused ? json(403, { error: { code: 'browser_grant_denied', message: 'Not allowed' } }) : json(200, liveSession(options.challenge))
      if (options.browserOwner && method === 'GET' && path.endsWith('/api/v1/browser/sessions/sess-1')) return options.browserOwner()
      if (method === 'POST' && path.endsWith('/api/v1/browser/sessions/sess-1/control')) {
        if (options.browserControl) return options.browserControl()
        if (options.controlRefused) return json(403, { error: { code: 'forbidden', message: 'Not allowed' } })
        const response = liveSession(options.challenge)
        return json(200, { data: { ...response.data, session: { ...response.data.session, control: { mode: 'human_takeover', epoch: 1 } } } })
      }
      if (method === 'POST' && path.endsWith('/api/v1/browser/sessions/sess-1/actions')) {
        if (options.browserAction) return options.browserAction()
        if (options.settledTabUrl) return json(200, liveSession())
      }
      if (options.settledTabUrl && method === 'GET' && path.endsWith('/api/v1/browser/sessions/sess-1/tabs')) return json(200, { data: { tabs: [{ active: true, tabId: 'tab-1', title: 'New page', url: options.settledTabUrl }] } })
      if (method === 'POST' && path.endsWith('/api/v1/browser/sessions/sess-1/ai-runs')) {
        return next(options.aiRuns, () => json(200, { data: { plan_id: 'plan-1', run_id: 'run-1', thread_id: 'thread-1' } }), aiRunIndex++)
      }
      if (method === 'GET' && path.endsWith('/api/v1/runs/run-1/events')) {
        return new Response('', { headers: { 'Content-Type': 'text/event-stream' }, status: 200 })
      }
      if (method === 'POST' && path.endsWith('/api/v1/actions/execute')) {
        return next(
          options.execute,
          () => json(200, { data: { actionId: 'browser.blocked.try_other_source', auditId: 'a', eventStream: '', runId: 'r', status: 'completed' } }),
          executeIndex++,
        )
      }
      return json(404, { error: { code: 'not_found', message: 'not found' } })
    }),
  )
  return calls
}

const started = { data: { plan_id: 'plan-1', run_id: 'run-1', thread_id: 'thread-1' } }

async function openPreview(container: HTMLElement) {
  const extract = [...container.querySelectorAll<HTMLButtonElement>('.dashboard-knowledge-composer__modes button')].find(button => button.textContent?.includes('Extract'))
  if (extract?.getAttribute('aria-pressed') !== 'true') fireEvent.click(extract!)
  const input = await waitFor(() => {
    const field = container.querySelector<HTMLInputElement>('.dashboard-knowledge-composer__input')
    expect(field).not.toBeNull()
    expect(field!.disabled).toBe(false)
    return field!
  })
  fireEvent.input(input, { target: { value: 'https://example.test/' } })
  await waitFor(() => expect(container.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(false))
  fireEvent.submit(container.querySelector('form')!)
  await waitFor(() => expect(container.querySelector('.knowledge-browser-chrome')).not.toBeNull())
}

const loopButton = (container: HTMLElement) =>
  container.querySelector<HTMLButtonElement>('button[aria-label="Kjør flere AI-foreslåtte nettlesersteg"]')!
const aiRunStarts = (calls: Call[]) => calls.filter((call) => call.method === 'POST' && call.path.endsWith('/sessions/sess-1/ai-runs'))
const keyOf = (call: Call) => call.headers.get('Idempotency-Key')
const noticeOf = (container: HTMLElement) => container.querySelector<HTMLElement>('.knowledge-browser-blocked')
const leaveButton = (container: HTMLElement) =>
  [...container.querySelectorAll<HTMLButtonElement>('.knowledge-browser-blocked__button')].find((candidate) => /annen kilde/.test(candidate.textContent ?? ''))!

describe('browser in Nett', () => {
  beforeEach(() => { window.localStorage.clear(); window.sessionStorage.clear() })
  afterEach(() => { clearSession(); vi.unstubAllGlobals(); vi.unstubAllEnvs() })

  it('opens a browser session directly and closes it on unmount without scraping', async () => {
    const response = liveSession()
    const calls = gateway({ browserSession: async () => json(200, { data: { ...response.data, session: { ...response.data.session, control: { mode: 'agent_control', epoch: 7 } } } }) })
    const view = render(() => <KnowledgeComposer browserOnly browserUrl="https://example.test/" />)
    await waitFor(() => expect(view.container.querySelector('.knowledge-browser-chrome')).not.toBeNull())
    expect(calls.find(call => call.method === 'POST' && call.path.endsWith('/api/v1/browser/sessions'))?.body).toMatchObject({
      purpose: 'personal_browsing',
      url: 'https://example.test/',
    })
    expect(calls.find(call => call.path.endsWith('/sessions/sess-1/control'))?.body).toEqual({ mode: 'human_takeover', expectedEpoch: 7 })
    expect(view.container.querySelector('.browser-control-strip[data-control-mode="human_takeover"] button')?.textContent).toContain('Gi agenten kontroll')
    expect(view.container.textContent).toContain('Du styrer nettleseren.')
    expect(view.container.textContent).not.toContain('Agenten er satt på pause')
    expect(calls.filter(call => call.path.includes('scrape-preview'))).toHaveLength(0)
    expect(view.container.querySelector('.dashboard-knowledge-composer__modes')).toBeNull()
    view.unmount()
    await waitFor(() => expect(calls.some(call => call.method === 'DELETE' && call.path.endsWith('/sessions/sess-1'))).toBe(true))
  })

  it('refreshes the actual tab address when navigation settles after the immediate observation', async () => {
    const calls = gateway({ settledTabUrl: 'https://example.test/next' })
    const view = render(() => <KnowledgeComposer browserOnly browserUrl="https://example.test/" />)
    await waitFor(() => expect(view.container.querySelector('.knowledge-browser-chrome')).not.toBeNull())
    fireEvent.click(view.container.querySelector<HTMLButtonElement>('button[aria-label="Last nettlesersiden på nytt"]')!)
    await waitFor(() => expect(calls.some(call => call.method === 'GET' && call.path.endsWith('/sessions/sess-1/tabs'))).toBe(true), { timeout: 3000 })
    await waitFor(() => expect(view.container.querySelector<HTMLInputElement>('input[aria-label="Nettleseradresse"]')?.value).toBe('https://example.test/next'))
    view.unmount()
  })

  it('keeps personal open-web browsing interactive while omitting agent controls', async () => {
    const response = liveSession()
    gateway({ browserSession: async () => json(200, { data: { ...response.data, session: {
      ...response.data.session,
      capabilities: ['navigate', 'human_browsing'],
      control: { mode: 'human_takeover', epoch: 1 },
    } } }) })
    const view = render(() => <KnowledgeComposer browserOnly browserUrl="https://example.test/" />)
    const address = await waitFor(() => {
      const input = view.container.querySelector<HTMLInputElement>('input[aria-label="Nettleseradresse"]')
      expect(input).not.toBeNull()
      return input!
    })
    expect(address.disabled).toBe(false)
    expect(view.container.textContent).toContain('Du styrer nettleseren.')
    expect(view.container.querySelector('.browser-control-strip__action')).toBeNull()
    fireEvent.click(view.container.querySelector<HTMLButtonElement>('button[aria-label="Flere valg"]')!)
    await waitFor(() => {
      const menu = view.container.querySelector('[aria-label="Flere nettleservalg"]')
      expect(menu?.textContent).toContain('Manuelle handlinger')
      expect(menu?.textContent).not.toMatch(/Foreslå nettlesersteg|Start AI-loop/)
    })
  })

  it('explains a refused site without pretending a page opened', async () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    const target = 'https://www.example.test/article?version=1'
    const browserSession = vi.fn(async () => json(200, liveSession()))
    browserSession.mockResolvedValueOnce(json(403, { error: { code: 'browser_grant_denied', message: 'Not allowed' } }))
    const calls = gateway({ browserSession })
    const view = render(() => <KnowledgeComposer browserOnly browserUrl={target} />)
    await waitFor(() => expect(view.container.querySelector('.dashboard-knowledge-composer__error')?.textContent).toContain('www.example.test'))
    expect(view.container.querySelector('.knowledge-browser-chrome')).toBeNull()
    expect(view.container.querySelector<HTMLAnchorElement>('a[href="/settings/browser-sites?host=www.example.test"]')?.target).toBe('_blank')
    expect(view.container.querySelector<HTMLAnchorElement>(`a[href="${target}"]`)?.target).toBe('_blank')
    const retry = [...view.container.querySelectorAll('button')].find(button => button.textContent === 'Prøv igjen')!
    fireEvent.click(retry)
    await waitFor(() => expect(view.container.querySelector('.knowledge-browser-chrome')).not.toBeNull())
    expect(calls.filter(call => call.method === 'POST' && call.path.endsWith('/api/v1/browser/sessions')).map(call => call.body)).toEqual([
      expect.objectContaining({ url: target }),
      expect.objectContaining({ url: target }),
    ])
  })

  it('offers a new session and an external link when the browser fails to open a page', async () => {
    vi.stubEnv('VITE_BROWSER_SITES', 'true')
    const target = 'https://www.example.test/article?version=1'
    const browserSession = vi.fn(async () => json(200, liveSession()))
    browserSession.mockResolvedValueOnce(json(502, { error: { code: 'DRIVER_FAILED', message: 'Internal driver failure' } }))
    const calls = gateway({ browserSession })
    const view = render(() => <KnowledgeComposer browserOnly browserUrl={target} />)

    expect((await view.findByRole('alert')).textContent).toContain('Verevons nettleserøkt feilet mens siden ble åpnet.')
    expect(view.queryByRole('link', { name: 'Se nettsidevalg' })).toBeNull()
    const external = view.getByRole('link', { name: 'Åpne utenfor Verevon' }) as HTMLAnchorElement
    expect(external.href).toBe(target)
    expect(external.target).toBe('_blank')
    expect(external.rel).toBe('noopener noreferrer')

    fireEvent.click(view.getByRole('button', { name: 'Start ny nettleserøkt' }))
    await waitFor(() => expect(view.container.querySelector('.knowledge-browser-chrome')).not.toBeNull())
    expect(view.queryByRole('alert')).toBeNull()
    expect(calls.filter(call => call.method === 'POST' && call.path.endsWith('/api/v1/browser/sessions')).map(call => call.body)).toEqual([
      expect.objectContaining({ purpose: 'personal_browsing', url: target }),
      expect.objectContaining({ purpose: 'personal_browsing', url: target }),
    ])
  })

  it('does not offer to open a site outside Verevon for an explicit security block', async () => {
    gateway({ browserSession: async () => json(403, { error: { code: 'SECURITY_BLOCKED', message: 'Blocked' } }) })
    const view = render(() => <KnowledgeComposer browserOnly browserUrl="https://www.example.test/" />)
    expect((await view.findByRole('alert')).textContent).toContain(ERROR_COPY.SECURITY_BLOCKED!.no)
    expect(view.queryByRole('link', { name: 'Åpne utenfor Verevon' })).toBeNull()
    expect(view.queryByRole('button', { name: 'Start ny nettleserøkt' })).toBeNull()
  })

  it('distinguishes a failed site response from a browser permission denial', async () => {
    gateway({ browserSession: async () => json(502, { error: {
      code: 'DRIVER_FAILED',
      message: 'chromiumoxide goto failed',
      details: { error: 'Navigation failed: net::ERR_HTTP_RESPONSE_CODE_FAILURE' },
    } }) })
    const view = render(() => <KnowledgeComposer browserOnly browserUrl="https://www.example.test/" />)
    expect((await view.findByRole('alert')).textContent).toBe('Nettstedet svarte med en feil da Verevon prøvde å åpne siden.')
    expect(view.getByRole('link', { name: 'Åpne utenfor Verevon' })).toBeTruthy()
    expect(view.queryByRole('link', { name: 'Se nettsidevalg' })).toBeNull()
  })

  it('keeps a failed site response in the browser and retries in the same tab', async () => {
    const target = 'https://www.example.test/article?version=1'
    const start = liveSession().data
    const failed = {
      observation: null,
      session: {
        ...start.session,
        title: '',
        url: 'about:blank',
        tabs: [{ active: true, tabId: 'tab-1', title: 'Tom fane', url: 'about:blank' }],
      },
    }
    const control = { ...failed, session: { ...failed.session, control: { mode: 'human_takeover', epoch: 1 } } }
    const browserAction = vi.fn(() => json(200, { data: {
      ...control,
      session: {
        ...control.session,
        url: target,
        tabs: [{ active: true, tabId: 'tab-1', title: 'Artikkel', url: target }],
      },
    } }))
    browserAction.mockReturnValueOnce(json(502, { error: {
      code: 'DRIVER_FAILED',
      message: 'Raw driver message',
      details: { reason: 'navigation_http_response', http_status: 503 },
    } }))
    const calls = gateway({
      browserAction,
      browserSession: async () => json(200, { data: { ...failed, navigationError: {
        code: 'browser_initial_navigation_failed',
        httpStatus: 403,
      } } }),
      browserControl: () => json(200, { data: control }),
    })
    const view = render(() => <KnowledgeComposer browserOnly browserUrl={target} />)

    expect((await view.findByRole('alert')).textContent).toContain('Nettstedet svarte med HTTP 403')
    expect(view.getByRole('tab', { name: 'Tom fane' }).getAttribute('aria-selected')).toBe('true')
    expect(view.queryByRole('link', { name: 'Se nettsidevalg' })).toBeNull()
    const external = view.getByRole('link', { name: 'Åpne utenfor Verevon' }) as HTMLAnchorElement
    expect(external.href).toBe(target)
    expect(external.target).toBe('_blank')
    expect(external.rel).toBe('noopener noreferrer')

    fireEvent.click(view.getByRole('button', { name: 'Prøv igjen' }))
    await waitFor(() => expect(view.getByRole('alert').textContent).toContain('Nettstedet svarte med HTTP 503'))
    expect(view.getByRole('alert').textContent).not.toContain('Raw driver message')
    fireEvent.click(view.getByRole('button', { name: 'Prøv igjen' }))
    await waitFor(() => expect(view.queryByRole('alert')).toBeNull())
    expect(view.getByRole('tab', { name: 'Artikkel' }).getAttribute('aria-selected')).toBe('true')
    expect(calls.filter(call => call.method === 'POST' && call.path.endsWith('/sessions/sess-1/actions')).map(call => call.body)).toEqual([
      expect.objectContaining({ action: { type: 'navigate', url: target } }),
      expect.objectContaining({ action: { type: 'navigate', url: target } }),
    ])
    expect(calls.filter(call => call.method === 'POST' && call.path.endsWith('/api/v1/browser/sessions'))).toHaveLength(1)
    expect(calls.some(call => call.method === 'DELETE' && call.path.endsWith('/sessions/sess-1'))).toBe(false)
    view.unmount()
  })

  it('waits for the same tab holder after the assistant task becomes terminal and the browser is visible', async () => {
    setSessionUser({ id: 'user-1', email: 'user@example.com', name: 'User', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org-1', name: 'Verevon', role: 'owner' })
    flush()
    const finalUrl = 'https://example.test/after-assistant'
    const ownerProjection = (holder: 'agent' | 'human') => json(200, { data: {
      runId: 'sess-1',
      live: true,
      controlMode: 'human_takeover',
      controlEpoch: 3,
      viewport: { width: 1280, height: 800 },
      zdr: false,
      profileScope: 'ephemeral',
      profileStorage: 'ephemeral',
      currentUrl: finalUrl,
      tabs: [
        { active: false, tabId: 'other-tab', holder: 'human', url: 'https://example.test/other' },
        { active: true, tabId: 'tab-1', holder, title: 'Resultat', url: finalUrl },
      ],
    } })
    const browserOwner = vi.fn(() => ownerProjection('human')).mockReturnValueOnce(ownerProjection('agent'))
    const calls = gateway({ browserOwner })
    const [assistantView, setAssistantView] = createSignal<AssistantAgentView | null>(null)
    const [visible, setVisible] = createSignal(true)
    const view = render(() => (
      <AssistantBrowserContext value={{ view: assistantView, stop: () => undefined }}>
        <KnowledgeComposer browserOnly browserContextVisible={visible()} browserUrl="https://example.test/" />
      </AssistantBrowserContext>
    ))
    await waitFor(() => expect(view.container.querySelector('.knowledge-browser-chrome')).not.toBeNull())
    setVisible(false)
    const completed: AssistantAgentView = {
      available: true, kind: 'browser', status: 'stopped', usesExistingTab: true, borrowedTabId: 'tab-1',
      goal: 'Les siden', error: null, action: null, sessionId: 'sess-1', runId: 'run-1', receipt: null,
    }
    setAssistantView(completed)
    flush()
    expect(browserOwner).not.toHaveBeenCalled()
    setVisible(true)
    const address = () => view.container.querySelector<HTMLInputElement>('input[aria-label="Nettleseradresse"]')
    await waitFor(() => expect(address()?.disabled).toBe(true))
    expect(address()?.value).toBe('https://example.test/')
    await waitFor(() => expect(address()?.value).toBe(finalUrl))
    expect(address()?.disabled).toBe(false)
    setAssistantView({ ...completed, result: 'Ferdig' })
    setVisible(false)
    flush()
    setVisible(true)
    flush()
    expect(browserOwner).toHaveBeenCalledTimes(2)
    expect(calls.filter(call => call.method === 'POST' && call.path.endsWith('/api/v1/browser/sessions'))).toHaveLength(1)
    expect(calls.some(call => call.method === 'DELETE' && call.path.endsWith('/sessions/sess-1'))).toBe(false)
    view.unmount()
  })

  it('offers a fresh browser when the borrowed session has expired', async () => {
    setSessionUser({ id: 'user-1', email: 'user@example.com', name: 'User', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org-1', name: 'Verevon', role: 'owner' })
    flush()
    const newSession = liveSession(undefined, 'sess-2')
    const browserSession = vi.fn(async () => json(200, { data: { ...newSession.data, session: {
      ...newSession.data.session, control: { mode: 'human_takeover', epoch: 1 },
    } } })).mockResolvedValueOnce(json(200, liveSession()))
    const browserOwner = vi.fn(() => json(404, { error: { code: 'browser_session_not_found', message: 'Expired' } }))
    const calls = gateway({ browserSession, browserOwner })
    const [assistantView, setAssistantView] = createSignal<AssistantAgentView | null>(null)
    const view = render(() => (
      <AssistantBrowserContext value={{ view: assistantView, stop: () => undefined }}>
        <KnowledgeComposer browserOnly browserContextVisible browserUrl="https://example.test/" />
      </AssistantBrowserContext>
    ))
    const address = () => view.container.querySelector<HTMLInputElement>('input[aria-label="Nettleseradresse"]')
    await waitFor(() => expect(address()?.disabled).toBe(false))
    setAssistantView({
      available: true, kind: 'browser', status: 'completed', usesExistingTab: true, borrowedTabId: 'tab-1',
      goal: 'Les siden', error: null, action: null, sessionId: 'sess-1', runId: 'run-1', receipt: null,
    })
    expect((await view.findByRole('alert')).textContent).toContain('Nettleserøkten er avsluttet')
    expect(address()?.disabled).toBe(true)
    expect(browserOwner).toHaveBeenCalledTimes(1)
    fireEvent.click(view.getByRole('button', { name: 'Start ny nettleserøkt' }))
    await waitFor(() => expect(address()?.disabled).toBe(false))
    expect(view.queryByRole('alert')).toBeNull()
    expect(calls.filter(call => call.method === 'POST' && call.path.endsWith('/api/v1/browser/sessions'))).toHaveLength(2)
    view.unmount()
  })

  it('does not claim the person controls a browser when takeover is denied', async () => {
    gateway({ controlRefused: true })
    const view = render(() => <KnowledgeComposer browserOnly browserUrl="https://example.test/" />)
    await waitFor(() => expect(view.container.querySelector('.knowledge-browser-chrome')).not.toBeNull())
    expect(view.container.querySelector('.browser-control-strip[data-control-mode="agent_control"] button')?.textContent).toContain('Ta over')
    expect(view.container.textContent).not.toContain('Du styrer nettleseren.')
    expect(view.container.querySelector('.dashboard-knowledge-composer__error')).not.toBeNull()
  })

})

describe('KnowledgeComposer AI-run start', () => {
  beforeEach(() => {
    try {
      window.localStorage.clear()
    } catch {
      /* storage is optional */
    }
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  async function startLoop(container: HTMLElement, expectedStarts: number, calls: Call[]) {
    await waitFor(() => expect(loopButton(container).disabled).toBe(false))
    fireEvent.click(loopButton(container))
    await waitFor(() => expect(aiRunStarts(calls)).toHaveLength(expectedStarts))
  }

  it('sends an Idempotency-Key with the start', async () => {
    const calls = gateway({})
    const view = render(() => <KnowledgeComposer />)
    await openPreview(view.container)
    expect(calls.find(call => call.method === 'POST' && call.path.endsWith('/api/v1/browser/sessions'))?.body).not.toHaveProperty('purpose')

    await startLoop(view.container, 1, calls)

    expect(keyOf(aiRunStarts(calls)[0]!)).toMatch(/^ai-run-[A-Za-z0-9-]+$/)
  })

  it('retries one start under one key after a lost connection, and a new submit gets a new key', async () => {
    const calls = gateway({
      aiRuns: [
        () => {
          throw new TypeError('Failed to fetch')
        },
        () => json(200, started),
      ],
    })
    const view = render(() => <KnowledgeComposer />)
    await openPreview(view.container)

    await startLoop(view.container, 1, calls)
    // The first attempt failed without an answer: the person tries the same start again.
    await waitFor(() => expect(view.container.querySelector('.dashboard-knowledge-composer__error')).not.toBeNull())
    await startLoop(view.container, 2, calls)
    // That one was accepted and its stream has ended; the next start is a new submit.
    await waitFor(() => expect(view.container.querySelector('.knowledge-browser-loop-chip--done')).not.toBeNull())
    await startLoop(view.container, 3, calls)

    const [first, retry, fresh] = aiRunStarts(calls).map(keyOf)
    expect(first).toMatch(/^ai-run-/)
    expect(retry).toBe(first)
    expect(fresh).toMatch(/^ai-run-/)
    expect(fresh).not.toBe(first)
  })

  it('uses a new key for the next start after the gateway refused the key (409)', async () => {
    const calls = gateway({
      aiRuns: [
        () => json(409, { error: { code: 'browser_idempotency_key_reused', message: 'raw gateway text' } }),
        () => json(200, started),
      ],
    })
    const view = render(() => <KnowledgeComposer />)
    await openPreview(view.container)

    await startLoop(view.container, 1, calls)
    await waitFor(() => expect(view.container.querySelector('.dashboard-knowledge-composer__error')?.textContent).toBe(ERROR_COPY.browser_idempotency_key_reused!.no))
    await startLoop(view.container, 2, calls)

    const [first, second] = aiRunStarts(calls).map(keyOf)
    expect(second).not.toBe(first)
  })

  it('never displays or removes a saved extraction while opening or closing the browser', async () => {
    const key = 'verevon.dashboard.browser.preview.org-1'
    const saved = JSON.stringify({ browserRationales: [], savedAt: '2026-01-01T00:00:00Z', preview: { blocks: [{ heading: false, raw: 'old', text: 'old' }], charCount: 3, description: '', markdown: 'old', source: 'inline', title: 'Saved extraction', url: 'https://old.test/' } })
    window.localStorage.setItem(key, saved)
    let complete!: (response: Response) => void
    const calls = gateway({ browserSession: () => new Promise(resolve => { complete = resolve }) })
    const view = render(() => <KnowledgeComposer browserOnly browserUrl="https://example.test/" />)
    await waitFor(() => expect(view.container.textContent).toContain('Åpner nettleseren'))
    expect(view.container.textContent).not.toContain('Saved extraction')
    view.unmount()
    complete(json(200, liveSession()))
    await waitFor(() => expect(calls.some(call => call.method === 'DELETE' && call.path.endsWith('/sessions/sess-1'))).toBe(true))
    expect(window.localStorage.getItem(key)).toBe(saved)
  })

  it('clears the old page while a replacement browser session opens', async () => {
    let complete!: (response: Response) => void
    let attempts = 0
    gateway({ browserSession: () => ++attempts === 1 ? Promise.resolve(json(200, liveSession())) : new Promise(resolve => { complete = resolve }) })
    const [url, setUrl] = createSignal('https://example.test/')
    const view = render(() => <KnowledgeComposer browserOnly browserUrl={url()} />)
    await waitFor(() => expect(view.container.querySelector('.knowledge-browser-chrome')).not.toBeNull())
    setUrl('https://other.test/')
    await waitFor(() => expect(view.container.textContent).toContain('Åpner nettleseren'))
    expect(view.container.querySelector('.knowledge-browser-chrome')).toBeNull()
    view.unmount()
    complete(json(200, liveSession(undefined, 'sess-2')))
  })

  it('starts Hent ut on Crawl while retaining the option to restore a saved extraction', async () => {
    gateway({})
    const key = 'verevon.dashboard.browser.preview.org-1'
    const saved = JSON.stringify({ browserRationales: [], savedAt: '2026-01-01T00:00:00Z', preview: { blocks: [{ heading: false, raw: 'old', text: 'old' }], charCount: 3, description: '', markdown: 'old', source: 'inline', title: 'Saved extraction', url: 'https://old.test/' } })
    window.localStorage.setItem(key, saved)
    const view = render(() => <KnowledgeComposer />)
    await waitFor(() => expect(view.container.querySelector<HTMLInputElement>('.dashboard-knowledge-composer__input')?.disabled).toBe(false))
    const modes = [...view.container.querySelectorAll<HTMLButtonElement>('.dashboard-knowledge-composer__modes button')]
    expect(modes.find(button => button.textContent?.includes('Crawl'))?.getAttribute('aria-pressed')).toBe('true')
    expect(view.container.querySelector('.knowledge-browser-chrome')).toBeNull()
    expect(window.localStorage.getItem(key)).toBe(saved)
    fireEvent.click(modes.find(button => button.textContent?.includes('Extract'))!)
    const resume = await waitFor(() => {
      const button = [...view.container.querySelectorAll<HTMLButtonElement>('button')].find(candidate => candidate.textContent?.includes('Fortsett forrige uthenting'))
      expect(button).toBeTruthy()
      return button!
    })
    fireEvent.click(resume!)
    await waitFor(() => expect(view.container.textContent).toContain('Saved extraction'))
  })
})

describe('KnowledgeComposer blocked notice', () => {
  beforeEach(() => {
    try {
      window.localStorage.clear()
    } catch {
      /* storage is optional */
    }
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  const actionCalls = (calls: Call[]) => calls.filter((call) => call.path.endsWith('/api/v1/actions/execute'))

  it('shows no notice for a page that is not a bot wall', async () => {
    gateway({ challenge: { kind: 'login', strength: 'non_signal', requires_escalation: false } })
    const view = render(() => <KnowledgeComposer />)
    await openPreview(view.container)
    expect(noticeOf(view.container)).toBeNull()
  })

  it('shows the notice for a strong signal, and "Try another source" runs the action and leaves the site', async () => {
    const calls = gateway({ challenge: STRONG_CHALLENGE })
    const view = render(() => <KnowledgeComposer />)
    await openPreview(view.container)
    expect(noticeOf(view.container)).not.toBeNull()

    fireEvent.click(leaveButton(view.container))

    await waitFor(() => expect(view.container.querySelector('.knowledge-browser-chrome')).toBeNull())
    const [call] = actionCalls(calls)
    expect(actionCalls(calls)).toHaveLength(1)
    expect(call!.body).toMatchObject({ actionId: 'browser.blocked.try_other_source', input: { sessionId: 'sess-1' } })
    expect((call!.body as { input: Record<string, unknown> }).input).not.toHaveProperty('runId')
    expect(call!.headers.get('x-verevon-org-id')).toBe('org-1')
  })

  it('names the AI run to stop when one was started', async () => {
    const calls = gateway({ challenge: STRONG_CHALLENGE })
    const view = render(() => <KnowledgeComposer />)
    await openPreview(view.container)
    await waitFor(() => expect(loopButton(view.container).disabled).toBe(false))
    fireEvent.click(loopButton(view.container))
    await waitFor(() => expect(aiRunStarts(calls)).toHaveLength(1))
    await waitFor(() => expect(calls.some((call) => call.path.endsWith('/runs/run-1/events'))).toBe(true))

    fireEvent.click(leaveButton(view.container))

    await waitFor(() => expect(actionCalls(calls)).toHaveLength(1))
    expect(actionCalls(calls)[0]!.body).toMatchObject({ input: { runId: 'run-1', sessionId: 'sess-1' } })
  })

  async function startRunOn(container: HTMLElement, calls: Call[]) {
    await waitFor(() => expect(loopButton(container).disabled).toBe(false))
    fireEvent.click(loopButton(container))
    await waitFor(() => expect(aiRunStarts(calls)).toHaveLength(1))
    await waitFor(() => expect(calls.some((call) => call.path.endsWith('/runs/run-1/events'))).toBe(true))
  }

  it('does not name the run of an earlier preview after Discard and a new preview', async () => {
    const calls = gateway({ challenge: STRONG_CHALLENGE })
    const view = render(() => <KnowledgeComposer />)
    await openPreview(view.container)
    await startRunOn(view.container, calls)

    fireEvent.click(view.container.querySelector<HTMLButtonElement>('.knowledge-browser-close')!)
    await waitFor(() => expect(view.container.querySelector('.knowledge-browser-chrome')).toBeNull())
    await openPreview(view.container)
    fireEvent.click(leaveButton(view.container))

    await waitFor(() => expect(actionCalls(calls)).toHaveLength(1))
    expect((actionCalls(calls)[0]!.body as { input: Record<string, unknown> }).input).toEqual({ sessionId: 'sess-1' })
  })

  it('keeps the preview and says why when the action fails, so Discard still works', async () => {
    const calls = gateway({
      challenge: STRONG_CHALLENGE,
      execute: [() => json(503, { error: { code: 'browser_unhealthy', message: 'raw gateway text at https://secret.example' } })],
    })
    const view = render(() => <KnowledgeComposer />)
    await openPreview(view.container)

    fireEvent.click(leaveButton(view.container))

    const shown = await waitFor(() => {
      const error = view.container.querySelector('.dashboard-knowledge-composer__error')
      expect(error).not.toBeNull()
      return error!.textContent ?? ''
    })
    expect(actionCalls(calls)).toHaveLength(1)
    expect(shown).toContain(ERROR_COPY.browser_unhealthy!.no)
    expect(shown).not.toMatch(/secret\.example|raw gateway/)
    expect(view.container.querySelector('.knowledge-browser-chrome')).not.toBeNull()
  })

  const receipt = (result: unknown) =>
    json(200, { data: { actionId: 'browser.blocked.try_other_source', auditId: 'a', eventStream: '', result, runId: 'r', status: 'completed' } })
  const leaveNoticeOf = (container: HTMLElement) =>
    container.querySelector<HTMLElement>('.dashboard-knowledge-composer__status[role="status"]')

  it('leaves the site and tells the person when the run could not be stopped (run controls off)', async () => {
    const calls = gateway({
      challenge: STRONG_CHALLENGE,
      execute: [() => receipt({ closed: true, runStopped: false, runControlsDisabled: true })],
    })
    const view = render(() => <KnowledgeComposer />)
    await openPreview(view.container)
    await startRunOn(view.container, calls)

    fireEvent.click(leaveButton(view.container))

    await waitFor(() => expect(view.container.querySelector('.knowledge-browser-chrome')).toBeNull())
    const notice = await waitFor(() => {
      const found = leaveNoticeOf(view.container)
      expect(found).not.toBeNull()
      return found!.textContent ?? ''
    })
    expect(notice).toContain('kjøringskontroller er slått av')
    expect(view.container.querySelector('.dashboard-knowledge-composer__error')).toBeNull()
  })

  it('shows no such note when the run was stopped, or when there was none', async () => {
    const calls = gateway({
      challenge: STRONG_CHALLENGE,
      execute: [() => receipt({ closed: true, runStopped: true })],
    })
    const view = render(() => <KnowledgeComposer />)
    await openPreview(view.container)
    await startRunOn(view.container, calls)

    fireEvent.click(leaveButton(view.container))

    await waitFor(() => expect(view.container.querySelector('.knowledge-browser-chrome')).toBeNull())
    expect(leaveNoticeOf(view.container)).toBeNull()
  })

  it('drops the note when the next link is submitted', async () => {
    gateway({
      challenge: STRONG_CHALLENGE,
      execute: [() => receipt({ closed: true, runStopped: false, runControlsDisabled: true })],
    })
    const view = render(() => <KnowledgeComposer />)
    await openPreview(view.container)
    fireEvent.click(leaveButton(view.container))
    await waitFor(() => expect(leaveNoticeOf(view.container)).not.toBeNull())

    await openPreview(view.container)

    expect(leaveNoticeOf(view.container)).toBeNull()
  })
})
