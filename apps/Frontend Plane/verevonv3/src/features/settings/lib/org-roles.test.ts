import { afterEach, describe, expect, it, vi } from 'vitest'
import { assignableRoleOptions, complianceRoleEnabled } from './org-roles'

const no = (noText: string, _enText: string) => noText
const en = (_noText: string, enText: string) => enText

afterEach(() => vi.unstubAllEnvs())

describe('organisation role pickers (O-21 compliance role)', () => {
  it.each([[undefined], [''], ['1'], ['TRUE'], ['yes']])(
    'off for %j: offers member and admin only, never compliance',
    (value) => {
      if (value !== undefined) vi.stubEnv('VITE_ORG_COMPLIANCE_ROLE', value)
      expect(complianceRoleEnabled()).toBe(false)
      expect(assignableRoleOptions(en).map((option) => option.value)).toEqual(['member', 'admin'])
    },
  )

  it('on for the literal "true": offers compliance after member and admin, with Norwegian and English labels', () => {
    vi.stubEnv('VITE_ORG_COMPLIANCE_ROLE', 'true')
    expect(complianceRoleEnabled()).toBe(true)
    expect(assignableRoleOptions(en)).toEqual([
      { value: 'member', label: 'Member' },
      { value: 'admin', label: 'Admin' },
      { value: 'compliance', label: 'Compliance' },
    ])
    expect(assignableRoleOptions(no).map((option) => option.label)).toEqual([
      'Medlem',
      'Administrator',
      'Etterlevelse (compliance)',
    ])
  })

  it('never offers owner', () => {
    vi.stubEnv('VITE_ORG_COMPLIANCE_ROLE', 'true')
    expect(assignableRoleOptions(en, 'owner').map((option) => option.value)).not.toContain('owner')
  })

  it('keeps compliance for a member who already holds it, even with the switch off', () => {
    expect(assignableRoleOptions(en, 'compliance').map((option) => option.value)).toEqual([
      'member',
      'admin',
      'compliance',
    ])
    expect(assignableRoleOptions(en, 'member').map((option) => option.value)).toEqual(['member', 'admin'])
  })
})
