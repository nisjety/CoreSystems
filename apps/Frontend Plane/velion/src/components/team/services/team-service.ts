/**
 * Team Service
 *
 * Wraps org-service member operations with role-aware filtering and
 * caching-friendly result shapes for the team management UI.
 */
import { orgService } from '@/lib/services/org-service'
import type { OrgMember } from '@/lib/services/org-service'

export type MemberRole = 'owner' | 'admin' | 'member' | 'viewer'

export interface TeamMemberDetail extends OrgMember {
  displayName: string
  avatarUrl?: string
  email: string
  lastActive?: string
  // Derived
  isAdmin: boolean
}

class TeamServiceAPI {
  async getMembers(orgId: string): Promise<TeamMemberDetail[]> {
    const members = await orgService.getOrganizationMembers(orgId)
    return members.map(m => this.enrich(m))
  }

  async inviteMember(
    orgId: string,
    email: string,
    role: MemberRole,
  ): Promise<{ invitation_id: string; status: string }> {
    return orgService.inviteMember(orgId, { email, role })
  }

  async removeMember(orgId: string, userId: string): Promise<void> {
    await orgService.removeMember(orgId, userId)
  }

  async updateMemberRole(
    orgId: string,
    userId: string,
    role: MemberRole,
  ): Promise<void> {
    await orgService.updateMemberRole(orgId, userId, role)
  }

  private enrich(m: OrgMember): TeamMemberDetail {
    const adminRoles = new Set(['owner', 'admin'])
    return {
      ...m,
      displayName: m.userId, // placeholder — enrich from user-service in hooks
      email:       '',        // placeholder
      isAdmin:     adminRoles.has(m.role),
    }
  }
}

export const teamService = new TeamServiceAPI()
