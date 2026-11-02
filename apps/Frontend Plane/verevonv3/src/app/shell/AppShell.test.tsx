// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, render, screen } from '@solidjs/testing-library'
import type { JSX } from '@solidjs/web'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { CHUNK_RELOAD_KEY, dismissStaleBuildNotice, installChunkRecovery } from '@/app/chunk-recovery'

// The core shell is not what is under test: only where the boundary sits relative to it matters.
vi.mock('@/features/core/components/CoreShell', () => ({
  CoreShell: (props: { children?: JSX.Element }) => <>{props.children}</>,
}))

vi.mock('@/shared/session/session-store', () => ({
  loadSession: vi.fn(async () => undefined),
  clearSession: vi.fn(),
}))

import { AppShell } from './AppShell'

let uninstall: () => void = () => undefined
let reload: ReturnType<typeof vi.fn<() => void>>

/** Dispatch the event Vite fires for a failed lazy import; true when a listener prevented it, which is a reload. */
const preloadFails = () => {
  const event = new Event('vite:preloadError', { cancelable: true })
  window.dispatchEvent(event)
  flush()
  return event.defaultPrevented
}

/** Render the app at `path`, with `Page` as the page the router matches. */
function renderApp(path: string, Page: () => never) {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: Page }],
    history: memoryHistory(path),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <AppShell {...props} />}</TestRouter>)
}

/** Give the router a turn to settle the route, then flush Solid's queue. */
async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

const staleChunkError = () => new TypeError('Failed to fetch dynamically imported module: /assets/AuthPage-1.js')

/** A reload already happened a moment ago, so the next failure of a chunk shows the notice instead. */
const reloadedJustNow = () => window.sessionStorage.setItem(CHUNK_RELOAD_KEY, String(Date.now()))

beforeEach(() => {
  reload = vi.fn()
  uninstall = installChunkRecovery(reload)
  vi.spyOn(console, 'error').mockImplementation(() => undefined)
})

afterEach(() => {
  uninstall()
  uninstall = () => undefined
  cleanup()
  dismissStaleBuildNotice()
  flush()
  vi.restoreAllMocks()
})

describe('AppShell when a routed page fails', () => {
  it.each(['/login', '/dashboard'])(
    'draws nothing for a stale chunk under %s, and the notice still comes up',
    async (path) => {
      renderApp(path, () => {
        throw staleChunkError()
      })
      await settle()
      expect(screen.queryByRole('alert')).toBeNull()
      expect(screen.queryByTestId('stale-build-notice')).toBeNull()

      reloadedJustNow()
      expect(preloadFails()).toBe(false)
      expect(screen.getByTestId('stale-build-notice')).toBeTruthy()
      expect(reload).not.toHaveBeenCalled()
    },
  )

  it.each(['/login', '/dashboard'])(
    'shows the generic error copy for any other failure under %s, and the notice still comes up',
    async (path) => {
      renderApp(path, () => {
        throw new Error('Cannot read properties of undefined')
      })
      expect((await screen.findByRole('alert')).textContent).toMatch(/Noe gikk galt|Something went wrong/)
      expect(console.error).toHaveBeenCalled()

      reloadedJustNow()
      preloadFails()
      expect(screen.getByTestId('stale-build-notice')).toBeTruthy()
    },
  )
})

// Runs last on purpose: a reload that started earlier in this file would stay marked for
// the rest of it, and the failure below would then be hidden for the wrong reason.
describe('AppShell while the automatic reload is under way', () => {
  it('draws nothing for the failure that follows the reload', async () => {
    // The first failure of a stale tab: the listener reloads, and the lazy import then
    // rejects while reading `default` from the undefined it resolved to.
    expect(preloadFails()).toBe(true)
    renderApp('/login', () => {
      throw new TypeError("Cannot read properties of undefined (reading 'default')")
    })
    await settle()
    expect(reload).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('alert')).toBeNull()
  })
})
