// Better Auth access control for the organisation plugin (O-21 compliance role).
//
// Kept apart from organization-roles.ts because better-auth's access modules are
// ESM-only: this file is loaded by auth.ts alone, and the pure role list (which the
// membership authority and the scope table read) stays loadable by every spec.
import { createAccessControl } from 'better-auth/plugins/access';
import {
  adminAc,
  defaultStatements,
  memberAc,
  ownerAc,
} from 'better-auth/plugins/organization/access';
import {
  COMPLIANCE_ORGANIZATION_STATEMENTS,
  COMPLIANCE_ROLE,
  complianceRoleEnabled,
} from './organization-roles';

const organizationAc = createAccessControl(defaultStatements);

const complianceOrganizationRole = organizationAc.newRole(
  COMPLIANCE_ORGANIZATION_STATEMENTS,
);

/**
 * Options spread into the organization() plugin. Off, nothing is added, so the
 * plugin keeps its built-in owner/admin/member roles exactly as before. On, the
 * compliance role is registered next to them so Better Auth accepts it on an
 * invitation and a role change. Passing `roles` REPLACES the plugin's default role
 * table for permission checks, so the three built-in roles are listed again
 * unchanged.
 */
export function organizationRoleOptions(
  env: Readonly<Record<string, string | undefined>> = process.env,
) {
  if (!complianceRoleEnabled(env)) {
    return {};
  }
  return {
    ac: organizationAc,
    roles: {
      owner: ownerAc,
      admin: adminAc,
      member: memberAc,
      [COMPLIANCE_ROLE]: complianceOrganizationRole,
    },
  };
}
