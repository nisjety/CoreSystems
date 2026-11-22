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
 * D21i: the organization's compliance role (Control's fifth canonical org role). It views, places and lifts legal
 * holds and nothing else an admin does.
 */
export function hasComplianceRole(session: SessionState): boolean {
  return session.status === 'authenticated' && roleMatches(session.activeOrg?.role, ['compliance'])
}

/**
 * Who may open workspace settings at all: an owner or admin, or a compliance member (owner decision after D21i).
 * What a compliance member sees there is narrowed by {@link isComplianceOnlySettingsViewer}.
 */
export function hasSettingsAccess(session: SessionState): boolean {
  return hasWorkspaceAdminAccess(session) || hasComplianceRole(session)
}

/** A compliance member with no admin access: workspace settings show them the Trust Center's legal holds only. */
export function isComplianceOnlySettingsViewer(session: SessionState): boolean {
  return hasComplianceRole(session) && !hasWorkspaceAdminAccess(session)
}

/** The Settings entry in the navigation: shown while the session is unknown, and to anyone who may open settings. */
export function shouldShowSettingsNavigation(session: SessionState): boolean {
  return session.status !== 'authenticated' || hasSettingsAccess(session)
}

/** The only settings section a compliance-only viewer may open. */
export const COMPLIANCE_SETTINGS_SECTION = 'trust'

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
