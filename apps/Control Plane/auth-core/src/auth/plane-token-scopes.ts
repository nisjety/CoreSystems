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

export function planeScopesForRole(
  role: string | null | undefined,
  audience: InteractiveScopeAudience = 'data-plane',
): string[] {
  const normalizedRole = (role ?? '').trim().toLowerCase();
  if (audience === 'inference-core') {
    return ['inference:invoke'];
  }
  if (audience === 'capability-core') {
    return normalizedRole === 'owner' || normalizedRole === 'admin'
      ? ['capability:read', 'capability:write']
      : ['capability:read'];
  }
  if (
    audience === 'session-core' ||
    audience === 'execution-core' ||
    audience === 'cost-core' ||
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
