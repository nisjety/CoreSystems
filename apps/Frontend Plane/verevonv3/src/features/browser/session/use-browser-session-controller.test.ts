// @vitest-environment jsdom
import { flush as solidFlush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// The gateway clients are mocked at the module boundary. The controller's own
// rules (ZDR, busy gating, stale replies, the navigation card, approvals) are
// what these tests exercise; the transport is not.
vi.mock('@/shared/api/browser-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/browser-client')>()),
  closeBrowserSession: vi.fn(),
  closeBrowserTab: vi.fn(),
  createBrowserSession: vi.fn(),
  createBrowserTab: vi.fn(),
  getBrowserOwnerTimeline: vi.fn(),
  runBrowserAction: vi.fn(),
  selectBrowserTab: vi.fn(),
  setBrowserControlMode: vi.fn(),
}))

vi.mock('@/shared/api/browser-run-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/browser-run-client')>()),
  controlBrowserAiRun: vi.fn(),
  startBrowserAiRun: vi.fn(),
}))

vi.mock('@/shared/api/run-console-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/run-console-client')>()),
  streamRunEvents: vi.fn(),
}))

vi.mock('@/shared/api/orchestration-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/orchestration-client')>()),
  decideApproval: vi.fn(),
  listApprovals: vi.fn(),
}))

import * as browserClient from '@/shared/api/browser-client'
import * as runClient from '@/shared/api/browser-run-client'
import * as consoleClient from '@/shared/api/run-console-client'
import * as orchestration from '@/shared/api/orchestration-client'
import { ApiError } from '@/shared/api/http'
import { createBrowserSessionController, type BrowserSessionController } from './use-browser-session-controller'

const ORG = 'org-1'

type ControllerOptions = Parameters<typeof createBrowserSessionController>[0]

function sessionResponse(id: string, options: { zdr?: boolean; url?: string } = {}) {
  return {
    observation: null,
    session: {
      capabilities: ['navigate'],
      control: { mode: 'agent_control' },
      id,
      profile: { id: null, scope: 'run_scoped', storage: 'isolated' },
      renderMode: 'chromium',
      status: 'live',
      tabs: [],
      title: 'Example',
      url: options.url ?? 'https://example.test/',
      viewport: { height: 800, width: 1280 },
      zdr: options.zdr ?? false,
    },
  } as unknown as Awaited<ReturnType<typeof browserClient.createBrowserSession>>
}

function timeline(items: unknown[] = []) {
  return { items, nextCursor: null } as never
}

function nextTick() {
  return new Promise<void>((resolve) => setTimeout(resolve, 0))
}

const controllers: BrowserSessionController[] = []

function makeController(overrides: Partial<ControllerOptions> = {}): BrowserSessionController {
  const controller = createBrowserSessionController({ orgId: () => ORG, zdr: () => false, ...overrides })
  controllers.push(controller)
  return controller
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.mocked(browserClient.getBrowserOwnerTimeline).mockResolvedValue(timeline())
  vi.mocked(browserClient.closeBrowserSession).mockResolvedValue({ closed: true })
  vi.mocked(runClient.controlBrowserAiRun).mockResolvedValue({} as never)
})

afterEach(async () => {
  // Detaches the page-hide listener of every controller a test made.
  while (controllers.length > 0) await controllers.pop()?.dispose()
  vi.restoreAllMocks()
})

describe('createBrowserSessionController (WS9.F)', () => {
  it('opens a session and closes it again', async () => {
    const controller = makeController()
    vi.mocked(browserClient.createBrowserSession).mockResolvedValue(sessionResponse('sess-1'))

    await controller.open('https://example.test/')

    expect(browserClient.createBrowserSession).toHaveBeenCalledWith(
      ORG,
      expect.objectContaining({ url: 'https://example.test/' }),
      expect.anything(),
    )
    expect(controller.preview()?.browserSession?.session.id).toBe('sess-1')
    expect(controller.view()).not.toBeNull()
    expect(controller.busy()).toBe(false)

    await controller.close()

    expect(browserClient.closeBrowserSession).toHaveBeenCalledWith(ORG, 'sess-1')
    expect(controller.preview()).toBeNull()
    expect(controller.view()).toBeNull()
  })

  it('never sends a profile on a ZDR session, even when the picker has one', async () => {
    const controller = makeController({
      zdr: () => true,
      profileChoice: () => ({ profile_id: 'prof-1', name: 'Work', scope: 'user_private' }) as never,
    })
    vi.mocked(browserClient.createBrowserSession).mockResolvedValue(sessionResponse('sess-z', { zdr: true }))

    await controller.open('https://example.test/')

    const body = vi.mocked(browserClient.createBrowserSession).mock.calls[0]?.[1] as Record<string, unknown>
    expect(body.zdr).toBe(true)
    expect(body).not.toHaveProperty('profileId')
    expect(body).not.toHaveProperty('scope')
    expect(body).not.toHaveProperty('persistentProfile')

    await controller.dispose()

    expect(browserClient.closeBrowserSession).toHaveBeenCalledWith(ORG, 'sess-z')
  })

  it('sends the chosen profile on a non-ZDR session and leaves it open on dispose', async () => {
    const controller = makeController({
      profileChoice: () => ({ profile_id: 'prof-1', name: 'Work', scope: 'user_private' }) as never,
    })
    vi.mocked(browserClient.createBrowserSession).mockResolvedValue(sessionResponse('sess-p'))

    await controller.open('https://example.test/')

    const body = vi.mocked(browserClient.createBrowserSession).mock.calls[0]?.[1] as Record<string, unknown>
    expect(body).not.toHaveProperty('zdr')
    expect(body.profileId).toBe('prof-1')
    expect(body.scope).toBe('user_private')

    await controller.dispose()

    expect(browserClient.closeBrowserSession).not.toHaveBeenCalled()
  })

  it('gates a second action while one is running', async () => {
    const controller = makeController()
    vi.mocked(browserClient.createBrowserSession).mockResolvedValue(sessionResponse('sess-1'))
    await controller.open('https://example.test/')

    let release: (value: unknown) => void = () => undefined
    vi.mocked(browserClient.runBrowserAction).mockImplementation(
      () => new Promise((resolve) => { release = resolve }) as never,
    )

    const first = controller.runAction({ type: 'back' })
    const second = controller.runAction({ type: 'forward' })
    // Solid 2 applies signal writes on flush, so read after flushing.
    solidFlush()
    expect(controller.busy()).toBe(true)
    await second

    expect(browserClient.runBrowserAction).toHaveBeenCalledTimes(1)

    release(sessionResponse('sess-1'))
    await first
    solidFlush()

    expect(controller.busy()).toBe(false)
  })

  it('closes a slow open that was superseded instead of attaching it', async () => {
    const controller = makeController()
    let resolveOld: (value: unknown) => void = () => undefined
    vi.mocked(browserClient.createBrowserSession)
      .mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve }) as never)
      .mockResolvedValueOnce(sessionResponse('sess-new'))

    const old = controller.open('https://old.example/')
    await controller.open('https://new.example/')

    expect(controller.preview()?.browserSession?.session.id).toBe('sess-new')

    resolveOld(sessionResponse('sess-old'))
    await old

    expect(controller.preview()?.browserSession?.session.id).toBe('sess-new')
    expect(browserClient.closeBrowserSession).toHaveBeenCalledWith(ORG, 'sess-old')
  })

  it('drops an owner-timeline reply that arrives after the session was replaced', async () => {
    const controller = makeController()
    let resolveOld: (value: unknown) => void = () => undefined
    vi.mocked(browserClient.getBrowserOwnerTimeline)
      .mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve }) as never)
      .mockResolvedValueOnce(timeline([{ id: 'new-entry' }]))
    vi.mocked(browserClient.createBrowserSession)
      .mockResolvedValueOnce(sessionResponse('sess-a'))
      .mockResolvedValueOnce(sessionResponse('sess-b'))

    await controller.open('https://a.example/')
    await controller.open('https://b.example/')
    await nextTick()

    resolveOld(timeline([{ id: 'old-entry' }]))
    await nextTick()

    expect(controller.preview()?.browserSession?.session.id).toBe('sess-b')
    const ids = (controller.preview()?.browserSession?.session.ownerTimeline ?? []).map((item) => (item as { id?: string }).id)
    expect(ids).toEqual(['new-entry'])
  })

  it('sets the navigation card on a failed navigation and clears it on success', async () => {
    const controller = makeController()
    vi.mocked(browserClient.createBrowserSession).mockResolvedValue(sessionResponse('sess-1'))
    await controller.open('https://example.test/')

    vi.mocked(browserClient.runBrowserAction).mockRejectedValueOnce(new ApiError('bad', 400, 'invalid_url'))
    await controller.runAction({ type: 'navigate', url: 'not a url' })

    expect(controller.navigationError()).toMatchObject({ url: 'not a url', reason: 'invalid_url' })
    expect(controller.error()).toBeNull()

    vi.mocked(browserClient.runBrowserAction).mockResolvedValueOnce(sessionResponse('sess-1', { url: 'https://ok.example/' }))
    await controller.runAction({ type: 'navigate', url: 'https://ok.example/' })

    expect(controller.navigationError()).toBeNull()
  })

  it('keeps a non-navigation failure out of the navigation card', async () => {
    const controller = makeController()
    vi.mocked(browserClient.createBrowserSession).mockResolvedValue(sessionResponse('sess-1'))
    await controller.open('https://example.test/')

    vi.mocked(browserClient.runBrowserAction).mockRejectedValueOnce(new ApiError('boom', 500, 'driver_failed'))
    await controller.runAction({ type: 'back' })

    expect(controller.navigationError()).toBeNull()
    expect(controller.error()).toEqual(expect.any(String))
  })

  it('reduces approval events through withBrowserApprovalRequested and decides through the approval route', async () => {
    const controller = makeController()
    vi.mocked(browserClient.createBrowserSession).mockResolvedValue(sessionResponse('sess-1'))
    await controller.open('https://example.test/')

    vi.mocked(runClient.startBrowserAiRun).mockResolvedValue({ runId: 'run-1', threadId: 'thread-1', planId: 'plan-1' })
    vi.mocked(consoleClient.streamRunEvents).mockImplementation(async (_runId, handlers) => {
      handlers.onBrowserActionApprovalRequired?.({
        runId: 'run-1',
        planId: 'plan-1',
        actionId: 'act-1',
        actionType: 'click',
        url: 'https://example.test/',
        riskCategory: 'checkout',
        reason: 'Pays for the order',
        approvalId: '',
      })
    })
    vi.mocked(orchestration.listApprovals).mockResolvedValue([{ id: 'appr-1', status: 'PENDING' }] as never)
    vi.mocked(orchestration.decideApproval).mockResolvedValue(undefined as never)

    await controller.startAiRun('buy the shoes')

    expect(controller.approvals()).toHaveLength(1)
    expect(controller.approvals()[0]).toMatchObject({ key: 'act-1', runId: 'run-1', status: 'pending' })

    await controller.decideApproval('act-1', 'approve')

    expect(orchestration.listApprovals).toHaveBeenCalledWith('run-1')
    expect(orchestration.decideApproval).toHaveBeenCalledWith('appr-1', 'approve')
    expect(controller.approvals()[0]?.status).toBe('granted')
    expect(controller.decidingApprovalKeys()).toEqual([])
  })

  it('closes the session on page hide with a keepalive DELETE', async () => {
    const controller = makeController()
    vi.mocked(browserClient.createBrowserSession).mockResolvedValue(sessionResponse('sess-hide'))
    await controller.open('https://example.test/')

    const fetchSpy = vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(null, { status: 200 }))
    window.dispatchEvent(new Event('pagehide'))

    expect(fetchSpy).toHaveBeenCalledWith(
      expect.stringContaining('/api/v1/browser/sessions/sess-hide'),
      expect.objectContaining({ method: 'DELETE', keepalive: true }),
    )
  })

  it('reads busy in the same tick as the action that set it, without a flush', async () => {
    const controller = makeController()
    vi.mocked(browserClient.createBrowserSession).mockResolvedValue(sessionResponse('sess-1'))
    await controller.open('https://example.test/')

    let release: (value: unknown) => void = () => undefined
    vi.mocked(browserClient.runBrowserAction).mockImplementation(
      () => new Promise((resolve) => { release = resolve }) as never,
    )

    const pending = controller.runAction({ type: 'back' })
    // The signals read through latest(), so the write is visible without a flush (D2-175).
    expect(controller.busy()).toBe(true)

    release(sessionResponse('sess-1'))
    await pending

    expect(controller.busy()).toBe(false)
  })

  it('reads the cleared preview in the same tick as close()', async () => {
    const controller = makeController()
    vi.mocked(browserClient.createBrowserSession).mockResolvedValue(sessionResponse('sess-1'))
    await controller.open('https://example.test/')
    expect(controller.preview()?.browserSession?.session.id).toBe('sess-1')

    const closing = controller.close()

    expect(controller.preview()).toBeNull()
    expect(controller.view()).toBeNull()
    await closing
    expect(browserClient.closeBrowserSession).toHaveBeenCalledWith(ORG, 'sess-1')
  })

  it('drops an action reply that arrives after the session was replaced', async () => {
    const controller = makeController()
    vi.mocked(browserClient.createBrowserSession)
      .mockResolvedValueOnce(sessionResponse('sess-a'))
      .mockResolvedValueOnce(sessionResponse('sess-b'))
    await controller.open('https://a.example/')

    let resolveAction: (value: unknown) => void = () => undefined
    vi.mocked(browserClient.runBrowserAction).mockImplementation(
      () => new Promise((resolve) => { resolveAction = resolve }) as never,
    )
    const action = controller.runAction({ type: 'navigate', url: 'https://slow.example/' })

    await controller.open('https://b.example/')
    expect(controller.preview()?.browserSession?.session.id).toBe('sess-b')

    resolveAction(sessionResponse('sess-a', { url: 'https://slow.example/' }))
    await action

    expect(controller.preview()?.browserSession?.session.id).toBe('sess-b')
    expect(controller.navigationError()).toBeNull()
    expect(controller.error()).toBeNull()
  })

  it('drops a failed action that arrives after the session was closed', async () => {
    const controller = makeController()
    vi.mocked(browserClient.createBrowserSession).mockResolvedValue(sessionResponse('sess-1'))
    await controller.open('https://example.test/')

    let rejectAction: (reason: unknown) => void = () => undefined
    vi.mocked(browserClient.runBrowserAction).mockImplementation(
      () => new Promise((_resolve, reject) => { rejectAction = reject }) as never,
    )
    const action = controller.runAction({ type: 'navigate', url: 'not a url' })

    await controller.close()
    rejectAction(new ApiError('bad', 400, 'invalid_url'))
    await action

    expect(controller.navigationError()).toBeNull()
    expect(controller.error()).toBeNull()
  })

  it('does not show a tab error from a session that was replaced', async () => {
    const controller = makeController()
    vi.mocked(browserClient.createBrowserSession)
      .mockResolvedValueOnce(sessionResponse('sess-a'))
      .mockResolvedValueOnce(sessionResponse('sess-b'))
    await controller.open('https://a.example/')

    let rejectTab: (reason: unknown) => void = () => undefined
    vi.mocked(browserClient.createBrowserTab).mockImplementation(
      () => new Promise((_resolve, reject) => { rejectTab = reject }) as never,
    )
    const tab = controller.newTab()

    await controller.open('https://b.example/')
    rejectTab(new ApiError('boom', 500, 'driver_failed'))
    await tab

    expect(controller.preview()?.browserSession?.session.id).toBe('sess-b')
    expect(controller.error()).toBeNull()
  })

  it('keeps busy for a new session action while an action of a replaced session settles', async () => {
    const controller = makeController()
    vi.mocked(browserClient.createBrowserSession)
      .mockResolvedValueOnce(sessionResponse('sess-a'))
      .mockResolvedValueOnce(sessionResponse('sess-b'))
    await controller.open('https://a.example/')

    const resolvers: Array<(value: unknown) => void> = []
    vi.mocked(browserClient.runBrowserAction).mockImplementation(
      () => new Promise((resolve) => { resolvers.push(resolve) }) as never,
    )
    const staleAction = controller.runAction({ type: 'back' })
    await controller.open('https://b.example/')
    const freshAction = controller.runAction({ type: 'forward' })
    expect(controller.busy()).toBe(true)

    resolvers[0]?.(sessionResponse('sess-a'))
    await staleAction
    expect(controller.busy()).toBe(true)

    resolvers[1]?.(sessionResponse('sess-b'))
    await freshAction
    expect(controller.busy()).toBe(false)
  })

  it('closes a session whose open settles after close(), and never attaches it', async () => {
    const controller = makeController()
    let resolveOpen: (value: unknown) => void = () => undefined
    vi.mocked(browserClient.createBrowserSession).mockImplementationOnce(
      () => new Promise((resolve) => { resolveOpen = resolve }) as never,
    )

    const opening = controller.open('https://late.example/')
    await controller.close()
    resolveOpen(sessionResponse('sess-late'))
    await opening

    expect(controller.preview()).toBeNull()
    expect(controller.busy()).toBe(false)
    expect(browserClient.closeBrowserSession).toHaveBeenCalledWith(ORG, 'sess-late')
  })

  it('closes a session whose open settles after dispose(), and never attaches it', async () => {
    const controller = makeController()
    let resolveOpen: (value: unknown) => void = () => undefined
    vi.mocked(browserClient.createBrowserSession).mockImplementationOnce(
      () => new Promise((resolve) => { resolveOpen = resolve }) as never,
    )

    const opening = controller.open('https://late.example/')
    await controller.dispose()
    resolveOpen(sessionResponse('sess-late'))
    await opening

    expect(controller.preview()).toBeNull()
    expect(browserClient.closeBrowserSession).toHaveBeenCalledWith(ORG, 'sess-late')
  })
})
