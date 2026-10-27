import { afterEach, describe, expect, it } from 'vitest'

import {
  DEFAULT_SPACE_COMPOSER_MODE,
  resolveSpaceComposerMode,
  SPACE_COMPOSER_MODES,
  visibleSpaceComposerModes,
  type SpaceComposerModeDefinition,
} from './space-composer-registry'

// The registry is a plain array, so a test can register a gated mode and take it out again.
const added: unknown[] = []
afterEach(() => {
  for (const entry of added.splice(0)) {
    const at = (SPACE_COMPOSER_MODES as readonly unknown[]).indexOf(entry)
    if (at >= 0) (SPACE_COMPOSER_MODES as unknown as unknown[]).splice(at, 1)
  }
})

function register(entry: SpaceComposerModeDefinition) {
  ;(SPACE_COMPOSER_MODES as unknown as SpaceComposerModeDefinition[]).push(entry)
  added.push(entry)
}

describe('space composer registry (P-SPA2, WS01-S6)', () => {
  it('lists post then ask, and ask stays the default', () => {
    expect(SPACE_COMPOSER_MODES.map((mode) => mode.id)).toEqual(['post', 'ask'])
    expect(DEFAULT_SPACE_COMPOSER_MODE).toBe('ask')
  })

  it('offers every registered mode that has no enabled switch', () => {
    expect(visibleSpaceComposerModes().map((mode) => mode.id)).toEqual(['post', 'ask'])
  })

  describe('resolveSpaceComposerMode', () => {
    it('returns a registered visible id unchanged', () => {
      expect(resolveSpaceComposerMode('post')).toBe('post')
      expect(resolveSpaceComposerMode('ask')).toBe('ask')
    })

    it('falls back to the default for unknown or undefined ids', () => {
      expect(resolveSpaceComposerMode(undefined)).toBe('ask')
      expect(resolveSpaceComposerMode('')).toBe('ask')
      expect(resolveSpaceComposerMode('nope')).toBe('ask')
      expect(resolveSpaceComposerMode('__proto__')).toBe('ask')
    })
  })

  describe('a mode whose enabled() is false', () => {
    it('is skipped by visibleSpaceComposerModes and resolves to the default', () => {
      register({ id: 'draft', labelNo: 'Utkast', labelEn: 'Draft', enabled: () => false })
      expect(visibleSpaceComposerModes().map((mode) => mode.id)).toEqual(['post', 'ask'])
      expect(resolveSpaceComposerMode('draft')).toBe('ask')
    })

    it('is offered while enabled() says yes', () => {
      register({ id: 'draft', labelNo: 'Utkast', labelEn: 'Draft', enabled: () => true })
      expect(visibleSpaceComposerModes().map((mode) => mode.id)).toEqual(['post', 'ask', 'draft'])
      expect(resolveSpaceComposerMode('draft')).toBe('draft')
    })
  })
})
