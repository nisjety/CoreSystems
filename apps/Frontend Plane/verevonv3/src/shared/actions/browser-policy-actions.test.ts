import { describe, expect, it } from 'vitest'
import { getActionDescriptor } from '@/shared/actions/action-registry'
import { MODEL_ACCESS } from '@/shared/actions/model-access'

describe('browser policy actions (E3.2-g1)', () => {
  it('registers the read as low risk and the save as high risk, approval-gated, owned by Control', () => {
    expect(getActionDescriptor('browser.get_org_policy')).toMatchObject({
      ownerPlane: 'control',
      risk: 'low',
      requiresApproval: false,
    })
    expect(getActionDescriptor('browser.set_org_policy')).toMatchObject({
      ownerPlane: 'control',
      risk: 'high',
      requiresApproval: true,
    })
  })

  it('keeps both closed to the model', () => {
    expect(MODEL_ACCESS['browser.get_org_policy'].access).toBe('none')
    expect(MODEL_ACCESS['browser.set_org_policy'].access).toBe('none')
  })

  it('takes the organization and the revision, and refuses any document field org-core does not read', () => {
    const save = getActionDescriptor('browser.set_org_policy')?.inputSchema
    expect(save?.safeParse({ orgId: 'org-1', ifMatch: 3, document: { caps: { 'browser.act.input': 'disabled' } } }).success).toBe(true)
    expect(save?.safeParse({ ifMatch: 3, document: {} }).success).toBe(false)
    expect(save?.safeParse({ orgId: 'org-1', document: {} }).success).toBe(false)
    expect(save?.safeParse({ orgId: 'org-1', ifMatch: -1, document: {} }).success).toBe(false)
    expect(save?.safeParse({ orgId: 'org-1', ifMatch: 3, document: { org_id: 'org-2' } }).success).toBe(false)
    expect(save?.safeParse({ orgId: 'org-1', ifMatch: 3, document: { domains: { allow: [], extra: [] } } }).success).toBe(false)
    expect(save?.safeParse({ orgId: 'org-1', ifMatch: 3, document: {}, role: 'owner' }).success).toBe(false)
  })

  it('takes no organization from the read beyond the id it names', () => {
    const read = getActionDescriptor('browser.get_org_policy')?.inputSchema
    expect(read?.safeParse({ orgId: 'org-1' }).success).toBe(true)
    expect(read?.safeParse({}).success).toBe(false)
    expect(read?.safeParse({ orgId: 'org-1', can_manage: true }).success).toBe(false)
  })
})
