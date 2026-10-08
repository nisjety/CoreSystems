import { afterEach, describe, expect, it, vi } from 'vitest'

const KEY = 'verevon.chat-appearance-preview.v3'
const OLDER_KEYS = ['verevon.chat-appearance-preview.v1', 'verevon.chat-appearance-preview.v2']
const allOff = { welcome: false, dock: false, composer: false, history: false, navigation: false, organizer: false, context: false, dashboard: false }

// Choices are read once when the module loads, so each case loads a fresh copy
// (with its matching Solid instance for flushing writes).
async function loadPreview() {
  vi.resetModules()
  const [{ flush }, preview] = await Promise.all([import('solid-js'), import('./chat-appearance-preview')])
  return { flush, ...preview }
}

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('production appearance', () => {
  it('always renders every refinement, whatever this browser saved', async () => {
    vi.stubEnv('DEV', false)
    window.localStorage.setItem(KEY, JSON.stringify(allOff))
    const { chatAppearanceActive, chatAppearanceKeys } = await loadPreview()

    expect(chatAppearanceKeys.filter((key) => !chatAppearanceActive(key))).toEqual([])
  })
})

describe('development appearance', () => {
  it('starts a fresh browser with every refinement on', async () => {
    const { chatAppearanceActive, chatAppearanceKeys } = await loadPreview()

    expect(chatAppearanceKeys.filter((key) => !chatAppearanceActive(key))).toEqual([])
  })

  it('ignores choices saved against the old all-off default', async () => {
    for (const key of OLDER_KEYS) window.localStorage.setItem(key, JSON.stringify({ ...allOff, welcome: true }))
    const { chatAppearanceActive, chatAppearanceKeys } = await loadPreview()

    expect(chatAppearanceKeys.filter((key) => !chatAppearanceActive(key))).toEqual([])
  })

  it('remembers only the switches turned off', async () => {
    const preview = await loadPreview()
    preview.chooseChatAppearance('dock', false)
    preview.flush()
    expect(preview.chatAppearanceActive('dock')).toBe(false)
    expect(JSON.parse(window.localStorage.getItem(KEY) ?? 'null')).toEqual({ dock: false })

    const reloaded = await loadPreview()
    expect(reloaded.chatAppearanceActive('dock')).toBe(false)
    expect(reloaded.chatAppearanceActive('composer')).toBe(true)

    reloaded.chooseChatAppearance('dock', true)
    reloaded.flush()
    expect(window.localStorage.getItem(KEY)).toBeNull()
  })

  it('compares against the legacy UI and resets to the standard', async () => {
    window.localStorage.setItem(OLDER_KEYS[0]!, JSON.stringify({ welcome: true }))
    const preview = await loadPreview()
    preview.chooseChatAppearance('navigation', false)
    preview.compareChatOriginal(true)
    preview.flush()
    expect(preview.chatAppearanceKeys.filter(preview.chatAppearanceActive)).toEqual([])

    preview.resetChatAppearance()
    preview.flush()
    expect(preview.chatAppearanceKeys.filter((key) => !preview.chatAppearanceActive(key))).toEqual([])
    for (const key of [KEY, ...OLDER_KEYS]) expect(window.localStorage.getItem(key)).toBeNull()
  })
})
