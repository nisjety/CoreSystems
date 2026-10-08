// @vitest-environment jsdom
import { createRoot, createStore, flush, untrack } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))
import {
  HOME_LAYOUT_DEFAULTS,
  homeLayoutStorageKey,
  readHomeLayout,
  useHomeLayout,
  writeHomeLayout,
} from './home-layout-state'

type Identity = { user: { id: string } | null; activeOrg: { id: string } | null }
const [identity, setIdentity] = createStore<Identity>({ user: { id: 'user-a' }, activeOrg: { id: 'org-a' } })
const signIn = (user: string | null, org: string | null) => {
  setIdentity((state) => { state.user = user ? { id: user } : null; state.activeOrg = org ? { id: org } : null })
  flush()
}
const keyA = homeLayoutStorageKey('user-a', 'org-a')!
const keyB = homeLayoutStorageKey('user-b', 'org-a')!
const stored = (key: string) => JSON.parse(localStorage.getItem(key) ?? 'null')
const roots: Array<() => void> = []
// Mounts the hook the way a component does and returns an untracked reader.
function mount() {
  let api!: ReturnType<typeof useHomeLayout>
  roots.push(createRoot((dispose) => { api = useHomeLayout(); return dispose }))
  return { state: () => untrack(api[0]), update: api[1] }
}

beforeEach(() => {
  localStorage.clear(); sessionStorage.clear()
  signIn('user-a', 'org-a')
})
afterEach(() => {
  vi.restoreAllMocks()
  for (const dispose of roots.splice(0)) dispose()
})

describe('home layout storage key', () => {
  it('is scoped by workspace and user, like the chat organizer', () => {
    expect(keyA).toBe('verevon.home-layout.v1:org-a:user-a')
    expect(homeLayoutStorageKey('a:b/c', 'o rg')).toBe('verevon.home-layout.v1:o%20rg:a%3Ab%2Fc')
  })

  it('exists only when both the user and the workspace are known', () => {
    for (const [user, org] of [[undefined, 'org-a'], ['user-a', undefined], [undefined, undefined], ['', 'org-a'], ['user-a', ''], ['  ', 'org-a'], ['user-a', ' ']] as const) {
      expect(homeLayoutStorageKey(user, org)).toBeNull()
    }
  })
})

describe('reading the remembered layout', () => {
  it('returns the defaults for an unknown identity without touching storage', () => {
    const read = vi.spyOn(Storage.prototype, 'getItem')
    expect(readHomeLayout(null)).toEqual(HOME_LAYOUT_DEFAULTS)
    expect(read).not.toHaveBeenCalled()
  })

  it('returns the defaults when nothing is stored', () => {
    expect(readHomeLayout(keyA)).toEqual({ dockMinimized: false, dockExpanded: false, feed: 'news', category: 'all', pinnedOnly: false })
  })

  it('does not hand out the shared defaults object', () => {
    readHomeLayout(null).feed = 'traffic'
    readHomeLayout(keyA).feed = 'traffic'
    expect(HOME_LAYOUT_DEFAULTS.feed).toBe('news')
  })

  it('restores every stored field', () => {
    const saved = { dockMinimized: true, dockExpanded: true, feed: 'aquatiq', category: 'Business', pinnedOnly: true }
    localStorage.setItem(keyA, JSON.stringify(saved))
    expect(readHomeLayout(keyA)).toEqual(saved)
  })

  it('accepts each feed the existing feed schema knows', () => {
    for (const feed of ['news', 'traffic', 'aquatiq'] as const) {
      localStorage.setItem(keyA, JSON.stringify({ feed }))
      expect(readHomeLayout(keyA).feed).toBe(feed)
    }
  })

  it('falls back to the defaults for corrupt or non-object data', () => {
    for (const raw of ['{not json', '', 'null', '[]', '"open"', '42', 'true', '[{"feed":"traffic"}]']) {
      localStorage.setItem(keyA, raw)
      expect(readHomeLayout(keyA)).toEqual(HOME_LAYOUT_DEFAULTS)
    }
  })

  it('falls back field by field and keeps the valid rest', () => {
    localStorage.setItem(keyA, JSON.stringify({ dockMinimized: 'yes', dockExpanded: true, feed: 'radio', category: 'evil&limit=9999', pinnedOnly: 1 }))
    expect(readHomeLayout(keyA)).toEqual({ ...HOME_LAYOUT_DEFAULTS, dockExpanded: true })
    localStorage.setItem(keyA, JSON.stringify({ feed: 'traffic', category: 'general', dockMinimized: null }))
    expect(readHomeLayout(keyA)).toEqual({ ...HOME_LAYOUT_DEFAULTS, feed: 'traffic' })
  })

  it('drops fields it does not know', () => {
    localStorage.setItem(keyA, JSON.stringify({ feed: 'traffic', tab: 'web', extra: { nested: true } }))
    expect(readHomeLayout(keyA)).toEqual({ ...HOME_LAYOUT_DEFAULTS, feed: 'traffic' })
  })

  it('reads another identity as its own, never the previous one', () => {
    writeHomeLayout(keyA, { feed: 'traffic', pinnedOnly: true })
    expect(readHomeLayout(keyB)).toEqual(HOME_LAYOUT_DEFAULTS)
  })
})

describe('writing a partial update', () => {
  it('merges the patch into what is stored and returns the saved state', () => {
    expect(writeHomeLayout(keyA, { pinnedOnly: true })).toEqual({ ...HOME_LAYOUT_DEFAULTS, pinnedOnly: true })
    expect(writeHomeLayout(keyA, { feed: 'aquatiq', category: 'Technology' })).toEqual({ ...HOME_LAYOUT_DEFAULTS, pinnedOnly: true, feed: 'aquatiq', category: 'Technology' })
    expect(stored(keyA)).toEqual({ dockMinimized: false, dockExpanded: false, feed: 'aquatiq', category: 'Technology', pinnedOnly: true })
  })

  it('writes nothing while the identity is unknown', () => {
    const write = vi.spyOn(Storage.prototype, 'setItem')
    expect(writeHomeLayout(null, { feed: 'traffic' })).toEqual(HOME_LAYOUT_DEFAULTS)
    expect(write).not.toHaveBeenCalled()
  })

  it('refuses a patch with any invalid field as a whole', () => {
    writeHomeLayout(keyA, { feed: 'traffic' })
    const write = vi.spyOn(Storage.prototype, 'setItem')
    writeHomeLayout(keyA, { pinnedOnly: true, feed: 'radio' } as never)
    writeHomeLayout(keyA, { category: 'evil' })
    expect(write).not.toHaveBeenCalled()
    expect(stored(keyA).feed).toBe('traffic')
    expect(stored(keyA).pinnedOnly).toBe(false)
  })

  it('repairs corrupt stored data with the next write', () => {
    localStorage.setItem(keyA, '{broken')
    writeHomeLayout(keyA, { dockMinimized: true })
    expect(stored(keyA)).toEqual({ ...HOME_LAYOUT_DEFAULTS, dockMinimized: true })
  })

  it('leaves another identity untouched', () => {
    writeHomeLayout(keyB, { feed: 'traffic' })
    writeHomeLayout(keyA, { feed: 'aquatiq' })
    expect(stored(keyB).feed).toBe('traffic')
  })
})

describe('useHomeLayout', () => {
  it('has the restored state on creation, before any effect runs', () => {
    writeHomeLayout(keyA, { dockMinimized: true, feed: 'traffic', category: 'Business', pinnedOnly: true })
    const layout = mount()
    expect(layout.state()).toEqual({ dockMinimized: true, dockExpanded: false, feed: 'traffic', category: 'Business', pinnedOnly: true })
  })

  it('starts from the defaults when nothing is stored', () => {
    expect(mount().state()).toEqual(HOME_LAYOUT_DEFAULTS)
  })

  it('updates the state and remembers it', () => {
    const layout = mount()
    layout.update({ dockMinimized: true })
    layout.update({ feed: 'aquatiq' })
    flush()
    expect(layout.state()).toEqual({ ...HOME_LAYOUT_DEFAULTS, dockMinimized: true, feed: 'aquatiq' })
    expect(stored(keyA)).toEqual({ ...HOME_LAYOUT_DEFAULTS, dockMinimized: true, feed: 'aquatiq' })
    expect(mount().state()).toEqual({ ...HOME_LAYOUT_DEFAULTS, dockMinimized: true, feed: 'aquatiq' })
  })

  it('lets components that own different fields update the same record', () => {
    const home = mount()
    const recent = mount()
    const updates = mount()
    home.update({ dockExpanded: true })
    recent.update({ pinnedOnly: true })
    updates.update({ feed: 'traffic', category: 'General' })
    flush()
    expect(stored(keyA)).toEqual({ dockMinimized: false, dockExpanded: true, feed: 'traffic', category: 'General', pinnedOnly: true })
    expect(home.state().feed).toBe('news')
  })

  it('ignores an invalid update', () => {
    const layout = mount()
    layout.update({ feed: 'radio' } as never)
    layout.update({ category: 'evil' })
    flush()
    expect(layout.state()).toEqual(HOME_LAYOUT_DEFAULTS)
    expect(localStorage.getItem(keyA)).toBeNull()
  })

  it('falls back to the defaults for invalid stored data', () => {
    localStorage.setItem(keyA, JSON.stringify({ dockMinimized: 'true', feed: 7, category: ['all'], pinnedOnly: {} }))
    expect(mount().state()).toEqual(HOME_LAYOUT_DEFAULTS)
    localStorage.setItem(keyA, 'not json at all')
    expect(mount().state()).toEqual(HOME_LAYOUT_DEFAULTS)
  })

  describe('identity', () => {
    it('reads and writes nothing until both the user and the workspace are known', () => {
      signIn('user-a', null)
      writeHomeLayout(keyA, { feed: 'traffic' })
      const read = vi.spyOn(Storage.prototype, 'getItem')
      const write = vi.spyOn(Storage.prototype, 'setItem')
      const layout = mount()
      expect(layout.state()).toEqual(HOME_LAYOUT_DEFAULTS)
      layout.update({ dockMinimized: true })
      flush()
      expect(layout.state().dockMinimized).toBe(true)
      signIn(null, 'org-a')
      layout.update({ pinnedOnly: true })
      flush()
      expect(read).not.toHaveBeenCalled()
      expect(write).not.toHaveBeenCalled()
      expect(stored(keyA)).toEqual({ ...HOME_LAYOUT_DEFAULTS, feed: 'traffic' })
    })

    it('restores the remembered layout as soon as the identity arrives', () => {
      writeHomeLayout(keyA, { feed: 'traffic', dockMinimized: true })
      signIn(null, null)
      const layout = mount()
      expect(layout.state()).toEqual(HOME_LAYOUT_DEFAULTS)
      signIn('user-a', null)
      expect(layout.state()).toEqual(HOME_LAYOUT_DEFAULTS)
      signIn('user-a', 'org-a')
      expect(layout.state()).toEqual({ ...HOME_LAYOUT_DEFAULTS, feed: 'traffic', dockMinimized: true })
    })

    it('re-reads for a new user and never writes under the previous one', () => {
      writeHomeLayout(keyA, { feed: 'traffic', pinnedOnly: true })
      writeHomeLayout(keyB, { dockExpanded: true })
      const layout = mount()
      expect(layout.state().feed).toBe('traffic')
      signIn('user-b', 'org-a')
      expect(layout.state()).toEqual({ ...HOME_LAYOUT_DEFAULTS, dockExpanded: true })
      layout.update({ feed: 'aquatiq' })
      flush()
      expect(stored(keyB)).toEqual({ ...HOME_LAYOUT_DEFAULTS, dockExpanded: true, feed: 'aquatiq' })
      expect(stored(keyA)).toEqual({ ...HOME_LAYOUT_DEFAULTS, feed: 'traffic', pinnedOnly: true })
      signIn('user-a', 'org-a')
      expect(layout.state()).toEqual({ ...HOME_LAYOUT_DEFAULTS, feed: 'traffic', pinnedOnly: true })
    })

    it('re-reads when only the workspace changes', () => {
      const keyOrgB = homeLayoutStorageKey('user-a', 'org-b')!
      writeHomeLayout(keyA, { feed: 'traffic' })
      const layout = mount()
      signIn('user-a', 'org-b')
      expect(layout.state()).toEqual(HOME_LAYOUT_DEFAULTS)
      layout.update({ pinnedOnly: true })
      flush()
      expect(stored(keyOrgB)).toEqual({ ...HOME_LAYOUT_DEFAULTS, pinnedOnly: true })
      expect(stored(keyA)).toEqual({ ...HOME_LAYOUT_DEFAULTS, feed: 'traffic' })
    })

    it('does not carry a signed-out account into the next one on a shared computer', () => {
      const layout = mount()
      layout.update({ dockMinimized: true, feed: 'aquatiq', pinnedOnly: true })
      flush()
      signIn(null, null)
      expect(layout.state()).toEqual(HOME_LAYOUT_DEFAULTS)
      signIn('user-b', 'org-a')
      expect(layout.state()).toEqual(HOME_LAYOUT_DEFAULTS)
      expect(localStorage.getItem(keyB)).toBeNull()
    })

    it('keeps the state when the session is refreshed with the same identity', () => {
      const layout = mount()
      layout.update({ feed: 'traffic' })
      flush()
      setIdentity((state) => { state.user = { id: 'user-a' }; state.activeOrg = { id: 'org-a' } })
      flush()
      expect(layout.state().feed).toBe('traffic')
    })
  })
})
