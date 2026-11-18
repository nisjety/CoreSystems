import { describe, expect, expectTypeOf, it } from 'vitest'

import type { SpaceMembership } from './core'

describe('SpaceMembership kind (WS01 Control vocabulary)', () => {
  it('takes the room kind Control emits', () => {
    const membership: SpaceMembership = {
      space_ref: 'room-1',
      org_id: 'org-1',
      subject_id: 'user-1',
      kind: 'room',
      role: 'editor',
      revisions: { authority: 1, membership: 1, privacy: 1, recipient_audience: 1, entitlement: 1 },
    }
    expect(membership.kind).toBe('room')
    // The kinds Control names are literal members of the union, not swallowed by its open tail. A union that only
    // says `string` has no literal 'room' to compare against, so this type check fails until the names are there.
    expectTypeOf<Extract<SpaceMembership['kind'], 'room'>>().toEqualTypeOf<'room'>()
    expectTypeOf<Extract<SpaceMembership['kind'], 'personal'>>().toEqualTypeOf<'personal'>()
  })
})
