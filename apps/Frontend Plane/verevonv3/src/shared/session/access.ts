import type { SessionState } from './session-store'

function roleMatches(value: string | null | undefined, allowed: string[]): boolean {
  return (value ?? '')
    .split(',')
    .map((role) => role.trim().toLowerCase())
    .some((role) => allowed.includes(role))
}

export function hasWorkspaceAdminAccess(session: SessionState): boolean {
  if (session.status !== 'authenticated') return false

  return (
    roleMatches(session.user?.role, ['admin', 'superadmin']) ||
    roleMatches(session.activeOrg?.role, ['owner', 'admin']) ||
    session.permissions.some((permission) => (
      permission === 'org:members:update' ||
      permission === 'admin:users:read' ||
      permission === 'admin:users:update'
    ))
  )
}

export function shouldShowWorkspaceAdminNavigation(session: SessionState): boolean {
  return session.status !== 'authenticated' || hasWorkspaceAdminAccess(session)
}

/**
 * Platform super-admin: a top-level Better Auth `admin`/`superadmin` role,
 * NOT merely an org owner/admin. Gates cross-org surfaces (e.g. the all-users
 * directory) — an org owner can manage their own org's members but must not
 * see other tenants' users. The gateway + auth-core enforce the same role;
 * this only controls whether the navigation is offered.
 */
export function hasPlatformAdminAccess(session: SessionState): boolean {
  if (session.status !== 'authenticated') return false
  return roleMatches(session.user?.role, ['admin', 'superadmin'])
}
