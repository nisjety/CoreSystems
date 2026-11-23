import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  checkSpaceService,
  noteRoomFresh,
  noteSendTransportLost,
  noteSendTransportOk,
  roomLastFreshAt,
  spaceConnectivity,
  spaceConnectivityBannerEnabled,
  spaceSendIdempotencyEnabled,
  watchBrowserConnectivity,
} from './space-connectivity'
import { clearSpaceRoomFeatures, publishSpaceRoomFeatures } from './space-room-features'

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

// The optional `/health` probe (WS01-S8). Each test stubs the answer it needs; the reset afterwards leaves the
// service answering, so no state leaks into the next test.
describe('the /health probe (WS01-S8, optional)', () => {
  const answer = (response: () => Promise<Response>) => vi.fn(async () => response())
  const ok = () => Promise.resolve(new Response('{}', { status: 200, headers: { 'Content-Type': 'application/json' } }))
  const status = (code: number) => () => Promise.resolve(new Response('', { status: code }))

  afterEach(async () => {
    vi.stubGlobal('fetch', answer(ok))
    await checkSpaceService()
    vi.unstubAllGlobals()
  })

  it('reads unreachable while the gateway does not answer, and online again once it does', async () => {
    vi.stubGlobal('fetch', answer(() => Promise.reject(new TypeError('network down'))))
    await checkSpaceService()
    expect(spaceConnectivity()).toBe('unreachable')
    vi.stubGlobal('fetch', answer(ok))
    await checkSpaceService()
    expect(spaceConnectivity()).toBe('online')
  })

  it('counts a server error as unreachable, and an answer below 500 as up', async () => {
    vi.stubGlobal('fetch', answer(status(503)))
    await checkSpaceService()
    expect(spaceConnectivity()).toBe('unreachable')
    vi.stubGlobal('fetch', answer(status(404)))
    await checkSpaceService()
    expect(spaceConnectivity()).toBe('online')
  })

  it('lets a newer probe win: an older answer that arrives late changes nothing', async () => {
    let answerFirst: (response: Response) => void = () => undefined
    const first = new Promise<Response>((resolve) => { answerFirst = resolve })
    vi.stubGlobal('fetch', vi.fn().mockReturnValueOnce(first).mockReturnValueOnce(ok()))
    const older = checkSpaceService()
    await checkSpaceService()
    expect(spaceConnectivity()).toBe('online')
    answerFirst(new Response('', { status: 503 }))
    await older
    expect(spaceConnectivity()).toBe('online')
  })

  it('a send that reaches the server, or a room read, clears a service the probe marked down', async () => {
    vi.stubGlobal('fetch', answer(status(503)))
    await checkSpaceService()
    expect(spaceConnectivity()).toBe('unreachable')
    noteSendTransportOk()
    flush()
    expect(spaceConnectivity()).toBe('online')

    vi.stubGlobal('fetch', answer(status(503)))
    await checkSpaceService()
    expect(spaceConnectivity()).toBe('unreachable')
    noteRoomFresh('probe-room-read', Date.now())
    flush()
    expect(spaceConnectivity()).toBe('online')
  })

  it('a down answer is stale when a sign of life arrived while the probe was out', async () => {
    let answerDown: (response: Response) => void = () => undefined
    const pending = new Promise<Response>((resolve) => { answerDown = resolve })
    vi.stubGlobal('fetch', vi.fn().mockReturnValueOnce(pending))
    const probe = checkSpaceService()
    noteRoomFresh('probe-room-overtaken', Date.now())
    flush()
    answerDown(new Response('', { status: 503 }))
    await probe
    expect(spaceConnectivity()).toBe('online')
  })

  it('asks /health when the watch starts online, without credentials, and not while offline', () => {
    const fetchSpy = vi.fn(ok)
    vi.stubGlobal('fetch', fetchSpy)
    stop?.()
    stop = watchBrowserConnectivity()
    expect(fetchSpy).toHaveBeenCalledWith('/health', expect.objectContaining({ method: 'GET', credentials: 'omit' }))

    stop?.()
    stop = undefined
    fetchSpy.mockClear()
    setNavigatorOnline(false)
    stop = watchBrowserConnectivity()
    expect(fetchSpy).not.toHaveBeenCalled()
  })
})

// The last moment a room heard from its server (WS01-S8): the banner and the live indicator date what is on screen.
describe('last fresh time per room (WS01-S8)', () => {
  it('remembers when the room last heard from its server, and says nothing for another room', () => {
    const at = Date.parse('2026-11-23T13:05:00Z')
    noteRoomFresh('fresh-room-a', at)
    flush()
    expect(roomLastFreshAt('fresh-room-a')).toBe(at)
    expect(roomLastFreshAt('fresh-room-b')).toBeUndefined()
  })

  it('a later stamp replaces the earlier one', () => {
    noteRoomFresh('fresh-room-c', Date.parse('2026-11-23T13:05:00Z'))
    flush()
    noteRoomFresh('fresh-room-c', Date.parse('2026-11-23T13:09:30Z'))
    flush()
    expect(roomLastFreshAt('fresh-room-c')).toBe(Date.parse('2026-11-23T13:09:30Z'))
  })

  it('stamps the moment it is noted when no time is given', () => {
    const before = Date.now()
    noteRoomFresh('fresh-room-d')
    flush()
    const after = Date.now()
    const stamped = roomLastFreshAt('fresh-room-d')
    expect(stamped).toBeGreaterThanOrEqual(before)
    expect(stamped).toBeLessThanOrEqual(after)
  })
})

describe('dev overrides of the BFF switches (WS01-S8)', () => {
  afterEach(() => clearSpaceRoomFeatures())

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

  it('an unset switch follows the context features: off until the room says on', () => {
    vi.stubEnv('VITE_SPACE_CONNECTIVITY_BANNER_UI', '')
    vi.stubEnv('VITE_SPACE_SEND_IDEMPOTENCY_API', '')
    expect(spaceConnectivityBannerEnabled()).toBe(false)
    expect(spaceSendIdempotencyEnabled()).toBe(false)
    publishSpaceRoomFeatures({ connectivity_banner: true, send_idempotency: true })
    expect(spaceConnectivityBannerEnabled()).toBe(true)
    expect(spaceSendIdempotencyEnabled()).toBe(true)
  })
})
