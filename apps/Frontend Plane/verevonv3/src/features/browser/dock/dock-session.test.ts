// @vitest-environment jsdom
import { createRoot, createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// The gateway client is mocked at the module boundary. What these tests exercise is the page's own rules:
// one controller kept for the page, zero retention from the thread and the workspace verdict, and the close
// when the chat page unmounts. The transport is not exercised.
vi.mock('@/shared/api/browser-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/browser-client')>()),
  closeBrowserSession: vi.fn(),
  createBrowserSession: vi.fn(),
  getBrowserOwnerTimeline: vi.fn(),
}))

import * as browserClient from '@/shared/api/browser-client'
import { __resetRetentionForTests, applyServerRetention } from '@/features/chat/lib/chat-retention'
import {
  useDockBrowserSession,
  type DockBrowserSession,
  type DockBrowserSessionOptions,
} from './dock-session'

const ORG = 'org-1'

function sessionResponse(id: string) {
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
      url: 'https://example.test/',
      viewport: { height: 800, width: 1280 },
      zdr: false,
    },
  } as unknown as Awaited<ReturnType<typeof browserClient.createBrowserSession>>
}

function nextTick(): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(resolve, 0))
}

// Each mount runs the hook under its own root, so the page's cleanup can be exercised on its own.
const disposers: Array<() => void> = []

function mount(options: Partial<DockBrowserSessionOptions> = {}): DockBrowserSession {
  let session: DockBrowserSession | undefined
  const dispose = createRoot((disposeRoot) => {
    session = useDockBrowserSession({
      orgId: () => ORG,
      threadTemporary: () => false,
      enabled: true,
      ...options,
    })
    return disposeRoot
  })
  disposers.push(dispose)
  if (!session) throw new Error('the dock session was not created')
  return session
}

function requireController(session: DockBrowserSession) {
  const controller = session.controller
  if (!controller) throw new Error('the dock controller is missing')
  return controller
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.mocked(browserClient.getBrowserOwnerTimeline).mockResolvedValue({ items: [], nextCursor: null } as never)
  vi.mocked(browserClient.closeBrowserSession).mockResolvedValue({ closed: true } as never)
  __resetRetentionForTests()
})

afterEach(async () => {
  while (disposers.length > 0) disposers.pop()?.()
  await nextTick()
  __resetRetentionForTests()
  vi.restoreAllMocks()
})

describe('useDockBrowserSession (WS9.H)', () => {
  it('creates no controller and opens nothing while the dock is off in this build', () => {
    const session = mount({ enabled: false })

    expect(session.controller).toBeNull()
    expect(session.zdr()).toBe(false)
    expect(browserClient.createBrowserSession).not.toHaveBeenCalled()
  })

  it('keeps one controller for the chat page, however often it is read', () => {
    const session = mount()
    const first = session.controller

    expect(first).not.toBeNull()
    expect(session.controller).toBe(first)
  })

  it('opens a ZDR session without a profile while the thread on screen is temporary', async () => {
    vi.mocked(browserClient.createBrowserSession).mockResolvedValue(sessionResponse('sess-1'))
    const controller = requireController(mount({ threadTemporary: () => true }))

    await controller.open('https://example.test/')

    const body = vi.mocked(browserClient.createBrowserSession).mock.calls[0]?.[1]
    expect(body).toMatchObject({ zdr: true, url: 'https://example.test/' })
    expect(body).not.toHaveProperty('profileId')
    expect(body).not.toHaveProperty('scope')
  })

  it('opens a ZDR session when the workspace retention verdict denies local copies', async () => {
    vi.mocked(browserClient.createBrowserSession).mockResolvedValue(sessionResponse('sess-2'))
    applyServerRetention(true)
    const session = mount()

    expect(session.zdr()).toBe(true)
    await requireController(session).open('https://example.test/')

    expect(vi.mocked(browserClient.createBrowserSession).mock.calls[0]?.[1]).toMatchObject({ zdr: true })
  })

  it('follows the retention verdict back when the workspace allows local copies again', () => {
    applyServerRetention(true)
    const session = mount()
    expect(session.zdr()).toBe(true)

    applyServerRetention(false)
    flush()

    expect(session.zdr()).toBe(false)
  })

  it('opens a plain session when neither the thread nor the workspace is temporary', async () => {
    vi.mocked(browserClient.createBrowserSession).mockResolvedValue(sessionResponse('sess-3'))
    const controller = requireController(mount())

    await controller.open('https://example.test/')

    expect(vi.mocked(browserClient.createBrowserSession).mock.calls[0]?.[1]).not.toHaveProperty('zdr')
  })

  it('reads the zero-retention state of the thread on screen, not a copy taken at mount', () => {
    const [temporary, setTemporary] = createSignal(false)
    const session = mount({ threadTemporary: () => temporary() })
    expect(session.zdr()).toBe(false)

    setTemporary(true)
    flush()

    expect(session.zdr()).toBe(true)
  })

  it('closes the open session when the chat page is disposed', async () => {
    vi.mocked(browserClient.createBrowserSession).mockResolvedValue(sessionResponse('sess-9'))
    const session = mount({ threadTemporary: () => true })
    await requireController(session).open('https://example.test/')

    // The page's cleanup closes the session on the next task, so the teardown does not write signals.
    disposers.pop()?.()
    await nextTick()

    expect(browserClient.closeBrowserSession).toHaveBeenCalledWith(ORG, 'sess-9')
  })
})
