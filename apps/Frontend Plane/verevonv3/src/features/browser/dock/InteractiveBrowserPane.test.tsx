// @vitest-environment jsdom
import { fireEvent, render } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import * as browserClient from '@/shared/api/browser-client'
import { ApiError } from '@/shared/api/http'
import type { BrowserNavigationFailure } from '@/features/browser/session/browser-navigation-error'
import type { BrowserSessionController } from '@/features/browser/session/use-browser-session-controller'
import { InteractiveBrowserPane, type InteractiveBrowserPaneProps } from './InteractiveBrowserPane'

// BrowserChrome is large and drives live frames; these tests check what the pane hands it, so it is
// replaced by a stub that keeps the props it was rendered with.
const chrome = vi.hoisted(() => ({ props: null as Record<string, unknown> | null }))
vi.mock('@/features/dashboard/home/BrowserChrome', () => ({
  BrowserChrome: (props: Record<string, unknown>) => {
    chrome.props = props
    const element = document.createElement('div')
    element.className = 'browser-chrome-stub'
    return element
  },
}))

const identity = vi.hoisted(() => ({
  user: { id: 'user-a' } as { id: string } | null,
  activeOrg: { id: 'org-a' } as { id: string } | null,
}))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))

vi.mock('@/shared/api/browser-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/browser-client')>()),
  keepAliveBrowserSession: vi.fn(),
}))

// The omnibox asks for suggestions and results over the network. These tests make no network calls.
vi.mock('@/shared/api/search-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/search-client')>()),
  loadSearchSuggestions: vi.fn(async () => ({ suggestions: [] })),
  runWebSearch: vi.fn(async () => ({ results: [] })),
}))

type FakeState = {
  view: Record<string, unknown> | null
  navigationError: BrowserNavigationFailure | null
  error: string | null
  busy: boolean
}

// A stand-in for the page's session controller, built on Solid signals so the pane re-renders when the
// test changes what the controller reports.
function fakeController(initial: Partial<FakeState> = {}) {
  const [view, setView] = createSignal<Record<string, unknown> | null>(initial.view ?? null)
  const [navigationError, setNavigationError] = createSignal<BrowserNavigationFailure | null>(
    initial.navigationError ?? null,
  )
  const [error, setError] = createSignal<string | null>(initial.error ?? null)
  const [busy, setBusy] = createSignal(initial.busy ?? false)
  const open = vi.fn(async (_url: string) => undefined)
  const runAction = vi.fn(async () => undefined)
  const controller = {
    view: () => view(),
    preview: () => null,
    busy: () => busy(),
    error: () => error(),
    navigationError: () => navigationError(),
    approvals: () => [],
    decidingApprovalKeys: () => [],
    aiRunRationale: () => null,
    loop: () => ({ error: null, goal: '', status: 'idle', step: 0 }),
    open,
    runAction,
    setControlMode: vi.fn(async () => undefined),
    newTab: vi.fn(async () => undefined),
    selectTab: vi.fn(async () => undefined),
    applySocketObservation: vi.fn(),
    decideApproval: vi.fn(async () => undefined),
  }
  return {
    controller: controller as unknown as BrowserSessionController,
    open,
    runAction,
    setView,
    setNavigationError,
    setError,
    setBusy,
  }
}

function sessionView(sessionId: string, host = 'example.test'): Record<string, unknown> {
  return {
    sessionId,
    host,
    url: `https://${host}/`,
    title: 'Example',
    status: 'live',
    renderMode: 'chromium',
    controlMode: 'agent_control',
    capabilities: [],
    zdr: false,
    tabs: [],
  }
}

function failure(url: string, reason: BrowserNavigationFailure['reason'], sentence: string): BrowserNavigationFailure {
  return { url, reason, sentence }
}

function required<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector)
  if (!element) throw new Error(`missing ${selector}`)
  return element
}

function renderPane(overrides: Partial<InteractiveBrowserPaneProps> & { controller: BrowserSessionController }) {
  const onClose = vi.fn()
  const onAskAboutPage = vi.fn()
  const onDockPositionChange = vi.fn()
  render(() => (
    <InteractiveBrowserPane
      controller={overrides.controller}
      orgId={overrides.orgId ?? (() => 'org-a')}
      zdr={overrides.zdr ?? (() => false)}
      dockPosition={overrides.dockPosition ?? 'right'}
      onDockPositionChange={overrides.onDockPositionChange ?? onDockPositionChange}
      hidden={overrides.hidden}
      searchEnabled={overrides.searchEnabled}
      onAskVerevon={overrides.onAskVerevon}
      onAskAboutPage={overrides.onAskAboutPage ?? onAskAboutPage}
      onClose={overrides.onClose ?? onClose}
    />
  ))
  return { onClose, onAskAboutPage, onDockPositionChange }
}

beforeEach(() => {
  vi.resetAllMocks()
  identity.user = { id: 'user-a' }
  identity.activeOrg = { id: 'org-a' }
  chrome.props = null
})

afterEach(() => {
  document.body.innerHTML = ''
  localStorage.clear()
})

describe('InteractiveBrowserPane start state (WS9.H)', () => {
  it('shows the start copy and the omnibox, and no browser chrome, while no session is open', () => {
    const fake = fakeController()
    renderPane({ controller: fake.controller })

    expect(document.querySelector('.browser-dock-pane__start')).not.toBeNull()
    expect(document.querySelector('.browser-omnibox__input')).not.toBeNull()
    expect(document.querySelector('.browser-chrome-stub')).toBeNull()
  })

  it('opens the typed address in a session when Enter is pressed', () => {
    const fake = fakeController()
    renderPane({ controller: fake.controller })
    const input = required<HTMLInputElement>('.browser-omnibox__input')

    fireEvent.input(input, { target: { value: 'https://example.test/' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    flush()

    expect(fake.open).toHaveBeenCalledTimes(1)
    expect(fake.open.mock.calls[0]?.[0]).toContain('example.test')
  })

  it('does nothing for free text while search is off, so no text leaves for a third-party engine', () => {
    const fake = fakeController()
    renderPane({ controller: fake.controller, searchEnabled: false })
    const input = required<HTMLInputElement>('.browser-omnibox__input')

    fireEvent.input(input, { target: { value: 'norwegian salmon exports' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    flush()

    expect(fake.open).not.toHaveBeenCalled()
  })

  it('shows the opening status and the controller error while the start state is up', () => {
    const fake = fakeController({ busy: true, error: 'Nettleserøkt kunne ikke startes.' })
    renderPane({ controller: fake.controller })

    expect(document.querySelector('.browser-dock-pane__status')).not.toBeNull()
    expect(required<HTMLElement>('.browser-dock-pane__error').textContent).toContain('kunne ikke startes')
  })
})

describe('InteractiveBrowserPane session state (WS9.H)', () => {
  it('mounts the browser chrome compactly beside the open session and hands it the controller state', () => {
    const fake = fakeController({ view: sessionView('sess-1') })
    renderPane({ controller: fake.controller })

    expect(document.querySelector('.browser-chrome-stub')).not.toBeNull()
    expect(document.querySelector('.browser-dock-pane__start')).toBeNull()
    expect(chrome.props?.compact).toBe(true)
    expect(chrome.props?.session).toMatchObject({ sessionId: 'sess-1' })
    expect(chrome.props?.browserBusy).toBe(false)
  })

  it('names the host the session is limited to, until a cross-host refusal is already shown', () => {
    const fake = fakeController({ view: sessionView('sess-1', 'example.test') })
    renderPane({ controller: fake.controller })

    expect(required<HTMLElement>('.browser-dock-pane__limit').textContent).toContain('example.test')

    fake.setNavigationError(failure('https://other.test/', 'outside_scope', 'This session is limited to example.test.'))
    flush()

    expect(document.querySelector('.browser-dock-pane__limit')).toBeNull()
  })

  it('hides a navigation failure the person dismissed, and shows the next one', () => {
    const first = failure('https://a.test/', 'unreachable', 'The page could not be opened.')
    const fake = fakeController({ view: sessionView('sess-1'), navigationError: first })
    renderPane({ controller: fake.controller })
    expect(chrome.props?.browserNavigationError).toBe(first)

    ;(chrome.props?.onBrowserNavigationDismiss as () => void)()
    flush()
    expect(chrome.props?.browserNavigationError).toBeNull()

    const second = failure('https://b.test/', 'unreachable', 'The page could not be opened.')
    fake.setNavigationError(second)
    flush()
    expect(chrome.props?.browserNavigationError).toBe(second)
  })

  it('reports a keep-alive as alive, and as expired once the grant can no longer be renewed', async () => {
    vi.mocked(browserClient.keepAliveBrowserSession).mockResolvedValue({ expiresAt: null })
    const fake = fakeController({ view: sessionView('sess-1') })
    renderPane({ controller: fake.controller })
    const keepAlive = chrome.props?.onKeepAlive as () => Promise<string>

    await expect(keepAlive()).resolves.toBe('alive')
    expect(browserClient.keepAliveBrowserSession).toHaveBeenCalledWith('org-a', 'sess-1')

    vi.mocked(browserClient.keepAliveBrowserSession).mockRejectedValue(
      new ApiError('gone', 410, 'browser_session_expired'),
    )
    await expect(keepAlive()).resolves.toBe('expired')
  })

  it('moves focus to the composer from the header, and closes the body from its close button', () => {
    const fake = fakeController({ view: sessionView('sess-1') })
    const { onAskAboutPage, onClose } = renderPane({ controller: fake.controller })

    fireEvent.click(required<HTMLElement>('.browser-dock-pane__ask'))
    fireEvent.click(required<HTMLElement>('.verevon-browser-dock-pane__close'))

    expect(onAskAboutPage).toHaveBeenCalledTimes(1)
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('stays mounted but is not shown while a canvas tab owns the rail', () => {
    const fake = fakeController()
    renderPane({ controller: fake.controller, hidden: true })

    expect(required<HTMLElement>('.verevon-browser-dock-pane').style.display).toBe('none')
  })
})
