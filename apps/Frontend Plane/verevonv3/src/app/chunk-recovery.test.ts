// @vitest-environment jsdom

import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  CHUNK_RELOAD_KEY,
  CHUNK_RELOAD_WINDOW_MS,
  dismissStaleBuildNotice,
  installChunkRecovery,
  isChunkLoadError,
  registerReloadGuard,
  staleBuildNotice,
} from './chunk-recovery'

const NOW = 1_800_000_000_000

let uninstall: () => void = () => undefined
let reload: ReturnType<typeof vi.fn<() => void>>

/** Dispatch the event Vite fires and report whether a listener prevented the default. */
const failChunk = () => {
  const event = new Event('vite:preloadError', { cancelable: true })
  window.dispatchEvent(event)
  flush()
  return event.defaultPrevented
}

const install = () => {
  uninstall = installChunkRecovery(reload)
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(NOW)
  window.sessionStorage.clear()
  reload = vi.fn()
})

afterEach(() => {
  uninstall()
  uninstall = () => undefined
  dismissStaleBuildNotice()
  flush()
  vi.useRealTimers()
  vi.unstubAllEnvs()
  vi.restoreAllMocks()
})

describe('installChunkRecovery', () => {
  it('reloads once on the first failure, prevents the throw and records the time', () => {
    install()
    expect(failChunk()).toBe(true)
    expect(reload).toHaveBeenCalledTimes(1)
    expect(window.sessionStorage.getItem(CHUNK_RELOAD_KEY)).toBe(String(NOW))
    expect(staleBuildNotice()).toBe(false)
  })

  it('shows the notice instead of a second reload inside the window', () => {
    install()
    failChunk()
    vi.setSystemTime(NOW + CHUNK_RELOAD_WINDOW_MS - 1)
    expect(failChunk()).toBe(false)
    expect(reload).toHaveBeenCalledTimes(1)
    expect(staleBuildNotice()).toBe(true)
  })

  it('never reaches a third navigation however often the chunk fails', () => {
    install()
    for (let i = 0; i < 5; i += 1) failChunk()
    expect(reload).toHaveBeenCalledTimes(1)
  })

  it('reloads again once the window has passed', () => {
    install()
    failChunk()
    vi.setSystemTime(NOW + CHUNK_RELOAD_WINDOW_MS)
    expect(failChunk()).toBe(true)
    expect(reload).toHaveBeenCalledTimes(2)
  })

  it('does not reload, record or prevent anything while a guard is dirty', () => {
    install()
    const unregister = registerReloadGuard(() => true)
    expect(failChunk()).toBe(false)
    expect(reload).not.toHaveBeenCalled()
    expect(window.sessionStorage.getItem(CHUNK_RELOAD_KEY)).toBeNull()
    expect(staleBuildNotice()).toBe(true)
    unregister()
  })

  it('reloads again after the guard is unregistered', () => {
    install()
    registerReloadGuard(() => true)()
    expect(failChunk()).toBe(true)
    expect(reload).toHaveBeenCalledTimes(1)
  })

  it('treats a guard that throws as dirty', () => {
    install()
    const unregister = registerReloadGuard(() => {
      throw new Error('boom')
    })
    expect(failChunk()).toBe(false)
    expect(reload).not.toHaveBeenCalled()
    unregister()
  })

  it('one clean guard does not hide another dirty one', () => {
    install()
    const clean = registerReloadGuard(() => false)
    const dirty = registerReloadGuard(() => true)
    failChunk()
    expect(reload).not.toHaveBeenCalled()
    clean()
    dirty()
  })

  it('never reloads on its own when the marker cannot be stored', () => {
    vi.spyOn(window.sessionStorage, 'setItem').mockImplementation(() => {
      throw new Error('quota')
    })
    install()
    expect(failChunk()).toBe(false)
    expect(reload).not.toHaveBeenCalled()
    expect(staleBuildNotice()).toBe(true)
  })

  it('never reloads on its own when the marker cannot be read', () => {
    vi.spyOn(window.sessionStorage, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    install()
    expect(failChunk()).toBe(false)
    expect(reload).not.toHaveBeenCalled()
  })

  it('ignores a marker that is not a number', () => {
    window.sessionStorage.setItem(CHUNK_RELOAD_KEY, 'soon')
    install()
    expect(failChunk()).toBe(true)
    expect(reload).toHaveBeenCalledTimes(1)
  })

  it('stops listening once uninstalled', () => {
    install()
    uninstall()
    expect(failChunk()).toBe(false)
    expect(reload).not.toHaveBeenCalled()
  })
})

describe('chunkReloadStarted', () => {
  it('is set by the reload itself, not by a failure that only shows the notice', async () => {
    // A fresh module, so the reloads made by the tests above are not visible here.
    vi.useRealTimers()
    vi.resetModules()
    const fresh = await import('./chunk-recovery')
    uninstall = fresh.installChunkRecovery(reload)
    expect(fresh.chunkReloadStarted()).toBe(false)

    const unregister = fresh.registerReloadGuard(() => true)
    window.dispatchEvent(new Event('vite:preloadError', { cancelable: true }))
    expect(fresh.chunkReloadStarted()).toBe(false)
    unregister()

    window.dispatchEvent(new Event('vite:preloadError', { cancelable: true }))
    expect(reload).toHaveBeenCalledTimes(1)
    expect(fresh.chunkReloadStarted()).toBe(true)
  })
})

describe('isChunkLoadError', () => {
  it.each([
    'Failed to fetch dynamically imported module: https://app.example/assets/AuthPage-1.js',
    'error loading dynamically imported module: https://app.example/assets/AuthPage-1.js',
    'Importing a module script failed.',
    'Unable to preload CSS for /assets/index-1.css',
  ])('recognises the failure a replaced build leaves: %s', (message) => {
    expect(isChunkLoadError(new TypeError(message))).toBe(true)
  })

  it('reads a thrown string the same way', () => {
    expect(isChunkLoadError('Importing a module script failed.')).toBe(true)
  })

  it("does not take any other failure for a stale chunk, not even the reload path's own", () => {
    expect(isChunkLoadError(new TypeError("Cannot read properties of undefined (reading 'default')"))).toBe(false)
    expect(isChunkLoadError(new Error('boom'))).toBe(false)
    expect(isChunkLoadError({ code: 'forbidden' })).toBe(false)
    expect(isChunkLoadError(undefined)).toBe(false)
    expect(isChunkLoadError(null)).toBe(false)
  })
})

describe('VITE_CHUNK_RECOVERY_DISABLED', () => {
  it('turns recovery off only for the literal true', () => {
    vi.stubEnv('VITE_CHUNK_RECOVERY_DISABLED', 'true')
    install()
    expect(failChunk()).toBe(false)
    expect(reload).not.toHaveBeenCalled()
    expect(staleBuildNotice()).toBe(false)
  })

  it.each(['TRUE', '1', 'false', ''])('leaves recovery on for %j', (value) => {
    vi.stubEnv('VITE_CHUNK_RECOVERY_DISABLED', value)
    install()
    expect(failChunk()).toBe(true)
    expect(reload).toHaveBeenCalledTimes(1)
  })
})
