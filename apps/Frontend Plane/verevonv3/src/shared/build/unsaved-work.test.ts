// @vitest-environment jsdom

import { afterEach, describe, expect, it } from 'vitest'

import { hasUnsavedWork, hasUnsavedWorkProbes, registerUnsavedWork } from './unsaved-work'

// WS07-S3: the unsaved-work registry shares its guard set with chunk recovery, so every test
// releases what it registers; `hasUnsavedWorkProbes()` is asserted against a clean registry.
const releases: Array<() => void> = []
const track = (release: () => void) => {
  releases.push(release)
  return release
}

afterEach(() => {
  while (releases.length > 0) releases.pop()?.()
})

describe('unsaved-work registry', () => {
  it('reports no probes and no unsaved work when nothing is registered', () => {
    expect(hasUnsavedWorkProbes()).toBe(false)
    expect(hasUnsavedWork()).toBe(false)
  })

  it('reports a registered probe and follows its answer', () => {
    let dirty = false
    track(registerUnsavedWork('composer', () => dirty))
    expect(hasUnsavedWorkProbes()).toBe(true)
    expect(hasUnsavedWork()).toBe(false)
    dirty = true
    expect(hasUnsavedWork()).toBe(true)
  })

  it('replaces the probe registered under the same key', () => {
    track(registerUnsavedWork('composer', () => true))
    track(registerUnsavedWork('composer', () => false))
    // The first probe is gone, not stacked: only the replacement counts.
    expect(hasUnsavedWork()).toBe(false)
    expect(hasUnsavedWorkProbes()).toBe(true)
  })

  it('only lets the owner of the current registration unregister it', () => {
    const releaseFirst = registerUnsavedWork('composer', () => false)
    const releaseSecond = track(registerUnsavedWork('composer', () => true))
    // The replaced probe's late cleanup must not drop its successor.
    releaseFirst()
    expect(hasUnsavedWorkProbes()).toBe(true)
    expect(hasUnsavedWork()).toBe(true)
    releaseSecond()
    expect(hasUnsavedWorkProbes()).toBe(false)
    expect(hasUnsavedWork()).toBe(false)
    // Unregistering twice is harmless.
    releaseSecond()
    expect(hasUnsavedWorkProbes()).toBe(false)
  })

  it('keeps probes under different keys independent', () => {
    const releaseA = track(registerUnsavedWork('a', () => false))
    track(registerUnsavedWork('b', () => true))
    expect(hasUnsavedWork()).toBe(true)
    releaseA()
    expect(hasUnsavedWorkProbes()).toBe(true)
    expect(hasUnsavedWork()).toBe(true)
  })

  it('counts a probe that throws as unsaved work', () => {
    track(
      registerUnsavedWork('broken', () => {
        throw new Error('probe failed')
      }),
    )
    expect(hasUnsavedWorkProbes()).toBe(true)
    expect(hasUnsavedWork()).toBe(true)
  })

  it('returns to a clean registry after the last unregister', () => {
    const release = registerUnsavedWork('only', () => true)
    expect(hasUnsavedWorkProbes()).toBe(true)
    release()
    expect(hasUnsavedWorkProbes()).toBe(false)
    expect(hasUnsavedWork()).toBe(false)
  })
})
