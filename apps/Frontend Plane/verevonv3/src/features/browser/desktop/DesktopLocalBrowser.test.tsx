// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, expect, it, vi } from 'vitest'
import { DesktopLocalBrowser } from './DesktopLocalBrowser'
import type { DesktopBrowserBridge } from './desktop-browser'

vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en }) }))

afterEach(() => {
  cleanup()
  document.querySelector('.global-assistant')?.remove()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

it('clips the native guest above the assistant and leaves the same tab available for handoff', async () => {
  const frames: FrameRequestCallback[] = []
  vi.spyOn(window, 'requestAnimationFrame').mockImplementation(callback => frames.push(callback))
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  })
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    return this.classList.contains('global-assistant')
      ? new DOMRect(60, 480, 400, 100)
      : new DOMRect(20, 150, 600, 500)
  })
  const assistant = document.createElement('section')
  assistant.className = 'global-assistant'
  document.body.append(assistant)

  const state = { url: 'https://www.finn.no/', title: 'FINN', isLoading: false, canGoBack: false, canGoForward: false }
  const snapshot = { ...state, heading: null, text: '', targets: [] }
  const setViewport = vi.fn(async () => undefined)
  const navigate = vi.fn(async () => snapshot)
  const subscribe = vi.fn<DesktopBrowserBridge['subscribe']>(() => vi.fn())
  const bridge: DesktopBrowserBridge = {
    getState: async () => state, setViewport, navigate, subscribe,
    command: async () => state, reset: async () => undefined, startRun: async () => 'run-1',
    execute: async () => snapshot, endRun: async () => undefined,
  }
  const stop = vi.fn()
  const [busy, setBusy] = createSignal(true)
  const view = render(() => <DesktopLocalBrowser bridge={bridge} agentBusy={busy()} onStop={stop} />)
  await Promise.resolve()
  flush()
  frames.shift()?.(0)
  expect(setViewport).toHaveBeenCalledExactlyOnceWith({ x: 20, y: 150, width: 600, height: 322 })

  const address = screen.getByRole('textbox', { name: 'Web address' }) as HTMLInputElement
  expect(address.disabled).toBe(true)
  fireEvent.click(screen.getByRole('button', { name: 'Stop and take over tab' }))
  expect(stop).toHaveBeenCalledTimes(1)
  setBusy(false)
  flush()
  expect(address.disabled).toBe(false)
  expect(address.value).toBe('https://www.finn.no/')
  fireEvent.input(address, { target: { value: 'finn.no/realestate/homes/search.html' } })
  flush()
  fireEvent.submit(address.form!)
  expect(navigate).toHaveBeenCalledExactlyOnceWith('https://finn.no/realestate/homes/search.html')

  view.unmount()
  expect(setViewport).toHaveBeenLastCalledWith(null)
})
