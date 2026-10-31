// @vitest-environment jsdom
import { fireEvent, render, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// The workspace lookup is the only thing mocked. The composer goes through the real
// clients and the real transport, so these tests cover the whole path from the AI-loop
// button and the blocked notice to the request the gateway receives.
vi.mock('@/shared/api/auth-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/auth-client')>()),
  getAuthSession: vi.fn(async () => ({ user: { id: 'user-1' } })),
  getSessionContext: vi.fn(async () => ({ orgs: [{ id: 'org-1' }] })),
}))

import { ERROR_COPY } from '@/shared/i18n/errors'
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
function gateway(options: { aiRuns?: Array<() => Response>; challenge?: unknown; execute?: Array<() => Response> }) {
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
      if (method === 'POST' && path.endsWith('/api/v1/browser/sessions')) return json(200, liveSession(options.challenge))
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
