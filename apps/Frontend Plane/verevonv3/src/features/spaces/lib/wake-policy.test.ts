import { describe, expect, it } from 'vitest'

import type { SpaceAgent, SpaceRosterMember } from '@/shared/api/spaces-client'
import {
  allowlistCandidates,
  draftProblem,
  draftsEqual,
  respondToOf,
  respondToSubjectsOf,
  WAKE_ALLOWLIST_MAX,
  whyNoAnswerHint,
  type WakePolicyDraft,
} from './wake-policy'

const english = (_norwegian: string, text: string) => text

const draft = (mode: WakePolicyDraft['mode'], subjects: string[] = []): WakePolicyDraft => ({ mode, subjects })

const member = (
  subjectId: string,
  role: SpaceRosterMember['role'],
  subjectType: SpaceRosterMember['subject_type'] = 'user',
): SpaceRosterMember => ({
  subject_type: subjectType,
  subject_id: subjectId,
  role,
  revision: 1,
  display_name: subjectId,
})

describe('respondToOf (WS08-S8)', () => {
  it('reads the stored mode', () => {
    expect(respondToOf({ respond_to: 'managers' })).toBe('managers')
    expect(respondToOf({ respond_to: 'allowlist' })).toBe('allowlist')
    expect(respondToOf({ respond_to: 'members' })).toBe('members')
  })

  it('defaults to members when the binding predates the field or carries a retired value', () => {
    expect(respondToOf({})).toBe('members')
    expect(respondToOf({ respond_to: undefined })).toBe('members')
    expect(respondToOf({ respond_to: 'owners_only' as unknown as SpaceAgent['respond_to'] })).toBe('members')
  })
})

describe('respondToSubjectsOf (WS08-S8)', () => {
  it('lists the chosen people only in allowlist mode, and none when the list is absent', () => {
    expect(respondToSubjectsOf({ respond_to: 'allowlist', respond_to_subjects: ['user-1', 'user-2'] })).toEqual([
      'user-1',
      'user-2',
    ])
    expect(respondToSubjectsOf({ respond_to: 'managers', respond_to_subjects: ['user-1'] })).toEqual([])
    expect(respondToSubjectsOf({ respond_to: 'members', respond_to_subjects: ['user-1'] })).toEqual([])
    expect(respondToSubjectsOf({ respond_to: 'allowlist' })).toEqual([])
  })
})

describe('draftsEqual (WS08-S8)', () => {
  it('compares an allowlist as a set, so the order of the chosen people does not matter', () => {
    expect(draftsEqual(draft('allowlist', ['user-1', 'user-2']), draft('allowlist', ['user-2', 'user-1']))).toBe(true)
    expect(draftsEqual(draft('allowlist', ['user-1', 'user-2']), draft('allowlist', ['user-1', 'user-3']))).toBe(false)
    expect(draftsEqual(draft('allowlist', ['user-1', 'user-2']), draft('allowlist', ['user-1']))).toBe(false)
  })

  it('compares the modes, and ignores the list outside allowlist mode', () => {
    expect(draftsEqual(draft('managers'), draft('members'))).toBe(false)
    expect(draftsEqual(draft('members', ['user-1']), draft('members'))).toBe(true)
  })
})

describe('draftProblem (WS08-S8)', () => {
  it('flags an empty allowlist and one over the cap, and never flags another mode', () => {
    expect(WAKE_ALLOWLIST_MAX).toBe(50)
    expect(draftProblem(draft('allowlist'))).toBe('allowlist_empty')
    const atCap = Array.from({ length: WAKE_ALLOWLIST_MAX }, (_, index) => `user-${index}`)
    expect(draftProblem(draft('allowlist', atCap))).toBeNull()
    expect(draftProblem(draft('allowlist', [...atCap, 'user-extra']))).toBe('allowlist_too_long')
    expect(draftProblem(draft('members'))).toBeNull()
    expect(draftProblem(draft('managers'))).toBeNull()
  })
})

describe('whyNoAnswerHint (WS08-S8)', () => {
  it('is null for members, and gives a reason for the two modes that can refuse someone', () => {
    expect(whyNoAnswerHint('members', english)).toBeNull()
    expect(whyNoAnswerHint('managers', english)).toContain('editor')
    expect(whyNoAnswerHint('allowlist', english)).toContain('list')
  })
})

describe('allowlistCandidates (WS08-S8)', () => {
  it('drops viewers and agents: only people who can post can be named', () => {
    const roster = [
      member('user-1', 'owner'),
      member('user-2', 'editor'),
      member('user-3', 'viewer'),
      member('user-4', 'manager'),
      // An agent is a service subject in the roster, even when its role is editor.
      member('agent-1', 'editor', 'service'),
    ]
    expect(allowlistCandidates(roster).map((entry) => entry.subject_id)).toEqual(['user-1', 'user-2', 'user-4'])
  })
})
