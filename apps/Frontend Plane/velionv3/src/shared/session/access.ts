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
