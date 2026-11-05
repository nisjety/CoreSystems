import { describe, expect, it, vi } from 'vitest'
import { createAssistantBrowserAgent } from './assistant-browser-agent'
import type { AssistantAgentView } from '@/features/core/components/GlobalAssistant'
import type { RunEventHandlers } from '@/shared/api/run-console-client'
import { ApiError } from '@/shared/api/api-error'
import { getRun } from '@/shared/api/runs-client'
import type { BrowserSiteExplanation } from '@/shared/api/browser-sites-client'

function explanation(host: string, effective: BrowserSiteExplanation['decision']['effective']): BrowserSiteExplanation {
  return {
    organization_id: 'org-1', user_id: 'user-1', revision: 1, mode: 'open',
    decision: { host, effective, cap: 'act', source: effective === 'ask' ? 'default' : 'user_pref' },
    org_rule: '', user_level: effective === 'ask' ? '' : effective === 'deny' ? 'none' : effective, user_expires_at: null,
  }
}

const siteDenied = () => new ApiError('Browser access was not granted.', 403, 'browser_grant_denied')
const redirectNeedsAccess = (host: string) => new ApiError('Redirect access denied.', 403, 'browser_redirect_access_denied', {
  details: { host, canRequestAccess: true },
})

function fixture(overrides: Record<string, unknown> = {}) {
  let handlers: RunEventHandlers = {}
  const pending = new Promise<void>(() => {})
  const api = {
    capability: vi.fn(async () => ({ available: true, policySupported: true, canAttach: true })),
    createSession: vi.fn(async () => ({ session: { id: 'session-1' } })),
    closeSession: vi.fn(async () => ({ closed: true })),
    explainSite: vi.fn(async () => null),
    setSiteChoice: vi.fn(async () => null),
    startRun: vi.fn(async () => ({ runId: 'run-1', planId: 'plan-1', threadId: 'thread-1' })),
    control: vi.fn(async () => {}),
    stream: vi.fn((_id: string, callbacks: RunEventHandlers) => { handlers = callbacks; return pending }),
    replay: vi.fn(() => pending),
    getRun: vi.fn(async () => null),
    decideApproval: vi.fn(async () => null),
    ...overrides,
  }
  let view: AssistantAgentView | undefined
  let scope: { userId: string; orgId: string } | null = { userId: 'user-1', orgId: 'org-1' }
  const controller = createAssistantBrowserAgent({
    scope: () => scope, onChange: next => { view = next }, tr: (_no, en) => en,
    dependencies: api as unknown as Parameters<typeof createAssistantBrowserAgent>[0]['dependencies'],
  })
  return { api, controller, view: () => view!, events: () => handlers, setScope: (next: typeof scope) => { scope = next } }
}

describe('assistant browser task', () => {
  it('explains a refused session grant without assuming who refused it', async () => {
    const f = fixture({ createSession: vi.fn(async () => { throw siteDenied() }) })
    await expect(f.controller.send('Inspect this page', 'https://example.com')).rejects.toThrow()
    expect(f.view().errorCode).toBe('browser_grant_denied')
    expect(f.view().error).toContain('administrator restrictions')
    expect(f.view().error).toContain('draft has been kept')
    expect(f.api.startRun).not.toHaveBeenCalled()
  })

  it('saves exact temporary site access only after approval, then retries the original task', async () => {
    const host = 'portal.example.net'
    const resolvedStartUrl = `https://${host}/catalog`
    const createSession = vi.fn(async () => ({ session: { id: 'session-1' }, resolvedStartUrl }))
      .mockRejectedValueOnce(redirectNeedsAccess(host))
    const explainSite = vi.fn(async () => ({ ...explanation(host, 'ask'), revision: 42 }))
    const f = fixture({ createSession, explainSite })
    const goal = 'Open example.com and summarize the page'
    const clock = vi.spyOn(Date, 'now').mockReturnValue(Date.parse('2026-01-01T10:00:00Z'))
    try {
      await expect(f.controller.send(goal, 'https://example.com')).rejects.toThrow('Redirect access denied')
      expect(f.view().siteAccessRequest).toEqual({ host })
      expect(f.view().error).toBeNull()
      expect(createSession).toHaveBeenCalledTimes(1)
      expect(explainSite).not.toHaveBeenCalled()
      expect(f.api.setSiteChoice).not.toHaveBeenCalled()
      expect(f.api.startRun).not.toHaveBeenCalled()

      await f.controller.allowSiteAccess()
      expect(explainSite).toHaveBeenCalledWith('org-1', host, expect.any(AbortSignal))
      expect(f.api.setSiteChoice).toHaveBeenCalledOnce()
      expect(f.api.setSiteChoice).toHaveBeenCalledWith('org-1', host, 42, 'act', '2026-01-01T10:10:00.000Z', expect.any(AbortSignal))
      expect(createSession).toHaveBeenCalledTimes(2)
      expect(createSession).toHaveBeenLastCalledWith('org-1', expect.objectContaining({ url: 'https://example.com/', purpose: 'assistant_task' }))
      expect(f.api.startRun).toHaveBeenCalledWith('org-1', 'session-1', expect.objectContaining({
        goal: `${goal}\n\nThe browser is already open on https://${host}. Continue from the current page.`,
        allowedDomains: [host], requireAttached: true, assistantInteractive: true,
      }), undefined, expect.anything())
      expect(f.view().goal).toBe(goal)
      expect(f.view().siteAccessRequest).toBeNull()
    } finally { clock.mockRestore(); f.controller.dispose() }
  })

  it.each([
    ['https://portal.example.net/catalog?from=landing', 'portal.example.net'],
    ['https://example.com/en-us/', 'example.com'],
  ])('starts one task at the resolved address %s returned by the gateway', async (resolvedStartUrl, hostname) => {
    const f = fixture({ createSession: vi.fn(async () => ({ session: { id: 'session-1' }, resolvedStartUrl })) })
    const goal = 'Open example.com and summarize the page'
    try {
      await f.controller.send(goal, 'https://example.com')
      expect(f.api.createSession).toHaveBeenCalledTimes(1)
      expect(f.api.createSession).toHaveBeenCalledWith('org-1', expect.objectContaining({
        url: 'https://example.com/', purpose: 'assistant_task', persistentProfile: false,
      }))
      expect(f.api.explainSite).not.toHaveBeenCalled()
      expect(f.api.startRun).toHaveBeenCalledTimes(1)
      expect(f.api.startRun).toHaveBeenCalledWith('org-1', 'session-1', expect.objectContaining({
        goal: `${goal}\n\nThe browser is already open on https://${hostname}. Continue from the current page.`,
        allowedDomains: [hostname], requireAttached: true, assistantInteractive: true,
      }), undefined, expect.anything())
      expect(f.view().goal).toBe(goal)
      expect(f.view().siteAccessRequest).toBeNull()
      expect(f.view().error).toBeNull()
    } finally { f.controller.dispose() }
  })

  it('keeps the draft and makes no access offer when the gateway denies the redirect', async () => {
    const error = new ApiError('Redirect access denied.', 403, 'browser_redirect_access_denied', {
      details: { host: 'portal.example.net', canRequestAccess: false },
    })
    const createSession = vi.fn().mockRejectedValue(error)
    const f = fixture({ createSession })
    const goal = 'Open example.com'
    await expect(f.controller.send(goal, 'https://example.com')).rejects.toThrow('Redirect access denied')
    expect(f.view()).toMatchObject({ goal, status: 'failed', errorCode: 'browser_redirect_access_denied', siteAccessRequest: null })
    expect(f.view().error).toContain('Your draft has been kept')
    expect(f.api.createSession).toHaveBeenCalledTimes(1)
    expect(f.api.explainSite).not.toHaveBeenCalled()
    expect(f.api.startRun).not.toHaveBeenCalled()
  })

  it('declines the inline request without writing a choice or starting a run', async () => {
    const f = fixture({ createSession: vi.fn().mockRejectedValue(redirectNeedsAccess('portal.example.net')) })
    await expect(f.controller.send('Inspect this page', 'https://example.com')).rejects.toThrow('Redirect access denied')
    f.controller.declineSiteAccess()
    expect(f.view().status).toBe('idle')
    expect(f.view().siteAccessRequest).toBeNull()
    expect(f.api.createSession).toHaveBeenCalledTimes(1)
    expect(f.api.explainSite).not.toHaveBeenCalled()
    expect(f.api.setSiteChoice).not.toHaveBeenCalled()
    expect(f.api.startRun).not.toHaveBeenCalled()
  })

  const permissionHost = 'portal.example.net'
  const permission = explanation(permissionHost, 'ask')
  it.each([
    explanation(permissionHost, 'deny'),
    { ...permission, decision: { ...permission.decision, cap: 'read' } },
    { ...permission, user_id: 'another-user' },
    explanation('another.example.net', 'ask'),
  ])('refuses the inline grant when the current policy cannot confirm exact writable access (%j)', async (result) => {
    const explainSite = vi.fn(async () => result)
    const f = fixture({ createSession: vi.fn().mockRejectedValue(redirectNeedsAccess(permissionHost)), explainSite })
    await expect(f.controller.send('Inspect this page', 'https://example.com')).rejects.toThrow('Redirect access denied')
    await expect(f.controller.allowSiteAccess()).rejects.toThrow('Could not confirm site access')
    expect(f.api.createSession).toHaveBeenCalledTimes(1)
    expect(f.api.setSiteChoice).not.toHaveBeenCalled()
    expect(f.api.startRun).not.toHaveBeenCalled()
  })

  it('closes a redirected browser session that arrives after the task was reset', async () => {
    type ResolvedSession = { session: { id: string }; resolvedStartUrl: string }
    let resolveSession!: (session: ResolvedSession) => void
    const waiting = new Promise<ResolvedSession>((resolve) => {
      resolveSession = resolve
    })
    const f = fixture({ createSession: vi.fn(() => waiting) })
    const send = f.controller.send('Inspect this page', 'https://example.com')
    await vi.waitFor(() => expect(f.api.createSession).toHaveBeenCalledTimes(1))
    f.controller.reset()
    resolveSession({ session: { id: 'late-session' }, resolvedStartUrl: 'https://portal.example.net/' })
    await send
    expect(f.api.createSession).toHaveBeenCalledTimes(1)
    expect(f.api.closeSession).toHaveBeenCalledWith('org-1', 'late-session')
    expect(f.api.startRun).not.toHaveBeenCalled()
    expect(f.view().status).toBe('idle')
  })

  it('refuses to create a browser without an attested capability', async () => {
    const f = fixture({ capability: vi.fn(async () => ({ available: false, policySupported: false, canAttach: false })) })
    await expect(f.controller.send('Find a page', 'https://example.com')).rejects.toThrow('unavailable')
    expect(f.api.createSession).not.toHaveBeenCalled()
    expect(f.view().status).toBe('failed')
  })

  it('starts a bounded attached run, displaying only its matching receipt', async () => {
    const f = fixture()
    await f.controller.send('Inspect this page', 'https://example.com/path')
    expect(f.api.startRun).toHaveBeenCalledWith('org-1', 'session-1', expect.objectContaining({ requireAttached: true, assistantInteractive: true, allowedDomains: ['example.com'] }), undefined, expect.anything())
    f.events().onBrowserAttachSharedJar?.({ runId: 'other', planId: 'plan-1', attachId: 'a', sessionRunId: 'session-1', tabId: 't' })
    expect(f.view().receipt).toBeNull()
    f.events().onBrowserAttachSharedJar?.({ runId: 'run-1', planId: 'plan-1', attachId: 'a', sessionRunId: 'session-1', tabId: 't' })
    expect(f.view().receipt?.runId).toBe('run-1')
    expect(f.view().status).toBe('running')
  })

  it('offers the requested page without starting the agent on a verified initial browser challenge', async () => {
    const createSession = vi.fn(async () => ({
      session: { id: 'session-1' },
      observation: { challenge: { strength: 'strong' } },
    }))
    const f = fixture({ createSession })
    const target = 'https://example.com/listings?location=oslo'
    await expect(f.controller.send('Open the listings', target)).rejects.toThrow('open the page yourself')
    expect(f.api.startRun).not.toHaveBeenCalled()
    expect(f.view()).toMatchObject({ status: 'failed', siteHandoffUrl: target })
    expect(f.api.closeSession).toHaveBeenCalledWith('org-1', 'session-1')
  })

  it('offers a manual link for a typed site challenge returned as an HTTP error', async () => {
    const createSession = vi.fn().mockRejectedValue(new ApiError('Site blocked', 403, 'browser_site_blocked'))
    const f = fixture({ createSession })
    const target = 'https://example.com/listings'
    await expect(f.controller.send('Open the listings', target)).rejects.toThrow('Site blocked')
    expect(f.api.startRun).not.toHaveBeenCalled()
    expect(f.view()).toMatchObject({ status: 'failed', siteHandoffUrl: target })
  })

  it('preserves the selected public result when the browser service times out before start', async () => {
    const createSession = vi.fn().mockRejectedValue(new ApiError('Upstream unavailable', 502, 'upstream_unavailable'))
    const f = fixture({ createSession })
    const target = 'https://example.com/search?location=oslo'
    await expect(f.controller.send('Open the listings', target)).rejects.toThrow('Upstream unavailable')
    expect(f.api.startRun).not.toHaveBeenCalled()
    expect(f.view()).toMatchObject({ status: 'failed', siteHandoffUrl: target, errorCode: 'browser_start_unavailable' })
    expect(f.view().error).toContain('browser service could not open')
  })

  it('offers the actual same-origin page when replay confirms an agent run hit a challenge', async () => {
    const url = 'https://example.com/listings?location=oslo'
    const f = fixture({ replay: vi.fn(async () => ({ runId: 'run-1', events: [
      { eventId: '1', eventType: 'EVENT_TYPE_BROWSER_OBSERVATION_RECEIVED', payload: { run_id: 'run-1', plan_id: 'plan-1', status: 'site_blocked', page_url: url } },
      { eventId: '2', eventType: 'EVENT_TYPE_RUN_FAILED', payload: { run_id: 'run-1', terminal: true, status: 'failed' } },
    ], truncated: false })) })
    try {
      await f.controller.send('Find the listings', 'https://example.com')
      await Promise.resolve()
      expect(f.view()).toMatchObject({ status: 'failed', siteHandoffUrl: url })
      expect(f.view().error).toContain('open the page yourself')
    } finally { f.controller.dispose() }
  })

  it.each([
    ['EVENT_TYPE_RUN_COMPLETED', 'completed', 'completed'],
    ['EVENT_TYPE_RUN_FAILED', 'failed', 'failed'],
    ['EVENT_TYPE_RUN_CANCELLED', 'cancelled', 'stopped'],
  ] as const)('returns the borrowed tab on %s without closing its session', async (eventType, status, viewStatus) => {
    let deliverTerminal!: (value: unknown) => void
    const replay = new Promise(resolve => { deliverTerminal = resolve })
    const f = fixture({ replay: vi.fn(() => replay) })
    try {
      const selected = { sessionId: 'personal', tabId: 'selected', expectedEpoch: 4, expectedOrigin: 'https://example.com' }
      await f.controller.send('Read this page', selected.expectedOrigin, selected)
      expect(f.api.createSession).not.toHaveBeenCalled()
      expect(f.api.startRun).toHaveBeenCalledWith('org-1', 'personal', expect.objectContaining({
        continueInTab: { tabId: 'selected', expectedEpoch: 4, expectedOrigin: 'https://example.com' },
      }), undefined, expect.anything())
      expect(f.api.startRun).not.toHaveBeenCalledWith('org-1', 'personal', expect.objectContaining({ allowedDomains: expect.anything() }), undefined, expect.anything())
      expect(f.view().usesExistingTab).toBe(true)
      const receipt = { runId: 'run-1', planId: 'plan-1', attachId: 'a', sessionRunId: 'personal', tabId: 'selected' }
      f.events().onBrowserAttachSharedJar?.({ ...receipt, tabId: 'other' })
      expect(f.view().receipt).toBeNull()
      f.events().onBrowserAttachSharedJar?.(receipt)
      expect(f.view().receipt).toBe(receipt)
      deliverTerminal({ runId: 'run-1', events: [{ eventId: '1', eventType, payload: { run_id: 'run-1', terminal: true, status } }], truncated: false })
      await replay
      expect(f.view().status).toBe(viewStatus)
      expect(f.controller.claimRetained('personal')).toBe(false)
      f.controller.reset()
      expect(f.api.closeSession).not.toHaveBeenCalled()
    } finally {
      f.controller.dispose()
    }
  })

  it('keeps the live receipt stable when replay repeats the attachment', async () => {
    let resolveReplay!: (value: unknown) => void
    const replay = new Promise(resolve => { resolveReplay = resolve })
    const f = fixture({ replay: vi.fn(() => replay) })
    try {
      await f.controller.send('Inspect this page', 'https://example.com')
      const receipt = { runId: 'run-1', planId: 'plan-1', attachId: 'attach-1', sessionRunId: 'session-1', tabId: 'tab-1' }
      f.events().onBrowserAttachSharedJar?.(receipt)
      expect(f.view().receipt).toBe(receipt)

      resolveReplay({ runId: 'run-1', events: [{
        eventId: '1',
        eventType: 'EVENT_TYPE_BROWSER_ATTACH_SHARED_JAR',
        payload: { run_id: 'run-1', plan_id: 'plan-1', attach_id: 'attach-1', session_run_id: 'session-1', tab_id: 'tab-1' },
      }], truncated: false })
      await replay
      expect(f.view().receipt).toBe(receipt)
      expect(f.view().status).toBe('running')
    } finally {
      f.controller.dispose()
    }
  })

  it('does not report a stop until a matching terminal run event arrives', async () => {
    let resolveReplay!: (value: unknown) => void
    const replay = new Promise(resolve => { resolveReplay = resolve })
    const f = fixture({ replay: vi.fn(() => replay) })
    await f.controller.send('Inspect', 'https://example.com')
    f.controller.stop()
    await Promise.resolve()
    expect(f.api.control).toHaveBeenCalledWith('org-1', 'run-1', 'stop')
    expect(f.view().status).toBe('stopping')
    f.events().onPlan?.({ runId: 'run-1', planId: 'plan-1', to: 'stopped' })
    expect(f.view().status).toBe('stopping')
    resolveReplay({ runId: 'run-1', events: [{ eventId: '1', eventType: 'EVENT_TYPE_RUN_CANCELLED', at: '2026-01-01T00:00:00Z', payload: { run_id: 'run-1', terminal: true, status: 'cancelled' } }], truncated: false })
    await Promise.resolve()
    expect(f.view().status).toBe('stopped')
  })

  it('does not claim completion if the event stream closes', async () => {
    const f = fixture()
    await f.controller.send('Inspect', 'https://example.com')
    f.events().onDone?.()
    expect(f.view().status).toBe('disconnected')
  })

  it('recovers an early receipt and terminal outcome from the run-scoped replay', async () => {
    const f = fixture({ replay: vi.fn(async () => ({ runId: 'run-1', events: [
      { eventId: '1', eventType: 'EVENT_TYPE_BROWSER_ATTACH_SHARED_JAR', at: '2026-01-01T00:00:00Z', payload: { run_id: 'run-1', plan_id: 'plan-1', attach_id: 'a', session_run_id: 'session-1', tab_id: 't' } },
      { eventId: '2', eventType: 'EVENT_TYPE_RUN_COMPLETED', at: '2026-01-01T00:00:01Z', payload: { run_id: 'run-1', terminal: true, status: 'completed' } },
    ], truncated: false })) })
    await f.controller.send('Inspect', 'https://example.com')
    await Promise.resolve()
    expect(f.view().receipt?.tabId).toBe('t')
    expect(f.view().status).toBe('completed')
    expect(f.api.closeSession).not.toHaveBeenCalled()
    f.controller.reset()
    expect(f.api.closeSession).toHaveBeenCalledWith('org-1', 'session-1')
  })

  it('keeps a claimed completed session past the snapshot deadline, then clears the claim on a new task', async () => {
    vi.useFakeTimers()
    try {
      const f = fixture({ replay: vi.fn(async () => ({ runId: 'run-1', events: [
        { eventId: '1', eventType: 'EVENT_TYPE_BROWSER_ATTACH_SHARED_JAR', payload: { run_id: 'run-1', plan_id: 'plan-1', attach_id: 'a', session_run_id: 'session-1', tab_id: 't' } },
        { eventId: '2', eventType: 'EVENT_TYPE_RUN_COMPLETED', payload: { run_id: 'run-1', terminal: true, status: 'completed' } },
      ], truncated: false })) })
      await f.controller.send('Inspect', 'https://example.com')
      await Promise.resolve()
      expect(f.controller.claimRetained('another-session')).toBe(false)
      expect(f.controller.claimRetained('session-1')).toBe(true)
      expect(f.view().browserClaimed).toBe(true)
      await vi.advanceTimersByTimeAsync(120001)
      expect(f.api.closeSession).not.toHaveBeenCalled()
      await f.controller.send('Inspect again', 'https://example.com')
      expect(f.view().browserClaimed).toBe(false)
      expect(f.api.closeSession).toHaveBeenCalledWith('org-1', 'session-1')
      f.controller.reset()
    } finally { vi.useRealTimers() }
  })

  it('ignores contradictory terminal event types and terminals for another run', async () => {
    const f = fixture({ replay: vi.fn(async () => ({ runId: 'run-1', events: [
      { eventId: '1', eventType: 'EVENT_TYPE_RUN_FAILED', at: '2026-01-01T00:00:00Z', payload: { run_id: 'run-1', terminal: true, status: 'completed' } },
      { eventId: '2', eventType: 'EVENT_TYPE_RUN_COMPLETED', at: '2026-01-01T00:00:01Z', payload: { run_id: 'other', terminal: true, status: 'completed' } },
    ], truncated: false })) })
    await f.controller.send('Inspect', 'https://example.com')
    await Promise.resolve()
    expect(f.view().status).toBe('starting')
    expect(f.api.closeSession).not.toHaveBeenCalled()
    f.controller.dispose()
  })

  it('shows only an authenticated final output for the same completed run', async () => {
    const f = fixture({
      replay: vi.fn(async () => ({ runId: 'run-1', events: [
        { eventId: '1', eventType: 'EVENT_TYPE_RUN_COMPLETED', at: '2026-01-01T00:00:00Z', payload: { run_id: 'run-1', terminal: true, status: 'completed' } },
      ], truncated: false })),
      getRun: vi.fn(async () => ({ runId: 'run-1', status: 'completed', finalOutput: 'The requested result' })),
    })
    await f.controller.send('Inspect', 'https://example.com')
    await Promise.resolve(); await Promise.resolve()
    expect(f.view().status).toBe('completed')
    expect(f.view().result).toBe('The requested result')
    expect(f.view().resultLoading).toBe(false)
    // Without an attachment receipt there is no verified browser to retain.
    expect(f.api.closeSession).toHaveBeenCalledWith('org-1', 'session-1')
  })

  it('delivers a completed run answer from the actual run-detail response', async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ run: {
      run_id: 'run-1', status: 'completed', final_output: 'The page title is Example Domain.',
    } }), { status: 200, headers: { 'Content-Type': 'application/json' } }))
    vi.stubGlobal('fetch', fetchMock)
    try {
      const f = fixture({
        getRun,
        replay: vi.fn(async () => ({ runId: 'run-1', events: [
          { eventId: '1', eventType: 'EVENT_TYPE_RUN_COMPLETED', at: '2026-01-01T00:00:00Z', payload: { run_id: 'run-1', terminal: true, status: 'completed' } },
        ], truncated: false })),
      })
      await f.controller.send('Read the page title', 'https://example.com')
      await vi.waitFor(() => expect(f.view().result).toBe('The page title is Example Domain.'))
      expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('/api/v1/agents/runs/run-1'), expect.objectContaining({ credentials: 'include' }))
      f.controller.dispose()
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('does not show a result from another run', async () => {
    const f = fixture({
      replay: vi.fn(async () => ({ runId: 'run-1', events: [
        { eventId: '1', eventType: 'EVENT_TYPE_RUN_COMPLETED', at: '2026-01-01T00:00:00Z', payload: { run_id: 'run-1', terminal: true, status: 'completed' } },
      ], truncated: false })),
      getRun: vi.fn(async () => ({ runId: 'other', status: 'completed', finalOutput: 'Other user result' })),
    })
    await f.controller.send('Inspect', 'https://example.com')
    await Promise.resolve(); await Promise.resolve()
    expect(f.view().result).toBeNull()
    expect(f.view().resultLoading).toBe(false)
  })

  it.each([['EVENT_TYPE_RUN_FAILED', 'failed'], ['EVENT_TYPE_RUN_CANCELLED', 'cancelled']] as const)('never fetches or displays final output for %s', async (eventType, status) => {
    const f = fixture({ replay: vi.fn(async () => ({ runId: 'run-1', events: [
      { eventId: '1', eventType, at: '2026-01-01T00:00:00Z', payload: { run_id: 'run-1', terminal: true, status } },
    ], truncated: false })) })
    await f.controller.send('Inspect', 'https://example.com')
    await Promise.resolve()
    expect(f.api.getRun).not.toHaveBeenCalled()
    expect(f.view().result).toBeNull()
  })

  it.each([
    ['browser_permission_denied', 'lacks permission'],
    ['browser_agent_permission_denied', 'lacks permission'],
    ['browser_timed_out', 'timed out'],
    ['browser_resource_exhausted', 'resource limit'],
  ])('shows a safe, actionable message for the matching terminal code %s', async (code, expected) => {
    const f = fixture({ replay: vi.fn(async () => ({ runId: 'run-1', events: [
      { eventId: '1', eventType: 'EVENT_TYPE_RUN_FAILED', at: '2026-01-01T00:00:00Z', payload: { run_id: 'run-1', terminal: true, status: 'failed', failure_code: code } },
    ], truncated: false })) })
    await f.controller.send('Inspect', 'https://example.com')
    await Promise.resolve()
    expect(f.view().status).toBe('failed')
    expect(f.view().error).toContain(expected)
    expect(f.api.getRun).not.toHaveBeenCalled()
  })

  it('never displays unknown terminal text or failure codes for a different run', async () => {
    const f = fixture({ replay: vi.fn(async () => ({ runId: 'run-1', events: [
      { eventId: '1', eventType: 'EVENT_TYPE_RUN_FAILED', at: '2026-01-01T00:00:00Z', payload: { run_id: 'other', terminal: true, status: 'failed', failure_code: 'browser_permission_denied' } },
      { eventId: '2', eventType: 'EVENT_TYPE_RUN_FAILED', at: '2026-01-01T00:00:01Z', payload: { run_id: 'run-1', terminal: true, status: 'failed', failure_code: 'secret internal text', error: 'sensitive upstream details' } },
    ], truncated: false })) })
    await f.controller.send('Inspect', 'https://example.com')
    await Promise.resolve()
    expect(f.view().status).toBe('failed')
    expect(f.view().error).toBeNull()
  })

  it('clears a connection warning when the verified run later completes', async () => {
    let resolveReplay!: (value: unknown) => void
    const replay = new Promise(resolve => { resolveReplay = resolve })
    const f = fixture({ replay: vi.fn(() => replay) })
    await f.controller.send('Inspect', 'https://example.com')
    f.events().onDone?.()
    expect(f.view().status).toBe('disconnected')
    resolveReplay({ runId: 'run-1', events: [
      { eventId: '1', eventType: 'EVENT_TYPE_RUN_COMPLETED', at: '2026-01-01T00:00:00Z', payload: { run_id: 'run-1', terminal: true, status: 'completed' } },
    ], truncated: false })
    await Promise.resolve()
    expect(f.view().status).toBe('completed')
    expect(f.view().error).toBeNull()
  })

  it('shows matching approval requests and waits for server decision confirmation', async () => {
    const f = fixture()
    await f.controller.send('Inspect', 'https://example.com')
    const request = { runId: 'run-1', planId: 'plan-1', approvalId: 'approval-1', actionId: 'action-1', actionType: 'click', selector: 'button.submit', riskCategory: 'posting_form', url: 'https://example.com/form?secret=hidden' }
    f.events().onBrowserActionApprovalRequired?.({ ...request, runId: 'other' })
    expect(f.view().approval).toBeNull()
    f.events().onBrowserActionApprovalRequired?.(request)
    expect(f.view().approval?.destination).toBe('https://example.com/form')
    await f.controller.decide('approve')
    expect(f.api.decideApproval).toHaveBeenCalledWith('approval-1', 'approve')
    expect(f.view().approval?.state).toBe('submitted')
    f.events().onBrowserActionDecided?.({ runId: 'run-1', planId: 'plan-1', approvalId: 'different', decision: 'granted' })
    expect(f.view().approval?.state).toBe('submitted')
    f.events().onBrowserActionDecided?.({ runId: 'run-1', planId: 'plan-1', approvalId: 'approval-1', decision: 'granted' })
    expect(f.view().approval).toBeNull()
    f.events().onBrowserActionApprovalRequired?.(request)
    expect(f.view().approval).toBeNull()
  })

  it('does not decide an event with no specific approval id', async () => {
    const f = fixture()
    await f.controller.send('Inspect', 'https://example.com')
    f.events().onBrowserActionApprovalRequired?.({ runId: 'run-1', planId: 'plan-1', actionId: 'action-1' })
    await f.controller.decide('approve')
    expect(f.api.decideApproval).not.toHaveBeenCalled()
    expect(f.view().approval?.id).toBeNull()
    f.events().onBrowserActionApprovalRequired?.({ runId: 'run-1', planId: 'plan-1', actionId: 'action-1', approvalId: 'approval-1', riskCategory: 'posting_form', url: 'https://example.com/send' })
    expect(f.view().approval?.id).toBe('approval-1')
    await f.controller.decide('reject')
    expect(f.api.decideApproval).toHaveBeenCalledWith('approval-1', 'reject')
  })

  it('refuses approval when a risky action has no reviewable target', async () => {
    const f = fixture()
    await f.controller.send('Inspect', 'https://example.com')
    f.events().onBrowserActionApprovalRequired?.({ runId: 'run-1', planId: 'plan-1', actionId: 'action-1', approvalId: 'approval-1', actionType: 'click', riskCategory: 'posting_form', url: 'https://example.com/form' })
    expect(f.view().approval?.canApprove).toBe(false)
    await f.controller.decide('approve')
    expect(f.api.decideApproval).not.toHaveBeenCalled()
    await f.controller.decide('reject')
    expect(f.api.decideApproval).toHaveBeenCalledWith('approval-1', 'reject')
  })

  it('does not retry or claim the run did not start after an ambiguous failure', async () => {
    const f = fixture({ startRun: vi.fn(async () => { throw new Error('network lost') }) })
    await expect(f.controller.send('Inspect', 'https://example.com')).rejects.toThrow('network lost')
    expect(f.api.startRun).toHaveBeenCalledOnce()
    expect(f.api.closeSession).not.toHaveBeenCalled()
    expect(f.view().status).toBe('disconnected')
    expect(f.view().error).toContain('may still be active')
  })

  it('recovers an uncertain start with the same session and idempotency key before stopping', async () => {
    const start = vi.fn()
      .mockRejectedValueOnce(new Error('network lost'))
      .mockResolvedValueOnce({ runId: 'run-1', planId: 'plan-1', threadId: 'thread-1' })
    const f = fixture({ startRun: start })
    await expect(f.controller.send('Inspect', 'https://example.com')).rejects.toThrow()
    const key = start.mock.calls[0]![4].idempotencyKey
    f.controller.stop()
    expect(f.view().status).toBe('disconnected')
    await f.controller.retry()
    expect(start.mock.calls[1]![1]).toBe('session-1')
    expect(start.mock.calls[1]![4].idempotencyKey).toBe(key)
    expect(f.api.createSession).toHaveBeenCalledOnce()
    expect(f.api.control).toHaveBeenCalledWith('org-1', 'run-1', 'stop')
    expect(f.view().status).toBe('stopping')
  })

  it('ignores events after the user or organization changes', async () => {
    const f = fixture()
    await f.controller.send('Inspect', 'https://example.com')
    f.setScope({ userId: 'other', orgId: 'org-1' })
    f.events().onBrowserAttachSharedJar?.({ runId: 'run-1', planId: 'plan-1', attachId: 'a', sessionRunId: 'session-1', tabId: 't' })
    expect(f.view().receipt).toBeNull()
  })
})
