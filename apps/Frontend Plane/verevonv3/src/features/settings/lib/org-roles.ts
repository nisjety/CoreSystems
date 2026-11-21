// The organisation roles an admin may assign from the member list and the
// invitation form (O-21). owner is never assignable here. `compliance` is offered
// only while VITE_ORG_COMPLIANCE_ROLE is the literal "true" (default off), the
// build-time twin of the Control Plane's ORG_COMPLIANCE_ROLE_ENABLED: the server
// refuses the role when its own switch is off, so the picker never offers a choice
// that can only fail.
import type { MembershipRole } from '@/shared/api/membership-client'

/** Only the exact literal "true" turns the compliance role on in the picker. */
export function complianceRoleEnabled(): boolean {
  return import.meta.env.VITE_ORG_COMPLIANCE_ROLE === 'true'
}

export type RoleOption = { value: MembershipRole; label: string }

type Translate = (no: string, en: string) => string

/**
 * The roles the pickers list. `current` is the role a listed member already has: a
 * member who already holds compliance keeps that option even when the switch is off
 * here, so the select shows the truth instead of a blank.
 */
export function assignableRoleOptions(tr: Translate, current?: string): RoleOption[] {
  const options: RoleOption[] = [
    { value: 'member', label: tr('Medlem', 'Member') },
    { value: 'admin', label: tr('Administrator', 'Admin') },
  ]
  if (complianceRoleEnabled() || current === 'compliance') {
    options.push({ value: 'compliance', label: tr('Etterlevelse (compliance)', 'Compliance') })
  }
  return options
}
