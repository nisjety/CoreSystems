import { describe, expect, it } from 'vitest'

import { spaceRetrievalDeclineCopy, spaceRetrievalDeclineReason } from './space-retrieval-decline'

const REASONS = [
  'entitlement_off',
  'policy_not_provisioned',
  'membership_required',
  'audience_not_current',
  'role_below_floor',
  'zdr_forbids_effect',
  'space_not_active',
  'not_authorized',
] as const

describe('space retrieval decline (D1-242)', () => {
  it('reads every reason of the closed set from its notice code, and words each in both languages', () => {
    for (const reason of REASONS) {
      expect(spaceRetrievalDeclineReason(`retrieval_declined_${reason}`)).toBe(reason)
      const copy = spaceRetrievalDeclineCopy(reason)
      expect(copy.no.length).toBeGreaterThan(10)
      expect(copy.en.length).toBeGreaterThan(10)
      expect(copy.no).not.toBe(copy.en)
    }
  })

  it('says nothing for another notice or a reason outside the set', () => {
    for (const code of [
      'retrieval_reduced',
      'zdr_dense_unavailable',
      'retrieval_declined_',
      'retrieval_declined_something_new',
      'retrieval_declined_constructor',
      'retrieval_declined___proto__',
      'RETRIEVAL_DECLINED_NOT_AUTHORIZED',
      '',
    ]) {
      expect(spaceRetrievalDeclineReason(code), code).toBeUndefined()
    }
  })
})
