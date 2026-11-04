// @vitest-environment jsdom

import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  base64UrlToBytes,
  bytesToBase64Url,
  disablePushOnThisDevice,
  fetchPushConfig,
  pushSupport,
  subscriptionBody,
} from '@/features/core/lib/push-subscription'

// Vitest runs from the Verevon v3 root, which is where the worker is served from.
const workerSource = readFileSync(resolve(process.cwd(), 'public', 'sw.js'), 'utf8')

const APPLICATION_SERVER_KEY = 'BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('push key encoding', () => {
  it('round-trips the base64url application server key', () => {
    const bytes = base64UrlToBytes(APPLICATION_SERVER_KEY)

    expect(bytes.length).toBe(65)
    expect(bytes[0]).toBe(0x04)
    expect(bytesToBase64Url(bytes.buffer)).toBe(APPLICATION_SERVER_KEY)
  })

  it('stores a subscription as its endpoint and base64url keys', () => {
    const auth = new Uint8Array(16).fill(7).buffer as ArrayBuffer
    const p256dh = new Uint8Array(65).fill(4).buffer as ArrayBuffer

    const body = subscriptionBody({
      endpoint: 'https://fcm.googleapis.com/fcm/send/abc',
      getKey: (name) => (name === 'auth' ? auth : p256dh),
    })

    expect(body.endpoint).toBe('https://fcm.googleapis.com/fcm/send/abc')
    expect(body.keys.auth).toBe(bytesToBase64Url(auth))
    expect(body.keys.p256dh).not.toMatch(/[+/=]/)
  })
})

describe('push configuration and support', () => {
  it('reads whether the server accepts Web Push and its public key', async () => {
    const fetchMock = vi.fn(async (_input: RequestInfo | URL) => jsonResponse({ enabled: true, public_key: APPLICATION_SERVER_KEY }))
    vi.stubGlobal('fetch', fetchMock)

    await expect(fetchPushConfig()).resolves.toEqual({ enabled: true, publicKey: APPLICATION_SERVER_KEY })
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/v1/push/config')
  })

  it('reports a disabled server without a key', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ enabled: false })))

    await expect(fetchPushConfig()).resolves.toEqual({ enabled: false, publicKey: null })
  })

  it('reports an insecure context before anything else', () => {
    Object.defineProperty(window, 'isSecureContext', { value: false, configurable: true })
    expect(pushSupport()).toBe('insecure')
    Object.defineProperty(window, 'isSecureContext', { value: true, configurable: true })
  })

  it('reports a browser without the push APIs as unsupported', () => {
    Object.defineProperty(window, 'isSecureContext', { value: true, configurable: true })
    vi.stubGlobal('navigator', { ...navigator, serviceWorker: undefined })
    expect(pushSupport()).toBe('unsupported')
  })
})

describe('disabling push on this device', () => {
  it('removes the stored subscription and then unsubscribes the browser', async () => {
    const fetchMock = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => new Response(null, { status: 204 }))
    vi.stubGlobal('fetch', fetchMock)
    const unsubscribe = vi.fn(async () => true)
    vi.stubGlobal('navigator', {
      ...navigator,
      serviceWorker: {
        getRegistration: async () => ({ pushManager: { getSubscription: async () => ({ unsubscribe }) } }),
      },
    })

    await disablePushOnThisDevice('wps_1')

    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/v1/push/subscriptions/wps_1')
    expect((fetchMock.mock.calls[0]?.[1] as RequestInit).method).toBe('DELETE')
    expect(unsubscribe).toHaveBeenCalledTimes(1)
  })

  it('unsubscribes the browser alone when this session does not know the server id', async () => {
    const fetchMock = vi.fn(async () => new Response(null, { status: 204 }))
    vi.stubGlobal('fetch', fetchMock)
    const unsubscribe = vi.fn(async () => true)
    vi.stubGlobal('navigator', {
      ...navigator,
      serviceWorker: {
        getRegistration: async () => ({ pushManager: { getSubscription: async () => ({ unsubscribe }) } }),
      },
    })

    await disablePushOnThisDevice(null)

    expect(fetchMock).not.toHaveBeenCalled()
    expect(unsubscribe).toHaveBeenCalledTimes(1)
  })
})

type WorkerListener = (event: Record<string, unknown>) => void

type FakeWindow = { visibilityState: string; focused: boolean; focus?: () => Promise<void>; navigate?: (url: string) => Promise<unknown> }

// Runs the real public/sw.js against a fake worker global. The worker has no fetch listener and no cache use, and the
// tests below check that from the file itself, so a later edit that adds either fails here.
function loadWorker(windows: FakeWindow[] = []) {
  const listeners = new Map<string, WorkerListener>()
  const shown: Array<{ title: string; options: Record<string, unknown> }> = []
  const opened: string[] = []
  const fakeSelf = {
    addEventListener: (type: string, listener: WorkerListener) => {
      listeners.set(type, listener)
    },
    skipWaiting: () => Promise.resolve(),
    clients: {
      claim: () => Promise.resolve(),
      matchAll: () => Promise.resolve(windows),
      openWindow: (url: string) => {
        opened.push(url)
        return Promise.resolve(null)
      },
    },
    registration: {
      showNotification: (title: string, options: Record<string, unknown>) => {
        shown.push({ title, options })
        return Promise.resolve()
      },
    },
  }
  new Function('self', workerSource)(fakeSelf)
  return { listeners, shown, opened }
}

async function dispatch(listener: WorkerListener | undefined, event: Record<string, unknown>) {
  const pending: Promise<unknown>[] = []
  listener?.({ ...event, waitUntil: (promise: Promise<unknown>) => pending.push(promise) })
  await Promise.all(pending)
}

describe('attention service worker', () => {
  it('shows the fixed attention notice when no window is visible', async () => {
    const worker = loadWorker([])

    await dispatch(worker.listeners.get('push'), {})

    expect(worker.shown).toHaveLength(1)
    expect(worker.shown[0]?.title).toBe('Verevon')
    expect(worker.shown[0]?.options.body).toBe('A chat needs your attention.')
    expect(worker.shown[0]?.options.data).toEqual({ url: '/chat?filter=attention' })
  })

  it('stays silent when a window is visible or focused', async () => {
    const worker = loadWorker([{ visibilityState: 'visible', focused: false }])

    await dispatch(worker.listeners.get('push'), {})

    expect(worker.shown).toHaveLength(0)
  })

  it('opens the attention rail when the notice is clicked', async () => {
    const worker = loadWorker([])
    const close = vi.fn()

    await dispatch(worker.listeners.get('notificationclick'), { notification: { close, data: {} } })

    expect(close).toHaveBeenCalledTimes(1)
    expect(worker.opened).toEqual(['/chat?filter=attention'])
  })

  it('has no fetch handler and never touches the Cache API', () => {
    const worker = loadWorker([])

    expect(worker.listeners.has('fetch')).toBe(false)
    expect(workerSource).not.toMatch(/caches\./)
  })
})
