import { describe, expect, it } from 'vitest'

import { buildModelContextPack } from './context-pack'

describe('buildModelContextPack', () => {
  it('advertises the same model-eligible actions regardless of route', () => {
    // buildModelContextPack defers entirely to isModelExecutableAction (see
    // context-pack.ts) -- there is no separate route-based restriction, so
    // this must equal the current global eligible set (model-eligibility.ts),
    // not an empty array. Asserted as an exact set so a further admission
    // doesn't silently pass this test without being revisited here too.
    const pack = buildModelContextPack({
      route: '/support/inbox',
      visibleItems: [],
    })

    expect(pack.availableActions).toEqual([
      'inbox.follow_conversation',
      'inbox.set_csat_preference',
      'inbox.review_ai_action',
      'tickets.create',
      'org.mark_exported',
      'org.acknowledge_deletion',
      'chat.save_thread_snapshot',
      'chat.submit_feedback',
    ])
    // Ineligible, inbox-adjacent actions must still not leak in.
    expect(pack.availableActions).not.toContain('inbox.claim_draft_lease')
    expect(pack.availableActions).not.toContain('inbox.save_draft')
    expect(pack.currentView).toBe('support.inbox')
  })
})
