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
  it('prefers an active personal space over an active shared one', () => {
    const shared = space({ space_ref: 'space_shared', kind: 'shared' })
    const personal = space({ space_ref: 'space_personal', kind: 'personal' })
    // Order deliberately puts the shared one first: the rule must be the kind,
    // not list position.
    expect(pickDefaultSpace([shared, personal])?.space_ref).toBe('space_personal')
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
    // Distinct from a failed read: the page renders "no rooms yet" and names
    // provisioning as the missing step, instead of navigating somewhere.
    expect(pickDefaultSpace([])).toBeUndefined()
  })
})
