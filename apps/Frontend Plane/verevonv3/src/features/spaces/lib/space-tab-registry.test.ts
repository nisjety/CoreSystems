import { afterEach, describe, expect, it, vi } from 'vitest'

import { DM_SPACE_FEATURE } from './space-kind'
import { primeSpaceRoomFeatures } from './space-room-features'
import { DEFAULT_SPACE_TAB, SPACE_TABS, spaceTabFromHash, visibleSpaceTabs } from './space-tab-registry'

afterEach(() => {
  vi.unstubAllEnvs()
  primeSpaceRoomFeatures(undefined)
})

describe('space tab registry (P-SPA1)', () => {
  it('lists the room tabs in their fixed order with unique ids', () => {
    const ids = SPACE_TABS.map((tab) => tab.id)
    // Filer (the Space files tab) sits after the artefakter tab, as main added it.
    expect(ids).toEqual(['chat', 'arbeid', 'kunnskap', 'artefakter', 'filer', 'aktivitet', 'agent', 'medlemmer'])
    expect(new Set(ids).size).toBe(ids.length)
    expect(DEFAULT_SPACE_TAB).toBe('chat')
  })

  describe('spaceTabFromHash', () => {
    it('resolves a registered id, with or without the leading #, case-insensitively', () => {
      expect(spaceTabFromHash('#kunnskap')).toBe('kunnskap')
      expect(spaceTabFromHash('aktivitet')).toBe('aktivitet')
      expect(spaceTabFromHash('#Agent')).toBe('agent')
      expect(spaceTabFromHash('#  medlemmer ')).toBe('medlemmer')
    })

    it('resolves the legacy English aliases to their tabs', () => {
      expect(spaceTabFromHash('#members')).toBe('medlemmer')
      expect(spaceTabFromHash('#work')).toBe('arbeid')
      expect(spaceTabFromHash('#activity')).toBe('aktivitet')
    })

    it('sends an unknown, empty or hostile hash to chat', () => {
      expect(spaceTabFromHash('')).toBe('chat')
      expect(spaceTabFromHash('#')).toBe('chat')
      expect(spaceTabFromHash('#nope')).toBe('chat')
      expect(spaceTabFromHash('#<script>')).toBe('chat')
      expect(spaceTabFromHash('#__proto__')).toBe('chat')
    })

    it('skips a tab whose enabled() is false and falls back to chat', () => {
      vi.stubEnv('VITE_SPACE_ROOM_ARTIFACTS_UI', 'false')
      expect(visibleSpaceTabs().map((tab) => tab.id)).not.toContain('artefakter')
      expect(spaceTabFromHash('#artefakter')).toBe('chat')
      // The other tabs are untouched.
      expect(spaceTabFromHash('#kunnskap')).toBe('kunnskap')
    })

    it('shows a tab again when its enabled() says yes', () => {
      vi.stubEnv('VITE_SPACE_ROOM_ARTIFACTS_UI', 'true')
      expect(visibleSpaceTabs().map((tab) => tab.id)).toContain('artefakter')
      expect(spaceTabFromHash('#artefakter')).toBe('artefakter')
    })
  })

  describe('a direct message (WS12-S3)', () => {
    it('hides the agent and members tabs and keeps the rest', () => {
      primeSpaceRoomFeatures({ [DM_SPACE_FEATURE]: true })
      const ids = visibleSpaceTabs().map((tab) => tab.id)
      expect(ids).not.toContain('agent')
      expect(ids).not.toContain('medlemmer')
      expect(ids).toContain('chat')
      expect(ids).toContain('aktivitet')
      expect(ids).toContain('arbeid')
    })

    it('sends a hash for a hidden tab, and its legacy alias, to chat', () => {
      primeSpaceRoomFeatures({ [DM_SPACE_FEATURE]: true })
      expect(spaceTabFromHash('#agent')).toBe('chat')
      expect(spaceTabFromHash('#medlemmer')).toBe('chat')
      expect(spaceTabFromHash('#members')).toBe('chat')
      expect(spaceTabFromHash('#aktivitet')).toBe('aktivitet')
    })

    it('a room (the flag false or absent) keeps both tabs', () => {
      primeSpaceRoomFeatures({ [DM_SPACE_FEATURE]: false })
      expect(visibleSpaceTabs().map((tab) => tab.id)).toEqual(expect.arrayContaining(['agent', 'medlemmer']))
      primeSpaceRoomFeatures(undefined)
      expect(spaceTabFromHash('#agent')).toBe('agent')
      expect(spaceTabFromHash('#members')).toBe('medlemmer')
    })
  })
})
