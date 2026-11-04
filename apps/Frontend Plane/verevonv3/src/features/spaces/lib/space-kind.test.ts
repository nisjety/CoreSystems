import { describe, expect, it } from 'vitest'
import { DM_SPACE_FEATURE, dmParticipantsLabel, isDmKind, spaceKindTraits, withSpaceKindTraits } from './space-kind'

const tr = (no: string, en: string) => `${no}|${en}`

describe('space kind traits (WS12-S3)', () => {
  it('knows the two direct message kinds and nothing else', () => {
    expect(isDmKind('dm')).toBe(true)
    expect(isDmKind('group_dm')).toBe(true)
    for (const other of ['room', 'personal', 'DM', '', undefined, null]) expect(isDmKind(other)).toBe(false)
  })

  it('a DM has a fixed roster, no members tab, no agents and no Ask', () => {
    for (const kind of ['dm', 'group_dm']) {
      expect(spaceKindTraits(kind)).toEqual({ dm: true, fixedRoster: true, showMembers: false, showAgents: false, canAsk: false })
    }
  })

  it('every other kind keeps the full room surface', () => {
    for (const kind of ['room', 'personal', undefined, null, 'something_new']) {
      expect(spaceKindTraits(kind)).toEqual({ dm: false, fixedRoster: false, showMembers: true, showAgents: true, canAsk: true })
    }
  })

  it('withSpaceKindTraits adds the DM feature key to the context features and keeps the rest', () => {
    expect(withSpaceKindTraits(undefined)).toBeUndefined()
    expect(withSpaceKindTraits(null)).toBeUndefined()
    expect(withSpaceKindTraits({ features: { human_posts: true }, space: { kind: 'dm' } })).toEqual({
      human_posts: true,
      [DM_SPACE_FEATURE]: true,
    })
    expect(withSpaceKindTraits({ features: { human_posts: true }, space: { kind: 'room' } })).toEqual({
      human_posts: true,
      [DM_SPACE_FEATURE]: false,
    })
    expect(withSpaceKindTraits({ space: { kind: 'group_dm' } })).toEqual({ [DM_SPACE_FEATURE]: true })
    expect(withSpaceKindTraits({ features: null })).toEqual({ [DM_SPACE_FEATURE]: false })
  })
})

describe('dmParticipantsLabel (WS12-S3)', () => {
  const people = [
    { subject_id: 'me', display_name: 'Caller' },
    { subject_id: 'u-2', display_name: ' Ann ' },
    { subject_id: 'u-3', display_name: 'Bo' },
  ]

  it('names the other people and leaves the viewer out', () => {
    expect(dmParticipantsLabel(people, 'me', tr)).toBe('Ann, Bo')
    expect(dmParticipantsLabel(people.slice(0, 2), 'me', tr)).toBe('Ann')
  })

  it('keeps everyone when the viewer is unknown', () => {
    expect(dmParticipantsLabel(people.slice(0, 2), undefined, tr)).toBe('Caller, Ann')
  })

  it('falls back to the generic label, or the given name, when nobody else has a name', () => {
    expect(dmParticipantsLabel(undefined, 'me', tr)).toBe('Direktemelding|Direct message')
    expect(dmParticipantsLabel([], 'me', tr)).toBe('Direktemelding|Direct message')
    expect(dmParticipantsLabel([{ subject_id: 'u-2', display_name: '  ' }], 'me', tr)).toBe('Direktemelding|Direct message')
    expect(dmParticipantsLabel([{ subject_id: 'me', display_name: 'Caller' }], 'me', tr, ' Ann ')).toBe('Ann')
  })
})
