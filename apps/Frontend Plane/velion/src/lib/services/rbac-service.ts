// U6-3 (ui-ux-verevon-gap.md §10): RBAC client.
//
// Speaks to verevon's existing /api/org/[...path] catch-all proxy which
// forwards to org-core's RBAC surface (see internal/http/rbac_handlers.go
// in org-core).

import { apiClient } from '@/lib/api-client'

export interface Role {
  id: string
  org_id: string
  role_name: string
  permissions: string[]
  is_custom: boolean
  created_at: string
  updated_at: string
}

export interface CapabilityCatalogEntry {
  key: string
  group: string
  label: string
  description: string
}

export interface MemberRoleAssignment {
  org_id: string
  user_id: string
  role: string
}

class RBACServiceAPI {
  // ── Capability catalog ────────────────────────────────────────────────
  async getCapabilityCatalog(orgId: string): Promise<CapabilityCatalogEntry[]> {
    const res = await apiClient.get<{ capabilities: CapabilityCatalogEntry[] }>(
      `/api/org/orgs/${encodeURIComponent(orgId)}/roles/catalog`,
    )
    return res.capabilities ?? []
  }

  // ── Roles CRUD ────────────────────────────────────────────────────────
  async listRoles(orgId: string): Promise<Role[]> {
    const res = await apiClient.get<{ roles: Role[] }>(
      `/api/org/orgs/${encodeURIComponent(orgId)}/roles`,
    )
    return res.roles ?? []
  }

  async createRole(
    orgId: string,
    params: { roleName: string; permissions: string[] },
  ): Promise<Role> {
    return apiClient.post<Role>(
      `/api/org/orgs/${encodeURIComponent(orgId)}/roles`,
      {
        role_name: params.roleName,
        permissions: params.permissions,
      },
    )
  }

  async updateRolePermissions(
    orgId: string,
    roleName: string,
    permissions: string[],
  ): Promise<Role> {
    return apiClient.patch<Role>(
      `/api/org/orgs/${encodeURIComponent(orgId)}/roles/${encodeURIComponent(roleName)}`,
      { permissions },
    )
  }

  async deleteRole(orgId: string, roleName: string): Promise<void> {
    await apiClient.delete<void>(
      `/api/org/orgs/${encodeURIComponent(orgId)}/roles/${encodeURIComponent(roleName)}`,
    )
  }

  // ── Member-role assignment ────────────────────────────────────────────
  async assignMemberRole(
    orgId: string,
    userId: string,
    role: string,
  ): Promise<MemberRoleAssignment> {
    return apiClient.patch<MemberRoleAssignment>(
      `/api/org/orgs/${encodeURIComponent(orgId)}/members/${encodeURIComponent(userId)}/role`,
      { role },
    )
  }
}

export const rbacService = new RBACServiceAPI()
