import { describe, expect, it } from 'vitest'

import { pickDefaultSpace } from './SpacesIndexPage'
import type { SpaceSummary } from '@/shared/api/spaces-client'

const space = (over: Partial<SpaceSummary> = {}): SpaceSummary => ({
  space_ref: 'space_1',
  name: 'Rom',
  kind: 'shared',
  lifecycle: 'active',
  ...over,
})

describe('pickDefaultSpace', () => {
  it('lands in the organization channel first, Slack-shaped', () => {
    const personal = space({ space_ref: 'space_personal', kind: 'personal' })
    const orgRoom = space({
      space_ref: 'space_org',
      kind: 'room',
      is_organization_room: true,
    })
    // Order deliberately puts the personal one first: the rule must be the
    // room's identity, not list position.
    expect(pickDefaultSpace([personal, orgRoom])?.space_ref).toBe('space_org')
  })

  it('prefers the organization channel over another active channel', () => {
    const otherRoom = space({ space_ref: 'space_room', kind: 'room' })
    const orgRoom = space({
      space_ref: 'space_org',
      kind: 'room',
      is_organization_room: true,
    })
    expect(pickDefaultSpace([otherRoom, orgRoom])?.space_ref).toBe('space_org')
  })

  it('never lands in a channel that Control has not activated yet', () => {
    // A pending_registration channel would greet the user with a refusal —
    // their own active room is the better landing while Control catches up.
    const pendingOrgRoom = space({
      space_ref: 'space_org',
      kind: 'room',
      is_organization_room: true,
      lifecycle: 'pending_registration',
    })
    const personal = space({ space_ref: 'space_personal', kind: 'personal' })
    expect(pickDefaultSpace([pendingOrgRoom, personal])?.space_ref).toBe('space_personal')
  })

  it('falls back to an active channel of unknown provenance before the personal room', () => {
    // The Control-outage fallback listing omits is_organization_room entirely;
    // an active channel still beats the personal room in that state.
    const room = space({ space_ref: 'space_room', kind: 'room' })
    const personal = space({ space_ref: 'space_personal', kind: 'personal' })
    expect(pickDefaultSpace([personal, room])?.space_ref).toBe('space_room')
  })

  it('still opens a non-active personal space rather than skipping to a shared one', () => {
    // An archived or suspended Space must stay openable so its owner can see
    // WHY it is unavailable. Silently redirecting to a different room would
    // hide a lifecycle state the user needs to act on.
    const archivedPersonal = space({
      space_ref: 'space_personal',
      kind: 'personal',
      lifecycle: 'archived',
    })
    const activeShared = space({ space_ref: 'space_shared', kind: 'shared' })
    expect(pickDefaultSpace([archivedPersonal, activeShared])?.space_ref).toBe('space_personal')
  })

  it('falls back to an active shared space when there is no personal one', () => {
    const suspended = space({ space_ref: 'space_old', lifecycle: 'suspended' })
    const active = space({ space_ref: 'space_new', lifecycle: 'active' })
    expect(pickDefaultSpace([suspended, active])?.space_ref).toBe('space_new')
  })

  it('opens the only space there is even when nothing is active', () => {
    const suspended = space({ space_ref: 'space_only', lifecycle: 'suspended' })
    expect(pickDefaultSpace([suspended])?.space_ref).toBe('space_only')
  })

  it('resolves nothing for an empty list rather than inventing a target', () => {
    // Distinct from a failed read: the page provisions the organization room
    // and reports that state, instead of navigating somewhere.
    expect(pickDefaultSpace([])).toBeUndefined()
  })
})
