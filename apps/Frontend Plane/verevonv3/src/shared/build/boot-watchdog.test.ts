// @vitest-environment jsdom

import { readFileSync } from 'node:fs'
import { join } from 'node:path'

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// WS07-S2: public/boot-watchdog.js is a classic script that index.html loads before the module
// script. This test lives under src/ (not public/) so it is never copied into the image. It runs
// the real file against jsdom: the script is evaluated fresh for each case.

const SOURCE = readFileSync(join(process.cwd(), 'public', 'boot-watchdog.js'), 'utf8')
const GUARD_KEY = 'verevon.chunk-reload.v1'
const TIMEOUT_MS = 15_000

type BootWindow = Window & { __verevonBoot?: boolean }

let fetchMock: ReturnType<typeof vi.fn>
let sendBeacon: ReturnType<typeof vi.fn>
let reload: ReturnType<typeof vi.fn>
let originalLocation: Location

function runWatchdog() {
  // eslint-disable-next-line no-new-func
  new Function(SOURCE)()
}

function setBuildMeta(build: string | null) {
  document.head.querySelectorAll('meta[name="verevon-build"]').forEach((el) => el.remove())
  if (build !== null) {
    const meta = document.createElement('meta')
    meta.setAttribute('name', 'verevon-build')
    meta.setAttribute('content', build)
    document.head.appendChild(meta)
  }
}

function versionAnswer(buildId: string) {
  return Promise.resolve(
    new Response(JSON.stringify({ build_id: buildId }), { status: 200, headers: { 'content-type': 'application/json' } }),
  )
}

async function fire() {
  await vi.advanceTimersByTimeAsync(TIMEOUT_MS)
  await vi.advanceTimersByTimeAsync(0)
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(new Date('2026-10-27T10:00:00.000Z'))
  document.body.innerHTML = '<div id="root"></div>'
  window.sessionStorage.clear()
  window.localStorage.clear()
  delete (window as BootWindow).__verevonBoot
  setBuildMeta('build-aaaa')
  fetchMock = vi.fn()
  sendBeacon = vi.fn(() => true)
  reload = vi.fn()
  originalLocation = window.location
  Object.defineProperty(window, 'location', { value: { ...originalLocation, reload }, configurable: true })
  vi.stubGlobal('fetch', fetchMock)
  Object.defineProperty(navigator, 'sendBeacon', { value: sendBeacon, configurable: true })
})

afterEach(() => {
  Object.defineProperty(window, 'location', { value: originalLocation, configurable: true })
  vi.useRealTimers()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('boot watchdog', () => {
  it('does nothing when the app booted', async () => {
    runWatchdog()
    ;(window as BootWindow).__verevonBoot = true
    await fire()
    expect(fetchMock).not.toHaveBeenCalled()
    expect(reload).not.toHaveBeenCalled()
    expect(document.getElementById('verevon-boot-failure')).toBeNull()
    expect(sendBeacon).not.toHaveBeenCalled()
  })

  it('waits the full 15 s before acting', async () => {
    runWatchdog()
    await vi.advanceTimersByTimeAsync(TIMEOUT_MS - 1)
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('reloads once on a build_id mismatch, guarded by the shared chunk-reload key', async () => {
    fetchMock.mockImplementation(() => versionAnswer('build-bbbb'))
    runWatchdog()
    await fire()
    expect(fetchMock).toHaveBeenCalledWith('/version.json', { cache: 'no-store' })
    expect(reload).toHaveBeenCalledTimes(1)
    expect(window.sessionStorage.getItem(GUARD_KEY)).toBe(String(Date.now()))
    expect(document.getElementById('verevon-boot-failure')).toBeNull()

    // A second boot failure inside the guard window paints the card instead of looping.
    reload.mockClear()
    runWatchdog()
    await fire()
    expect(reload).not.toHaveBeenCalled()
    expect(document.getElementById('verevon-boot-failure')).not.toBeNull()
  })

  it('paints the card and beacons a content-free watchdog event when the build matches', async () => {
    fetchMock.mockImplementation(() => versionAnswer('build-aaaa'))
    runWatchdog()
    await fire()
    expect(reload).not.toHaveBeenCalled()
    const card = document.getElementById('verevon-boot-failure')
    expect(card?.getAttribute('role')).toBe('alert')
    expect(card?.querySelector('button')).not.toBeNull()
    expect(sendBeacon).toHaveBeenCalledTimes(1)
    const [url, blob] = sendBeacon.mock.calls[0] as [string, Blob]
    expect(url).toBe('/api/v1/client-diagnostics')
    const payload = JSON.parse(await blob.text()) as Record<string, unknown>
    expect(payload).toEqual({ kind: 'watchdog', build: 'build-aaaa' })
  })

  it('paints the card when version.json is HTML or the request fails', async () => {
    fetchMock.mockImplementationOnce(() =>
      Promise.resolve(new Response('<html></html>', { status: 200, headers: { 'content-type': 'text/html' } })),
    )
    runWatchdog()
    await fire()
    expect(document.getElementById('verevon-boot-failure')).not.toBeNull()
    expect(reload).not.toHaveBeenCalled()

    document.body.innerHTML = '<div id="root"></div>'
    fetchMock.mockImplementationOnce(() => Promise.reject(new TypeError('Failed to fetch')))
    runWatchdog()
    await fire()
    expect(document.getElementById('verevon-boot-failure')).not.toBeNull()
  })

  it('never reloads a dev build or a page without a build meta', async () => {
    fetchMock.mockImplementation(() => versionAnswer('build-bbbb'))
    setBuildMeta('dev')
    runWatchdog()
    await fire()
    expect(reload).not.toHaveBeenCalled()

    setBuildMeta(null)
    document.body.innerHTML = '<div id="root"></div>'
    runWatchdog()
    await fire()
    expect(reload).not.toHaveBeenCalled()
  })

  it('does not paint when the app boots while the version check is in flight', async () => {
    let release: (value: Response) => void = () => undefined
    fetchMock.mockImplementation(() => new Promise<Response>((resolve) => (release = resolve)))
    runWatchdog()
    await vi.advanceTimersByTimeAsync(TIMEOUT_MS)
    ;(window as BootWindow).__verevonBoot = true
    release(new Response(JSON.stringify({ build_id: 'build-aaaa' }), { status: 200, headers: { 'content-type': 'application/json' } }))
    await vi.advanceTimersByTimeAsync(0)
    expect(document.getElementById('verevon-boot-failure')).toBeNull()
    expect(reload).not.toHaveBeenCalled()
  })

  it('never throws when storage access throws, and does not reload without the guard', async () => {
    vi.spyOn(window.sessionStorage, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    vi.spyOn(window.sessionStorage, 'setItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    fetchMock.mockImplementation(() => versionAnswer('build-bbbb'))
    expect(() => runWatchdog()).not.toThrow()
    await expect(fire()).resolves.toBeUndefined()
    expect(reload).not.toHaveBeenCalled()
    expect(document.getElementById('verevon-boot-failure')).not.toBeNull()
  })

  it('shows English copy when the stored locale is English', async () => {
    window.localStorage.setItem('verevon.locale', 'en')
    fetchMock.mockImplementation(() => versionAnswer('build-aaaa'))
    runWatchdog()
    await fire()
    expect(document.getElementById('verevon-boot-failure')?.textContent).toContain('The app did not start')
  })
})
