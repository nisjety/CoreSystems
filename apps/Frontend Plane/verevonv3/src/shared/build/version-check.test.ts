// @vitest-environment jsdom

import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const state = vi.hoisted(() => ({ buildId: 'build-aaaa' }))

vi.mock('@/shared/build/build-info', () => ({
  get BUILD_ID() {
    return state.buildId
  },
  isDevBuild: () => state.buildId === 'dev',
}))
vi.mock('@/shared/build/client-contract', () => ({ checkClientContract: vi.fn(() => Promise.resolve()) }))

const json = (body: unknown, init: { status?: number; type?: string } = {}) =>
  new Response(typeof body === 'string' ? body : JSON.stringify(body), {
    status: init.status ?? 200,
    headers: { 'content-type': init.type ?? 'application/json' },
  })

let fetchMock: ReturnType<typeof vi.fn>

async function load() {
  vi.resetModules()
  return import('./version-check')
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(new Date('2026-10-27T10:00:00.000Z'))
  state.buildId = 'build-aaaa'
  fetchMock = vi.fn()
  vi.stubGlobal('fetch', fetchMock)
  window.sessionStorage.clear()
})

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('liveBuildIdFrom', () => {
  it('reads a non-empty string build_id and nothing else', async () => {
    const { liveBuildIdFrom } = await load()
    expect(liveBuildIdFrom({ build_id: 'abc' })).toBe('abc')
    expect(liveBuildIdFrom({ build_id: '' })).toBeNull()
    expect(liveBuildIdFrom({ build_id: 7 })).toBeNull()
    expect(liveBuildIdFrom(null)).toBeNull()
    expect(liveBuildIdFrom('abc')).toBeNull()
  })
})

describe('checkClientVersion', () => {
  it('sets newVersionBuildId when the live build differs', async () => {
    const mod = await load()
    fetchMock.mockResolvedValueOnce(json({ build_id: 'build-bbbb' }))
    await mod.checkClientVersion()
    expect(fetchMock).toHaveBeenCalledWith('/version.json', { cache: 'no-store', credentials: 'omit' })
    expect(mod.newVersionBuildId()).toBe('build-bbbb')
  })

  it('stays quiet when the live build equals this tab', async () => {
    const mod = await load()
    fetchMock.mockResolvedValueOnce(json({ build_id: 'build-aaaa' }))
    await mod.checkClientVersion()
    expect(mod.newVersionBuildId()).toBeNull()
  })

  it('ignores an HTML (non-JSON) response such as the SPA fallback', async () => {
    const mod = await load()
    fetchMock.mockResolvedValueOnce(json('<!doctype html><html></html>', { type: 'text/html' }))
    await mod.checkClientVersion()
    expect(mod.newVersionBuildId()).toBeNull()
  })

  it('ignores a non-ok answer and a body without a build_id', async () => {
    const mod = await load()
    fetchMock.mockResolvedValueOnce(json({ build_id: 'build-bbbb' }, { status: 500 }))
    await mod.checkClientVersion()
    expect(mod.newVersionBuildId()).toBeNull()
    vi.advanceTimersByTime(mod.MIN_CHECK_GAP_MS + 1)
    fetchMock.mockResolvedValueOnce(json({ other: 'x' }))
    await mod.checkClientVersion()
    expect(mod.newVersionBuildId()).toBeNull()
  })

  it('never banners a dev build and never calls fetch', async () => {
    state.buildId = 'dev'
    const mod = await load()
    await mod.checkClientVersion({ force: true })
    expect(fetchMock).not.toHaveBeenCalled()
    expect(mod.newVersionBuildId()).toBeNull()
  })

  it('keeps a dismissed id hidden and shows a newer id again', async () => {
    const mod = await load()
    fetchMock.mockResolvedValueOnce(json({ build_id: 'build-bbbb' }))
    await mod.checkClientVersion()
    expect(mod.newVersionBuildId()).toBe('build-bbbb')

    mod.dismissNewVersion()
    flush()
    expect(mod.newVersionBuildId()).toBeNull()
    expect(window.sessionStorage.getItem(mod.DISMISSED_KEY)).toBe('build-bbbb')

    vi.advanceTimersByTime(mod.MIN_CHECK_GAP_MS + 1)
    fetchMock.mockResolvedValueOnce(json({ build_id: 'build-bbbb' }))
    await mod.checkClientVersion()
    expect(mod.newVersionBuildId()).toBeNull()

    vi.advanceTimersByTime(mod.MIN_CHECK_GAP_MS + 1)
    fetchMock.mockResolvedValueOnce(json({ build_id: 'build-cccc' }))
    await mod.checkClientVersion()
    expect(mod.newVersionBuildId()).toBe('build-cccc')
  })

  it('throttles checks inside 30 s and lets force override', async () => {
    const mod = await load()
    fetchMock.mockResolvedValue(json({ build_id: 'build-aaaa' }))
    await mod.checkClientVersion()
    await mod.checkClientVersion()
    expect(fetchMock).toHaveBeenCalledTimes(1)

    vi.advanceTimersByTime(mod.MIN_CHECK_GAP_MS - 1000)
    await mod.checkClientVersion()
    expect(fetchMock).toHaveBeenCalledTimes(1)

    await mod.checkClientVersion({ force: true })
    expect(fetchMock).toHaveBeenCalledTimes(2)

    vi.advanceTimersByTime(mod.MIN_CHECK_GAP_MS + 1)
    await mod.checkClientVersion()
    expect(fetchMock).toHaveBeenCalledTimes(3)
  })

  it('collapses concurrent calls into one request', async () => {
    const mod = await load()
    let release: (value: Response) => void = () => undefined
    fetchMock.mockReturnValueOnce(new Promise<Response>((resolve) => (release = resolve)))
    const first = mod.checkClientVersion({ force: true })
    const second = mod.checkClientVersion({ force: true })
    release(json({ build_id: 'build-bbbb' }))
    await Promise.all([first, second])
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(mod.newVersionBuildId()).toBe('build-bbbb')
  })

  it('swallows a fetch that throws and tries again on the next trigger', async () => {
    const mod = await load()
    fetchMock.mockRejectedValueOnce(new TypeError('Failed to fetch'))
    await expect(mod.checkClientVersion()).resolves.toBeUndefined()
    expect(mod.newVersionBuildId()).toBeNull()

    vi.advanceTimersByTime(mod.MIN_CHECK_GAP_MS + 1)
    fetchMock.mockResolvedValueOnce(json({ build_id: 'build-bbbb' }))
    await mod.checkClientVersion()
    expect(mod.newVersionBuildId()).toBe('build-bbbb')
  })

  it('still dismisses for the render when sessionStorage throws', async () => {
    const mod = await load()
    fetchMock.mockResolvedValueOnce(json({ build_id: 'build-bbbb' }))
    await mod.checkClientVersion()
    const spy = vi.spyOn(window.sessionStorage, 'setItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    expect(() => mod.dismissNewVersion()).not.toThrow()
    flush()
    expect(mod.newVersionBuildId()).toBeNull()
    spy.mockRestore()
  })
})

describe('startVersionCheck', () => {
  it('checks at start and removes its listeners on teardown', async () => {
    const mod = await load()
    fetchMock.mockResolvedValue(json({ build_id: 'build-aaaa' }))
    const stop = mod.startVersionCheck()
    expect(fetchMock).toHaveBeenCalledTimes(1)
    stop()
    vi.advanceTimersByTime(mod.MIN_CHECK_GAP_MS + 1)
    window.dispatchEvent(new Event('online'))
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('does nothing in a dev build', async () => {
    state.buildId = 'dev'
    const mod = await load()
    const stop = mod.startVersionCheck()
    stop()
    expect(fetchMock).not.toHaveBeenCalled()
  })
})
