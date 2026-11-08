// @vitest-environment jsdom
import { createRoot, createStore, flush, untrack } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { CHAT_RUN_PANEL_COLLAPSED_KEY } from '@/features/chat/lib/chat-run-watch'

vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))

import {
  BROWSER_DOCK_DEFAULTS,
  browserDockPageClass,
  browserDockPatchSchema,
  browserDockSchema,
  browserDockSizeCss,
  browserDockStorageKey,
  isChatBrowserDockEnabled,
  nextBrowserDockPosition,
  readBrowserDockState,
  toggleBrowserDockPosition,
  useBrowserDock,
  writeBrowserDockState,
  type BrowserDockState,
} from './browser-dock-state'

type Identity = { user: { id: string } | null; activeOrg: { id: string } | null }
const [identity, setIdentity] = createStore<Identity>({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const signIn = (user: string | null, org: string | null) => {
  setIdentity((state) => {
    state.user = user ? { id: user } : null
    state.activeOrg = org ? { id: org } : null
  })
  flush()
}

const keyA = browserDockStorageKey('user-a', 'org-a')!
const keyB = browserDockStorageKey('user-b', 'org-a')!
const stored = (key: string) => JSON.parse(localStorage.getItem(key) ?? 'null')
const dockKeys = () =>
  Array.from({ length: localStorage.length }, (_, index) => localStorage.key(index) ?? '').filter((key) =>
    key.startsWith('verevon.chat.browser-dock'),
  )
const roots: Array<() => void> = []

// Mounts the hook the way a component does and returns an untracked reader.
function mount() {
  let api!: ReturnType<typeof useBrowserDock>
  roots.push(createRoot((dispose) => {
    api = useBrowserDock()
    return dispose
  }))
  return { state: () => untrack(api[0]), update: api[1] }
}

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  signIn('user-a', 'org-a')
})

afterEach(() => {
  for (const dispose of roots.splice(0)) dispose()
  vi.unstubAllEnvs()
})

describe('browser dock storage key', () => {
  it('is scoped by workspace then user, under the v1 dock prefix', () => {
    expect(keyA).toBe('verevon.chat.browser-dock.v1:org-a:user-a')
    expect(browserDockStorageKey('a:b/c', 'o rg')).toBe('verevon.chat.browser-dock.v1:o%20rg:a%3Ab%2Fc')
  })

  it('exists only when both the user and the workspace are known', () => {
    for (const [user, org] of [[undefined, 'org-a'], ['user-a', undefined], [undefined, undefined], ['', 'org-a'], ['user-a', '  ']] as const) {
      expect(browserDockStorageKey(user, org)).toBeNull()
    }
  })
})

describe('browser dock record schema', () => {
  it('accepts the defaults and the range edges of the size', () => {
    expect(browserDockSchema.safeParse(BROWSER_DOCK_DEFAULTS).success).toBe(true)
    expect(browserDockSchema.safeParse({ ...BROWSER_DOCK_DEFAULTS, size: 0.25 }).success).toBe(true)
    expect(browserDockSchema.safeParse({ ...BROWSER_DOCK_DEFAULTS, size: 0.7 }).success).toBe(true)
  })

  it('refuses a size outside 0.25 to 0.7, an unknown layout, tab rule or flag', () => {
    expect(browserDockSchema.safeParse({ ...BROWSER_DOCK_DEFAULTS, size: 0.2 }).success).toBe(false)
    expect(browserDockSchema.safeParse({ ...BROWSER_DOCK_DEFAULTS, size: 0.71 }).success).toBe(false)
    expect(browserDockSchema.safeParse({ ...BROWSER_DOCK_DEFAULTS, position: 'left' }).success).toBe(false)
    expect(browserDockSchema.safeParse({ ...BROWSER_DOCK_DEFAULTS, chatPerTab: 'both' }).success).toBe(false)
    expect(browserDockSchema.safeParse({ ...BROWSER_DOCK_DEFAULTS, composerHidden: 'yes' }).success).toBe(false)
  })

  it('refuses any field beyond the four, so no URL, title, session id or run id can be stored', () => {
    const extras = { url: 'https://example.com/private', title: 'Private page', sessionId: 'sess_1', runId: 'run_1' }
    for (const [field, value] of Object.entries(extras)) {
      expect(browserDockSchema.safeParse({ ...BROWSER_DOCK_DEFAULTS, [field]: value }).success).toBe(false)
    }
    expect(browserDockPatchSchema.safeParse({ url: 'https://example.com' }).success).toBe(false)
  })
})

describe('writing the remembered layout', () => {
  it('stores exactly four fields and returns what it stored', () => {
    const saved = writeBrowserDockState(keyA, { ...BROWSER_DOCK_DEFAULTS, position: 'bottom' })
    expect(saved).toEqual({ position: 'bottom', size: 0.4, chatPerTab: 'keep', composerHidden: false })
    expect(Object.keys(stored(keyA)).sort()).toEqual(['chatPerTab', 'composerHidden', 'position', 'size'])
  })

  it('refuses a state with an extra field and stores nothing for it', () => {
    const withUrl = { ...BROWSER_DOCK_DEFAULTS, url: 'https://example.com/private?token=1' } as BrowserDockState
    expect(writeBrowserDockState(keyA, withUrl)).toBeNull()
    expect(localStorage.getItem(keyA)).toBeNull()
  })

  it('stores nothing without an identity', () => {
    expect(writeBrowserDockState(null, BROWSER_DOCK_DEFAULTS)).toBeNull()
    expect(dockKeys()).toEqual([])
  })
})

describe('reading the remembered layout', () => {
  it('returns the defaults for an unknown identity and does not read the old flag', () => {
    localStorage.setItem(CHAT_RUN_PANEL_COLLAPSED_KEY, '1')
    expect(readBrowserDockState(null)).toEqual(BROWSER_DOCK_DEFAULTS)
  })

  it('returns the defaults when nothing is stored', () => {
    expect(readBrowserDockState(keyA)).toEqual({ position: 'right', size: 0.4, chatPerTab: 'keep', composerHidden: false })
  })

  it('does not hand out the shared defaults object', () => {
    readBrowserDockState(null).position = 'hidden'
    readBrowserDockState(keyA).position = 'hidden'
    expect(BROWSER_DOCK_DEFAULTS.position).toBe('right')
  })

  it('migrates a collapsed rail to hidden, and a read writes nothing', () => {
    localStorage.setItem(CHAT_RUN_PANEL_COLLAPSED_KEY, '1')
    expect(readBrowserDockState(keyA).position).toBe('hidden')
    expect(localStorage.getItem(keyA)).toBeNull()
  })

  it('reads an expanded rail as the default layout', () => {
    localStorage.setItem(CHAT_RUN_PANEL_COLLAPSED_KEY, '0')
    expect(readBrowserDockState(keyA).position).toBe('right')
  })

  it('prefers a stored record over the old flag', () => {
    localStorage.setItem(CHAT_RUN_PANEL_COLLAPSED_KEY, '1')
    localStorage.setItem(keyA, JSON.stringify({ position: 'bottom', size: 0.5, chatPerTab: 'keep', composerHidden: false }))
    expect(readBrowserDockState(keyA)).toEqual({ position: 'bottom', size: 0.5, chatPerTab: 'keep', composerHidden: false })
  })

  it('treats a corrupt record as missing and falls back to the migrated value', () => {
    localStorage.setItem(keyA, '{not json')
    expect(readBrowserDockState(keyA).position).toBe('right')
    localStorage.setItem(CHAT_RUN_PANEL_COLLAPSED_KEY, '1')
    expect(readBrowserDockState(keyA).position).toBe('hidden')
  })

  it('treats a stored value that is not an object as missing', () => {
    localStorage.setItem(keyA, '"bottom"')
    expect(readBrowserDockState(keyA).position).toBe('right')
  })

  it('reads each field forgivingly, so one bad value keeps the rest', () => {
    localStorage.setItem(keyA, JSON.stringify({ position: 'maximised', size: 9, chatPerTab: 'new', composerHidden: true }))
    expect(readBrowserDockState(keyA)).toEqual({ position: 'maximised', size: 0.4, chatPerTab: 'new', composerHidden: true })
  })

  it('drops anything beyond the four fields, even when a hand edit put it there', () => {
    localStorage.setItem(keyA, JSON.stringify({ position: 'bottom', url: 'https://example.com/x', title: 'Secret', sessionId: 'sess_1', runId: 'run_1' }))
    const read = readBrowserDockState(keyA)
    expect(read).toEqual({ position: 'bottom', size: 0.4, chatPerTab: 'keep', composerHidden: false })
    expect(Object.keys(read).sort()).toEqual(['chatPerTab', 'composerHidden', 'position', 'size'])
  })
})

describe('layout reducers and classes', () => {
  it('cycles right, bottom, maximised, hidden and back to right', () => {
    expect(nextBrowserDockPosition('right')).toBe('bottom')
    expect(nextBrowserDockPosition('bottom')).toBe('maximised')
    expect(nextBrowserDockPosition('maximised')).toBe('hidden')
    expect(nextBrowserDockPosition('hidden')).toBe('right')
  })

  it('hides an open dock and reopens a hidden one in the layout it had', () => {
    expect(toggleBrowserDockPosition('bottom')).toBe('hidden')
    expect(toggleBrowserDockPosition('hidden')).toBe('right')
    expect(toggleBrowserDockPosition('hidden', 'maximised')).toBe('maximised')
  })

  it('maps each open layout to its page class and gives a hidden dock none', () => {
    expect(browserDockPageClass('right')).toBe('verevon-chat-page--dock-right')
    expect(browserDockPageClass('bottom')).toBe('verevon-chat-page--dock-bottom')
    expect(browserDockPageClass('maximised')).toBe('verevon-chat-page--dock-maximised')
    expect(browserDockPageClass('hidden')).toBeNull()
  })

  it('writes the bottom size as a whole-number percentage of the page', () => {
    expect(browserDockSizeCss({ size: 0.4 })).toBe('40%')
    expect(browserDockSizeCss({ size: 0.25 })).toBe('25%')
    expect(browserDockSizeCss({ size: 0.7 })).toBe('70%')
  })
})

describe('browser dock flag', () => {
  it('is off unless VITE_CHAT_BROWSER_DOCK is 1 or true', () => {
    vi.unstubAllEnvs()
    expect(isChatBrowserDockEnabled()).toBe(false)
    for (const value of ['', '0', 'false', 'yes']) {
      vi.stubEnv('VITE_CHAT_BROWSER_DOCK', value)
      expect(isChatBrowserDockEnabled()).toBe(false)
    }
    vi.stubEnv('VITE_CHAT_BROWSER_DOCK', '1')
    expect(isChatBrowserDockEnabled()).toBe(true)
    vi.stubEnv('VITE_CHAT_BROWSER_DOCK', 'true')
    expect(isChatBrowserDockEnabled()).toBe(true)
  })
})

describe('useBrowserDock', () => {
  it('saves an update for the current identity and reads it back', () => {
    const dock = mount()
    dock.update({ position: 'bottom' })
    expect(dock.state().position).toBe('bottom')
    expect(stored(keyA)).toEqual({ position: 'bottom', size: 0.4, chatPerTab: 'keep', composerHidden: false })
  })

  it('merges a partial update into the stored record instead of overwriting it', () => {
    const dock = mount()
    dock.update({ position: 'bottom' })
    dock.update({ composerHidden: true })
    expect(stored(keyA)).toEqual({ position: 'bottom', size: 0.4, chatPerTab: 'keep', composerHidden: true })
  })

  it('gives another person in the same workspace the defaults and leaves the first record alone', () => {
    const first = mount()
    first.update({ position: 'bottom' })
    signIn('user-b', 'org-a')
    const second = mount()
    expect(second.state().position).toBe('right')
    expect(stored(keyA).position).toBe('bottom')
    expect(localStorage.getItem(keyB)).toBeNull()
  })

  it('holds a change for the view only, with no identity, and stores nothing', () => {
    signIn(null, 'org-a')
    const dock = mount()
    dock.update({ position: 'hidden' })
    expect(dock.state().position).toBe('hidden')
    expect(dockKeys()).toEqual([])
  })

  it('refuses an invalid patch and changes nothing', () => {
    const dock = mount()
    dock.update({ url: 'https://example.com' } as never)
    expect(dock.state()).toEqual(BROWSER_DOCK_DEFAULTS)
    expect(localStorage.getItem(keyA)).toBeNull()
  })
})
