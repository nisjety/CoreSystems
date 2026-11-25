import { COMPLIANCE_ROLE, complianceRoleEnabled } from './organization-roles';

const MEMBER_SCOPES = [
  'data:read',
  'documents:read',
  'documents:write',
  'wiki.read',
  'wiki.write',
] as const;

const ADMIN_SCOPES = [
  'admin',
  'data:admin',
  'data:orchestrate',
  'data:quality:admin',
  'org:data:read_all',
  'org:data:write_all',
  'wiki.approve',
  'wiki.maintenance.write',
] as const;

// A compliance member (O-21, ORG_COMPLIANCE_ROLE_ENABLED) reads and runs no
// agents: Data and wiki reads only, no write, no inference, no cost read. Model
// Plane services get no scope for it beyond capability:read.
const COMPLIANCE_READ_SCOPES = [
  'data:read',
  'documents:read',
  'wiki.read',
] as const;

export type InteractiveScopeAudience =
  | 'data-plane'
  | 'quarry'
  | 'ingestion'
  | 'control-plane'
  | 'application-plane'
  | 'session-core'
  | 'inference-core'
  | 'execution-core'
  | 'cost-core'
  | 'capability-core'
  | 'letta-bridge'
  | 'browser-broker'
  | 'sandbox-manager'
  | 'bridge-core';

/**
 * Is the org-admin marker on for capability-core? Read per call from the environment so an
 * operator flip needs no rebuild (flag `PLANE_TOKEN_CAPABILITY_ADMIN_SCOPE`, strict literal
 * `true`, default off). With it on, owners and admins also carry `capability:admin`, which
 * capability-core's verifiedIsAdmin reads as the admin marker for org-scope writes. Members and
 * viewers never get it: it only adds a marker for a role that already holds `capability:write`.
 */
export function capabilityAdminScopeEnabled(
  environment: Record<string, string | undefined> = process.env,
): boolean {
  return environment.PLANE_TOKEN_CAPABILITY_ADMIN_SCOPE === 'true';
}

export function planeScopesForRole(
  role: string | null | undefined,
  audience: InteractiveScopeAudience = 'data-plane',
): string[] {
  const normalizedRole = (role ?? '').trim().toLowerCase();
  const isCompliance =
    normalizedRole === COMPLIANCE_ROLE && complianceRoleEnabled();
  if (audience === 'inference-core') {
    return isCompliance ? [] : ['inference:invoke'];
  }
  if (audience === 'capability-core') {
    if (normalizedRole === 'owner' || normalizedRole === 'admin') {
      return capabilityAdminScopeEnabled()
        ? ['capability:read', 'capability:write', 'capability:admin']
        : ['capability:read', 'capability:write'];
    }
    return ['capability:read'];
  }
  if (audience === 'cost-core') {
    return normalizedRole === 'owner' || normalizedRole === 'admin'
      ? ['cost:org:read']
      : [];
  }
  if (
    audience === 'session-core' ||
    audience === 'execution-core' ||
    audience === 'letta-bridge' ||
    audience === 'browser-broker' ||
    audience === 'sandbox-manager' ||
    audience === 'bridge-core'
  ) {
    return [];
  }

  if (normalizedRole === 'owner' || normalizedRole === 'admin') {
    return [...MEMBER_SCOPES, ...ADMIN_SCOPES];
  }
  if (isCompliance) {
    return [...COMPLIANCE_READ_SCOPES];
  }
  return [...MEMBER_SCOPES];
}

export function modelGatewayScopesForRole(
  role: string | null | undefined,
): string[] {
  const normalizedRole = (role ?? '').trim().toLowerCase();
  return normalizedRole === 'owner' || normalizedRole === 'admin'
    ? ['admin']
    : [];
}
