import { createEffect, createRoot, flush } from 'solid-js'
import { afterEach, describe, expect, it } from 'vitest'

import { commandPaletteEnabled, setCommandPaletteFeature } from './command-registry'

afterEach(() => {
  setCommandPaletteFeature(undefined)
})

describe('command palette switch (DEPTH-WS13-S6-1)', () => {
  it('the gateway answer overrides the build default at once, and undoing it restores the default', () => {
    setCommandPaletteFeature(undefined)
    const buildDefault = commandPaletteEnabled()
    setCommandPaletteFeature(!buildDefault)
    expect(commandPaletteEnabled()).toBe(!buildDefault)
    setCommandPaletteFeature(undefined)
    expect(commandPaletteEnabled()).toBe(buildDefault)
  })

  it('a reader that tracks the switch runs again when the answer changes after mount, and not otherwise', () => {
    setCommandPaletteFeature(false)
    const seen: boolean[] = []
    const dispose = createRoot((done) => {
      createEffect(
        () => commandPaletteEnabled(),
        (enabled) => {
          seen.push(enabled)
        },
      )
      return done
    })
    flush()
    setCommandPaletteFeature(true)
    flush()
    setCommandPaletteFeature(true)
    flush()
    setCommandPaletteFeature(false)
    flush()
    dispose()
    expect(seen).toEqual([false, true, false])
  })
})
