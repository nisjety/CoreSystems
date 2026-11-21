import {
  BASE_ORGANIZATION_ROLES,
  COMPLIANCE_ORGANIZATION_STATEMENTS,
  COMPLIANCE_ROLE,
  OrganizationRoleSchema,
  canonicalOrganizationRoles,
  complianceRoleEnabled,
  isCanonicalOrganizationRole,
  isComplianceMember,
} from './organization-roles';
import { z } from 'zod';

// O-21: the compliance role, behind ORG_COMPLIANCE_ROLE_ENABLED (default off,
// only the literal "true" turns it on).
describe('organization compliance role', () => {
  const original = process.env.ORG_COMPLIANCE_ROLE_ENABLED;

  afterEach(() => {
    if (original === undefined) {
      delete process.env.ORG_COMPLIANCE_ROLE_ENABLED;
    } else {
      process.env.ORG_COMPLIANCE_ROLE_ENABLED = original;
    }
  });

  it('reads the switch as the exact literal true and defaults off', () => {
    expect(complianceRoleEnabled({})).toBe(false);
    for (const value of ['', '1', 'TRUE', 'True', 'yes', ' true']) {
      expect(
        complianceRoleEnabled({ ORG_COMPLIANCE_ROLE_ENABLED: value }),
      ).toBe(false);
    }
    expect(complianceRoleEnabled({ ORG_COMPLIANCE_ROLE_ENABLED: 'true' })).toBe(
      true,
    );
  });

  it('refuses compliance as a role value while the switch is off', () => {
    const roles = canonicalOrganizationRoles({});
    expect([...roles].sort()).toEqual([...BASE_ORGANIZATION_ROLES].sort());
    expect(isCanonicalOrganizationRole(COMPLIANCE_ROLE, {})).toBe(false);
    for (const role of BASE_ORGANIZATION_ROLES) {
      expect(isCanonicalOrganizationRole(role, {})).toBe(true);
    }
  });

  it('accepts compliance, and nothing else beyond the base tiers, with the switch on', () => {
    const env = { ORG_COMPLIANCE_ROLE_ENABLED: 'true' };
    expect(isCanonicalOrganizationRole(COMPLIANCE_ROLE, env)).toBe(true);
    for (const role of BASE_ORGANIZATION_ROLES) {
      expect(isCanonicalOrganizationRole(role, env)).toBe(true);
    }
    for (const role of ['auditor', 'superadmin', '', 'Compliance']) {
      expect(isCanonicalOrganizationRole(role, env)).toBe(false);
    }
  });

  it('gives compliance no organisation, member, invitation or team permission', () => {
    expect(COMPLIANCE_ORGANIZATION_STATEMENTS).toEqual({
      organization: [],
      member: [],
      invitation: [],
      team: [],
      ac: ['read'],
    });
  });

  // The schema is built when the module loads, so each case loads a fresh copy
  // under the switch value it checks.
  const loadSchema = (value: string | undefined) => {
    if (value === undefined) delete process.env.ORG_COMPLIANCE_ROLE_ENABLED;
    else process.env.ORG_COMPLIANCE_ROLE_ENABLED = value;
    let schema!: typeof OrganizationRoleSchema;
    jest.isolateModules(() => {
      schema = jest.requireActual<typeof import('./organization-roles')>(
        './organization-roles',
      ).OrganizationRoleSchema;
    });
    return schema;
  };

  it('validates an invitation or record role against the switch', () => {
    const off = loadSchema(undefined);
    expect(off.safeParse('member').success).toBe(true);
    expect(off.safeParse(COMPLIANCE_ROLE).success).toBe(false);
    const on = loadSchema('true');
    expect(on.safeParse(COMPLIANCE_ROLE).success).toBe(true);
    expect(on.safeParse('auditor').success).toBe(false);
    expect(loadSchema('1').safeParse(COMPLIANCE_ROLE).success).toBe(false);
  });

  it('off, is exactly the original enum: same error issue, same options', () => {
    const schema = loadSchema(undefined);
    const original = z.enum(['owner', 'admin', 'member']);
    for (const bad of [COMPLIANCE_ROLE, 'auditor', '', 7]) {
      const got = schema.safeParse(bad);
      const want = original.safeParse(bad);
      expect(got.success).toBe(false);
      expect(want.success).toBe(false);
      expect(JSON.stringify(got.error?.issues)).toBe(
        JSON.stringify(want.error?.issues),
      );
      expect(got.error?.message).not.toContain('compliance');
    }
    for (const good of ['owner', 'admin', 'member']) {
      expect(schema.safeParse(good).success).toBe(true);
    }
  });

  it('on, lists compliance among the accepted values', () => {
    const result = loadSchema('true').safeParse('auditor');
    expect(result.success).toBe(false);
    expect(JSON.stringify(result.error?.issues)).toContain('compliance');
  });

  it('isComplianceMember is true only for the compliance role with the switch on', () => {
    const on = { ORG_COMPLIANCE_ROLE_ENABLED: 'true' };
    expect(isComplianceMember('compliance', on)).toBe(true);
    expect(isComplianceMember(' Compliance ', on)).toBe(true);
    expect(isComplianceMember('compliance', {})).toBe(false);
    expect(
      isComplianceMember('compliance', { ORG_COMPLIANCE_ROLE_ENABLED: '1' }),
    ).toBe(false);
    for (const role of [
      'owner',
      'admin',
      'member',
      'viewer',
      '',
      null,
      undefined,
    ]) {
      expect(isComplianceMember(role, on)).toBe(false);
    }
  });
});
