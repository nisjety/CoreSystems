// The organisation role list and the compliance role (O-21).
//
// The four base tiers (owner, admin, member, viewer) are always valid. A fifth,
// `compliance`, is valid only while ORG_COMPLIANCE_ROLE_ENABLED is the literal
// "true" (default off, read at call time so a deployment's value is what counts and
// a test can set it). A compliance member reads audit and the legal-hold lists, may
// place and release holds in org-core, and runs no agents: it holds no Model Plane
// scope beyond read (see plane-token-scopes.ts).
import { HttpException, HttpStatus } from '@nestjs/common';
import { z } from 'zod';

export const COMPLIANCE_ROLE = 'compliance';

export const BASE_ORGANIZATION_ROLES = [
  'owner',
  'admin',
  'member',
  'viewer',
] as const;

type Env = Readonly<Record<string, string | undefined>>;

/** Only the exact literal "true" turns the compliance role on. */
export function complianceRoleEnabled(env: Env = process.env): boolean {
  return env.ORG_COMPLIANCE_ROLE_ENABLED === 'true';
}

/** The canonical organisation roles for the current switch value. */
export function canonicalOrganizationRoles(
  env: Env = process.env,
): ReadonlySet<string> {
  return new Set<string>(
    complianceRoleEnabled(env)
      ? [...BASE_ORGANIZATION_ROLES, COMPLIANCE_ROLE]
      : BASE_ORGANIZATION_ROLES,
  );
}

export function isCanonicalOrganizationRole(
  role: string,
  env: Env = process.env,
): boolean {
  return canonicalOrganizationRoles(env).has(role);
}

/**
 * The role value of an organisation API record or an invitation: owner, admin or
 * member, or compliance while the switch is on. A zod 4 schema (and `z.lazy`'s
 * getter) is built once, so the switch is read once, when this module loads: that
 * is the deployment's value, which is fixed for the process. Off, this is exactly
 * the original `z.enum(['owner', 'admin', 'member'])`: a compliance value, or any
 * other unknown one, gets that enum's own error and message, byte for byte, and
 * the generated OpenAPI lists the same three values.
 */
const ORGANIZATION_ROLE_SCHEMA_WITH_COMPLIANCE = z.enum([
  'owner',
  'admin',
  'member',
  COMPLIANCE_ROLE,
]);

// Typed as the wider enum so every record and handler type admits compliance; off,
// the value is the original three-role enum, which refuses it at runtime.
export const OrganizationRoleSchema: typeof ORGANIZATION_ROLE_SCHEMA_WITH_COMPLIANCE =
  complianceRoleEnabled()
    ? ORGANIZATION_ROLE_SCHEMA_WITH_COMPLIANCE
    : (z.enum([
        'owner',
        'admin',
        'member',
      ]) as unknown as typeof ORGANIZATION_ROLE_SCHEMA_WITH_COMPLIANCE);

/**
 * What the compliance role may do in Better Auth's organisation plugin: nothing
 * about the organisation, its members, invitations or teams, only the member-level
 * read of the access-control table. It can be invited and held as a member and can
 * change nothing; the legal-hold and audit authority lives in org-core, which reads
 * the role name from the membership.
 */
export const COMPLIANCE_ORGANIZATION_STATEMENTS = {
  organization: [],
  member: [],
  invitation: [],
  team: [],
  ac: ['read'],
} as const;

/**
 * True for a member whose organisation role is compliance while the switch is on.
 * That role reads audit and the legal-hold lists and may place and release holds,
 * and runs no agents: every mint of an agent-running bearer for it is refused.
 */
export function isComplianceMember(
  role: string | null | undefined,
  env: Env = process.env,
): boolean {
  return (
    complianceRoleEnabled(env) &&
    (role ?? '').trim().toLowerCase() === COMPLIANCE_ROLE
  );
}

/** The typed 403 for an agent-running mint by a compliance member. */
export function complianceCannotRunAgents(): HttpException {
  return new HttpException(
    {
      error: {
        code: 'compliance_role_cannot_run_agents',
        message:
          'The compliance role reads audit and legal holds and cannot run agents',
        details: { reason: 'compliance_role' },
      },
    },
    HttpStatus.FORBIDDEN,
  );
}
