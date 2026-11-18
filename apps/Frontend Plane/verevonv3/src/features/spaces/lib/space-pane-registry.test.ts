import { afterEach, describe, expect, it, vi } from 'vitest'

import { resolveOpenSpacePane, SPACE_PANES } from './space-pane-registry'
import { clearSpaceRoomFeatures, primeSpaceRoomFeatures } from './space-room-features'

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('space pane registry (P-SPA1)', () => {
  it('registers the thread pane on ?thread_id', () => {
    expect(SPACE_PANES.map((pane) => [pane.id, pane.searchParam])).toEqual([
      ['thread', 'thread_id'],
      ['private_panel', 'panel'],
      ['browser', 'browser_run'],
    ])
  })

  describe('resolveOpenSpacePane', () => {
    it('returns the thread pane for a valid thread_id', () => {
      vi.stubEnv('VITE_SPACE_THREAD_PANE_UI', 'true')
      expect(resolveOpenSpacePane({ thread_id: 'thr_1' })).toEqual({ id: 'thread', value: 'thr_1' })
      expect(resolveOpenSpacePane({ thread_id: '  thr_2 ' })).toEqual({ id: 'thread', value: 'thr_2' })
    })

    it('returns null for a blank, missing, repeated or over-long parameter', () => {
      vi.stubEnv('VITE_SPACE_THREAD_PANE_UI', 'true')
      expect(resolveOpenSpacePane({})).toBeNull()
      expect(resolveOpenSpacePane({ thread_id: '' })).toBeNull()
      expect(resolveOpenSpacePane({ thread_id: '   ' })).toBeNull()
      expect(resolveOpenSpacePane({ thread_id: undefined })).toBeNull()
      expect(resolveOpenSpacePane({ thread_id: ['a', 'b'] })).toBeNull()
      expect(resolveOpenSpacePane({ thread_id: 'x'.repeat(201) })).toBeNull()
      expect(resolveOpenSpacePane({ thread_id: 'x'.repeat(200) })).toEqual({ id: 'thread', value: 'x'.repeat(200) })
    })

    it('ignores parameters that are not a registered pane', () => {
      vi.stubEnv('VITE_SPACE_THREAD_PANE_UI', 'true')
      expect(resolveOpenSpacePane({ run_id: 'run_1', other: 'x' })).toBeNull()
    })

    it('returns null when the pane is disabled, even for a valid parameter', () => {
      vi.stubEnv('VITE_SPACE_THREAD_PANE_UI', 'false')
      expect(resolveOpenSpacePane({ thread_id: 'thr_1' })).toBeNull()
    })
  })
})

describe('space pane registry: private panel (WS17-S6)', () => {
  afterEach(() => {
    clearSpaceRoomFeatures()
  })

  it('opens the private panel on ?panel=private only while features.private_panel is on', () => {
    primeSpaceRoomFeatures({ private_panel: false })
    expect(resolveOpenSpacePane({ panel: 'private' })).toBeNull()
    primeSpaceRoomFeatures({})
    expect(resolveOpenSpacePane({ panel: 'private' })).toBeNull()
    primeSpaceRoomFeatures({ private_panel: true })
    expect(resolveOpenSpacePane({ panel: 'private' })).toEqual({ id: 'private_panel', value: 'private' })
  })

  it('names nothing for any other ?panel value, a repeated one or a missing one', () => {
    primeSpaceRoomFeatures({ private_panel: true })
    expect(resolveOpenSpacePane({ panel: 'public' })).toBeNull()
    expect(resolveOpenSpacePane({ panel: 'Private' })).toBeNull()
    expect(resolveOpenSpacePane({ panel: '' })).toBeNull()
    expect(resolveOpenSpacePane({ panel: ['private'] })).toBeNull()
    expect(resolveOpenSpacePane({ panel: ['private', 'private'] })).toBeNull()
    expect(resolveOpenSpacePane({ panel: undefined })).toBeNull()
  })

  it('keeps ?thread_id ahead of ?panel=private', () => {
    vi.stubEnv('VITE_SPACE_THREAD_PANE_UI', 'true')
    primeSpaceRoomFeatures({ thread_pane: true, private_panel: true })
    expect(resolveOpenSpacePane({ thread_id: 'thr_1', panel: 'private' })).toEqual({ id: 'thread', value: 'thr_1' })
  })
})

describe('space pane registry: browser run (WS20-S5)', () => {
  afterEach(() => {
    clearSpaceRoomFeatures()
  })

  it('resolves ?browser_run to the browser pane only while features.browser_card is on', () => {
    primeSpaceRoomFeatures({ browser_card: false })
    expect(resolveOpenSpacePane({ browser_run: 'run_1' })).toBeNull()
    primeSpaceRoomFeatures({ browser_card: true })
    expect(resolveOpenSpacePane({ browser_run: 'run_1' })).toEqual({ id: 'browser', value: 'run_1' })
    primeSpaceRoomFeatures({ browser_card: false })
    expect(resolveOpenSpacePane({ browser_run: 'run_1' })).toBeNull()
  })
})
