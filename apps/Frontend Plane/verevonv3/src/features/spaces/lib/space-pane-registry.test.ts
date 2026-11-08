import { afterEach, describe, expect, it, vi } from 'vitest'

import { resolveOpenSpacePane, SPACE_PANES } from './space-pane-registry'

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('space pane registry (P-SPA1)', () => {
  it('registers the thread pane on ?thread_id', () => {
    expect(SPACE_PANES.map((pane) => [pane.id, pane.searchParam])).toEqual([['thread', 'thread_id']])
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
