import { createRoot, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  createReaderAppearance,
  isReaderPolishEnabled,
  READER_DYSLEXIA_CACHE_KEY,
  readCachedDyslexiaFont,
  readerAttributes,
  readerSpacingFor,
  resolveReaderFontSize,
  writeCachedDyslexiaFont,
  type ReaderAppearanceDeps,
} from './reader-polish'

afterEach(() => {
  localStorage.clear()
  vi.restoreAllMocks()
})

/** The settings read runs after the first render, so let it land before a test acts. */
async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0))
  flush()
}

function mountAppearance(deps: ReaderAppearanceDeps) {
  let dispose: (() => void) | undefined
  const control = createRoot((done) => {
    dispose = done
    return createReaderAppearance(deps)
  })
  return { control, dispose: () => dispose?.() }
}

describe('reader polish switch', () => {
  it('turns on only for the literal true', () => {
    expect(isReaderPolishEnabled('true')).toBe(true)
    for (const raw of ['TRUE', '1', 'yes', '', undefined]) {
      expect(isReaderPolishEnabled(raw)).toBe(false)
    }
  })
})

describe('resolveReaderFontSize', () => {
  it('keeps each of the three stored sizes', () => {
    expect(resolveReaderFontSize('small')).toBe('small')
    expect(resolveReaderFontSize('medium')).toBe('medium')
    expect(resolveReaderFontSize('large')).toBe('large')
  })

  it('reads anything else at the default size, and never as an error', () => {
    for (const value of ['huge', 'Large', '', 42, null, undefined]) {
      expect(resolveReaderFontSize(value)).toBe('medium')
    }
  })
})

describe('readerSpacingFor', () => {
  it('leaves the spacing to the stored compact flag while the level switch is off', () => {
    for (const level of ['detailed', 'standard', 'compact', 'minimal'] as const) {
      expect(readerSpacingFor(false, level)).toBeUndefined()
    }
  })

  it('tightens the rhythm only at the compact and minimal levels', () => {
    expect(readerSpacingFor(true, 'detailed')).toBe('comfortable')
    expect(readerSpacingFor(true, 'standard')).toBe('comfortable')
    expect(readerSpacingFor(true, 'compact')).toBe('compact')
    expect(readerSpacingFor(true, 'minimal')).toBe('compact')
  })
})

describe('readerAttributes', () => {
  it('writes the size, and the font and spacing only when they are chosen', () => {
    expect(readerAttributes({ fontSize: 'large', dyslexiaFont: true, spacing: 'compact' })).toEqual({
      'data-reader-font-size': 'large',
      'data-reader-font': 'dyslexia',
      'data-reader-spacing': 'compact',
    })
    const plain = readerAttributes({ fontSize: 'medium', dyslexiaFont: false })
    expect(plain['data-reader-font-size']).toBe('medium')
    expect(plain['data-reader-font']).toBeUndefined()
    expect(plain['data-reader-spacing']).toBeUndefined()
  })
})

describe('dyslexia-friendly font cache', () => {
  it('remembers the choice on this device, and forgets it when it is turned off', () => {
    writeCachedDyslexiaFont(true)
    expect(localStorage.getItem(READER_DYSLEXIA_CACHE_KEY)).toBe('1')
    expect(readCachedDyslexiaFont()).toBe(true)
    writeCachedDyslexiaFont(false)
    expect(localStorage.getItem(READER_DYSLEXIA_CACHE_KEY)).toBeNull()
    expect(readCachedDyslexiaFont()).toBe(false)
  })

  it('reads an unknown cached value as off', () => {
    localStorage.setItem(READER_DYSLEXIA_CACHE_KEY, 'yes')
    expect(readCachedDyslexiaFont()).toBe(false)
  })

  it('does not fail when the browser refuses storage', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    vi.spyOn(Storage.prototype, 'removeItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    expect(readCachedDyslexiaFont()).toBe(false)
    expect(() => writeCachedDyslexiaFont(true)).not.toThrow()
    expect(() => writeCachedDyslexiaFont(false)).not.toThrow()
  })
})

describe('createReaderAppearance', () => {
  it('reads nothing and writes no attribute with the switch off, even with a cached choice', async () => {
    writeCachedDyslexiaFont(true)
    const load = vi.fn(async () => ({ fontSize: 'large' }))
    const { control, dispose } = mountAppearance({ enabled: false, load })
    await settle()
    expect(load).not.toHaveBeenCalled()
    expect(control.dyslexiaFont()).toBe(false)
    expect(control.attributes('compact')).toEqual({})
    dispose()
  })

  it('takes the stored text size once the settings are read', async () => {
    const load = vi.fn(async () => ({ fontSize: 'large' }))
    const { control, dispose } = mountAppearance({ enabled: true, load })
    await settle()
    expect(load).toHaveBeenCalledTimes(1)
    expect(control.attributes(undefined)['data-reader-font-size']).toBe('large')
    dispose()
  })

  it('keeps the default text size when the stored one is unknown, or when the read fails', async () => {
    const unknown = mountAppearance({ enabled: true, load: vi.fn(async () => ({ fontSize: 'huge' })) })
    await settle()
    expect(unknown.control.attributes(undefined)['data-reader-font-size']).toBe('medium')
    unknown.dispose()

    const failing = mountAppearance({
      enabled: true,
      load: vi.fn(async () => {
        throw new Error('down')
      }),
    })
    await settle()
    expect(failing.control.attributes(undefined)['data-reader-font-size']).toBe('medium')
    failing.dispose()
  })

  it('shows the font chosen on this device, and changes it on the page and on this device', async () => {
    writeCachedDyslexiaFont(true)
    const { control, dispose } = mountAppearance({ enabled: true, load: vi.fn(async () => ({})) })
    await settle()
    expect(control.dyslexiaFont()).toBe(true)
    expect(control.attributes('compact')['data-reader-font']).toBe('dyslexia')
    control.setDyslexiaFont(false)
    flush()
    expect(control.dyslexiaFont()).toBe(false)
    expect(control.attributes('compact')['data-reader-font']).toBeUndefined()
    expect(readCachedDyslexiaFont()).toBe(false)
    dispose()
  })

  it('passes the spacing through to the attributes', async () => {
    const { control, dispose } = mountAppearance({ enabled: true, load: vi.fn(async () => ({})) })
    await settle()
    expect(control.attributes('compact')['data-reader-spacing']).toBe('compact')
    expect(control.attributes(undefined)['data-reader-spacing']).toBeUndefined()
    dispose()
  })
})
