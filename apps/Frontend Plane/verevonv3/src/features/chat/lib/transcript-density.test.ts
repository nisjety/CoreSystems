import { createRoot, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  createTranscriptDensity,
  densityRules,
  isTranscriptDensityEnabled,
  readCachedDensity,
  resolveDensity,
  TRANSCRIPT_DENSITY_CACHE_KEY,
  UNRESTRICTED_RULES,
  writeCachedDensity,
  type TranscriptDensityDeps,
} from './transcript-density'

afterEach(() => {
  localStorage.clear()
  vi.restoreAllMocks()
})

/** `onSettled` and the settings read run after the first render, so let that happen before a test acts. */
async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

function mountDensity(deps: TranscriptDensityDeps) {
  let dispose: (() => void) | undefined
  const control = createRoot((done) => {
    dispose = done
    return createTranscriptDensity(deps)
  })
  return { control, dispose: () => dispose?.() }
}

const stored = { colorScheme: 'blue', compactMode: false, fontSize: 'medium', theme: 'light' }

describe('transcript density switch', () => {
  it('turns on only for the literal true', () => {
    expect(isTranscriptDensityEnabled('true')).toBe(true)
    for (const raw of ['TRUE', '1', 'yes', '', undefined]) {
      expect(isTranscriptDensityEnabled(raw)).toBe(false)
    }
  })
})

describe('resolveDensity', () => {
  it('reads a stored level as it is stored', () => {
    for (const level of ['detailed', 'standard', 'compact', 'minimal'] as const) {
      expect(resolveDensity({ transcriptDensity: level, compactMode: true })).toBe(level)
    }
  })

  it('derives compact from the older compact flag when no level is stored', () => {
    expect(resolveDensity({ compactMode: true })).toBe('compact')
    expect(resolveDensity({ transcriptDensity: '', compactMode: true })).toBe('compact')
  })

  it('reads at the standard level when nothing is stored and the flag is off or missing', () => {
    expect(resolveDensity({ compactMode: false })).toBe('standard')
    expect(resolveDensity({})).toBe('standard')
    expect(resolveDensity(null)).toBe('standard')
    expect(resolveDensity(undefined)).toBe('standard')
  })

  it('ignores a stored value outside the set rather than failing on it', () => {
    expect(resolveDensity({ transcriptDensity: 'huge', compactMode: false })).toBe('standard')
    expect(resolveDensity({ transcriptDensity: 'Compact', compactMode: true })).toBe('compact')
    expect(resolveDensity({ transcriptDensity: 42 })).toBe('standard')
  })
})

describe('densityRules', () => {
  it('folds settled turns only at compact and minimal', () => {
    expect(densityRules('detailed').foldSettled).toBe(false)
    expect(densityRules('standard').foldSettled).toBe(false)
    expect(densityRules('compact').foldSettled).toBe(true)
    expect(densityRules('minimal').foldSettled).toBe(true)
  })

  it('hides the tool and reasoning trail only at minimal', () => {
    expect(densityRules('detailed').showProcess).toBe(true)
    expect(densityRules('standard').showProcess).toBe(true)
    expect(densityRules('compact').showProcess).toBe(true)
    expect(densityRules('minimal').showProcess).toBe(false)
  })

  it('changes nothing when the switch is off', () => {
    expect(UNRESTRICTED_RULES).toEqual({ foldSettled: true, showProcess: true })
  })
})

describe('device cache', () => {
  it('remembers the level chosen on this device', () => {
    writeCachedDensity('minimal')
    expect(localStorage.getItem(TRANSCRIPT_DENSITY_CACHE_KEY)).toBe('minimal')
    expect(readCachedDensity()).toBe('minimal')
  })

  it('reads an unknown cached value as nothing', () => {
    localStorage.setItem(TRANSCRIPT_DENSITY_CACHE_KEY, 'huge')
    expect(readCachedDensity()).toBeUndefined()
  })

  it('does not fail when the browser refuses storage reads or writes', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    expect(readCachedDensity()).toBeUndefined()
    expect(() => writeCachedDensity('compact')).not.toThrow()
  })
})

describe('createTranscriptDensity', () => {
  it('takes the stored level once the settings are read, and caches it on this device', async () => {
    const load = vi.fn(async () => ({ ...stored, transcriptDensity: 'minimal' }))
    const { control, dispose } = mountDensity({ enabled: true, load, save: vi.fn(async () => undefined) })
    await settle()
    expect(control.level()).toBe('minimal')
    expect(readCachedDensity()).toBe('minimal')
    expect(control.rules().showProcess).toBe(false)
    dispose()
  })

  it('keeps a level the person chose before the settings read landed', async () => {
    let finishLoad: (value: { transcriptDensity?: string }) => void = () => undefined
    const load = vi.fn(
      () =>
        new Promise<{ transcriptDensity?: string }>((resolve) => {
          finishLoad = resolve
        }),
    )
    const save = vi.fn(async () => undefined)
    const { control, dispose } = mountDensity({ enabled: true, load, save })
    await settle()
    control.choose('compact')
    finishLoad({ transcriptDensity: 'minimal' })
    await settle()
    expect(control.level()).toBe('compact')
    expect(save).toHaveBeenCalledWith('compact')
    dispose()
  })

  it('puts the previous level back when the save does not land', async () => {
    const load = vi.fn(async () => ({ ...stored, transcriptDensity: 'standard' }))
    const save = vi.fn(async () => {
      throw new Error('refused')
    })
    const { control, dispose } = mountDensity({ enabled: true, load, save })
    await settle()
    control.choose('minimal')
    expect(control.level()).toBe('minimal')
    await settle()
    expect(control.level()).toBe('standard')
    expect(readCachedDensity()).toBe('standard')
    dispose()
  })

  it('reads nothing and saves nothing with the switch off', async () => {
    const load = vi.fn(async () => ({ ...stored, transcriptDensity: 'minimal' }))
    const save = vi.fn(async () => undefined)
    const { control, dispose } = mountDensity({ enabled: false, load, save })
    await settle()
    expect(load).not.toHaveBeenCalled()
    expect(control.level()).toBe('standard')
    expect(control.rules()).toEqual(UNRESTRICTED_RULES)
    dispose()
  })
})
