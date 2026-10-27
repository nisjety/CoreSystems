import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  noteSendTransportLost,
  noteSendTransportOk,
  spaceConnectivity,
  spaceConnectivityBannerEnabled,
  spaceSendIdempotencyEnabled,
  watchBrowserConnectivity,
} from './space-connectivity'

const setNavigatorOnline = (online: boolean) => {
  Object.defineProperty(window.navigator, 'onLine', { configurable: true, get: () => online })
}

let stop: (() => void) | undefined

beforeEach(() => {
  setNavigatorOnline(true)
  noteSendTransportOk()
  // Called outside any component, so the first sync is not an owned write.
  stop = watchBrowserConnectivity()
  window.dispatchEvent(new Event('online'))
  flush()
})

afterEach(() => {
  stop?.()
  stop = undefined
  vi.unstubAllEnvs()
  setNavigatorOnline(true)
  window.dispatchEvent(new Event('online'))
  noteSendTransportOk()
  flush()
})

describe('space connectivity state (WS01-S8)', () => {
  it('is online by default and says nothing', () => {
    expect(spaceConnectivity()).toBe('online')
  })

  it('reads offline after the browser says so, and online again on the online event', () => {
    setNavigatorOnline(false)
    window.dispatchEvent(new Event('offline'))
    flush()
    expect(spaceConnectivity()).toBe('offline')
    setNavigatorOnline(true)
    window.dispatchEvent(new Event('online'))
    flush()
    expect(spaceConnectivity()).toBe('online')
  })

  it('reads unreachable after a send transport was lost, and online after one reached the server', () => {
    noteSendTransportLost()
    flush()
    expect(spaceConnectivity()).toBe('unreachable')
    noteSendTransportOk()
    flush()
    expect(spaceConnectivity()).toBe('online')
  })

  it('lets offline win over unreachable', () => {
    noteSendTransportLost()
    setNavigatorOnline(false)
    window.dispatchEvent(new Event('offline'))
    flush()
    expect(spaceConnectivity()).toBe('offline')
  })

  it('clears unreachable when the browser comes back online: a banner must not outlive its cause', () => {
    noteSendTransportLost()
    flush()
    expect(spaceConnectivity()).toBe('unreachable')
    window.dispatchEvent(new Event('online'))
    flush()
    expect(spaceConnectivity()).toBe('online')
  })

  it('stops following the browser events once the returned stop function is called', () => {
    stop?.()
    stop = undefined
    setNavigatorOnline(false)
    window.dispatchEvent(new Event('offline'))
    flush()
    expect(spaceConnectivity()).toBe('online')
  })

  // DEPTH-SPA-2 (fixed): the first sync() writes only when the state changed, so the watcher can start
  // inside an owned scope without Solid 2 refusing the write.
  it('DEPTH-SPA-2: can be started from inside an owned scope without throwing', () => {
    createRoot((dispose) => {
      const off = watchBrowserConnectivity()
      off()
      dispose()
    })
  })
})

describe('build-time switches (WS01-S8)', () => {
  it('the banner follows VITE_SPACE_CONNECTIVITY_BANNER_UI, true and false both explicit', () => {
    vi.stubEnv('VITE_SPACE_CONNECTIVITY_BANNER_UI', 'true')
    expect(spaceConnectivityBannerEnabled()).toBe(true)
    vi.stubEnv('VITE_SPACE_CONNECTIVITY_BANNER_UI', 'false')
    expect(spaceConnectivityBannerEnabled()).toBe(false)
  })

  it('the send key follows VITE_SPACE_SEND_IDEMPOTENCY_API, true and false both explicit', () => {
    vi.stubEnv('VITE_SPACE_SEND_IDEMPOTENCY_API', 'true')
    expect(spaceSendIdempotencyEnabled()).toBe(true)
    vi.stubEnv('VITE_SPACE_SEND_IDEMPOTENCY_API', 'false')
    expect(spaceSendIdempotencyEnabled()).toBe(false)
  })

  it('an unset switch is on only in dev: production builds default off', () => {
    vi.stubEnv('VITE_SPACE_CONNECTIVITY_BANNER_UI', '')
    vi.stubEnv('VITE_SPACE_SEND_IDEMPOTENCY_API', '')
    expect(spaceConnectivityBannerEnabled()).toBe(import.meta.env.DEV === true)
    expect(spaceSendIdempotencyEnabled()).toBe(import.meta.env.DEV === true)
  })
})
