import { describe, expect, it } from 'vitest'

import type { SpaceAgent } from '@/shared/api/spaces-client'
import { AGENT_AVATAR_COLORS } from '../components/SpaceCreateAgentDialog'
import {
  agentAvatarColor,
  agentForName,
  avatarTextColor,
  contrastRatio,
  relativeLuminance,
  validAvatarColor,
} from './agent-avatar'

const agent = (overrides: Partial<SpaceAgent> = {}): SpaceAgent => ({
  subject_id: 'agent-1',
  role: 'editor',
  revision: 1,
  identity_published: true,
  name: 'Lagerhjelpen',
  status: 'active',
  ...overrides,
}) as SpaceAgent

describe('validAvatarColor (WS07-S7)', () => {
  it('accepts exactly #rrggbb in either case and returns it unchanged', () => {
    expect(validAvatarColor('#2563eb')).toBe('#2563eb')
    expect(validAvatarColor('#2563EB')).toBe('#2563EB')
    expect(validAvatarColor('#000000')).toBe('#000000')
  })

  it('rejects every other shape, so a stored value can never reach a style attribute', () => {
    for (const bad of ['red', '#12345', '#1234567', '2563eb', '#12345g', 'url(x)', ' #2563eb', '#2563eb;x', '#fff', '']) {
      expect(validAvatarColor(bad), JSON.stringify(bad)).toBeUndefined()
    }
  })

  it('rejects non-strings', () => {
    for (const bad of [undefined, null, 0, 2563, {}, ['#2563eb']]) {
      expect(validAvatarColor(bad)).toBeUndefined()
    }
  })
})

describe('contrast (WS07-S7)', () => {
  it('relative luminance runs from 0 (black) to 1 (white)', () => {
    expect(relativeLuminance('#000000')).toBeCloseTo(0, 5)
    expect(relativeLuminance('#ffffff')).toBeCloseTo(1, 5)
  })

  it('contrast ratio is symmetric and 21 for black on white', () => {
    expect(contrastRatio('#000000', '#ffffff')).toBeCloseTo(21, 3)
    expect(contrastRatio('#ffffff', '#000000')).toBeCloseTo(21, 3)
    expect(contrastRatio('#2563eb', '#2563eb')).toBeCloseTo(1, 5)
  })

  it('picks white on dark fills and near-black on light fills', () => {
    expect(avatarTextColor('#000000')).toBe('#ffffff')
    expect(avatarTextColor('#7c3aed')).toBe('#ffffff')
    expect(avatarTextColor('#ffffff')).toBe('#111111')
    expect(avatarTextColor('#f0a202')).toBe('#111111')
  })

  it('offers ten colours and every one is a valid #rrggbb', () => {
    expect(AGENT_AVATAR_COLORS).toHaveLength(10)
    for (const colour of AGENT_AVATAR_COLORS) expect(validAvatarColor(colour)).toBe(colour)
  })

  // The glyph is decorative (the Agent chip and the name carry the identity), so the floor is the
  // WCAG non-text/large-glyph threshold of 3:1, which the chosen glyph colour must clear on every swatch.
  it('the glyph colour meets 3:1 against every one of the ten picker colours', () => {
    for (const colour of AGENT_AVATAR_COLORS) {
      const ratio = contrastRatio(colour, avatarTextColor(colour))
      expect(ratio, `${colour} with ${avatarTextColor(colour)}`).toBeGreaterThanOrEqual(3)
    }
  })

  it('the glyph colour is always the better of the two candidates', () => {
    for (const colour of AGENT_AVATAR_COLORS) {
      const chosen = contrastRatio(colour, avatarTextColor(colour))
      expect(chosen).toBeGreaterThanOrEqual(contrastRatio(colour, '#ffffff'))
      expect(chosen).toBeGreaterThanOrEqual(contrastRatio(colour, '#111111'))
    }
  })
})

describe('agentForName (WS07-S7)', () => {
  const first = agent({ subject_id: 'agent-1', name: 'Lagerhjelpen' })
  const second = agent({ subject_id: 'agent-2', name: 'Innkjøpsagent' })

  it('returns the agent when exactly one has that display name', () => {
    expect(agentForName([first, second], 'Innkjøpsagent')).toBe(second)
  })

  it('trims both sides before comparing', () => {
    expect(agentForName([agent({ name: '  Lagerhjelpen ' })], ' Lagerhjelpen')?.subject_id).toBe('agent-1')
  })

  it('returns undefined for two same-named agents rather than guessing', () => {
    const twin = agent({ subject_id: 'agent-3', name: 'Lagerhjelpen' })
    expect(agentForName([first, twin], 'Lagerhjelpen')).toBeUndefined()
  })

  it('returns undefined for no match, an empty roster, and a missing or blank name', () => {
    expect(agentForName([first], 'Ukjent')).toBeUndefined()
    expect(agentForName([], 'Lagerhjelpen')).toBeUndefined()
    expect(agentForName([first], undefined)).toBeUndefined()
    expect(agentForName([first], '   ')).toBeUndefined()
  })

  it('does not match an agent that has no name', () => {
    expect(agentForName([agent({ name: undefined })], 'Lagerhjelpen')).toBeUndefined()
  })
})

describe('agentAvatarColor (WS07-S7)', () => {
  it('paints the validated colour of a published agent', () => {
    expect(agentAvatarColor(agent({ avatar_color: '#2563eb' }))).toBe('#2563eb')
  })

  it('is neutral when the identity is unpublished, even if a colour is present', () => {
    expect(agentAvatarColor(agent({ identity_published: false, avatar_color: '#2563eb' }))).toBeUndefined()
  })

  it('is neutral for an invalid colour, a missing colour and an undefined agent', () => {
    expect(agentAvatarColor(agent({ avatar_color: 'red' }))).toBeUndefined()
    expect(agentAvatarColor(agent({ avatar_color: 'url(x)' }))).toBeUndefined()
    expect(agentAvatarColor(agent())).toBeUndefined()
    expect(agentAvatarColor(undefined)).toBeUndefined()
  })
})
