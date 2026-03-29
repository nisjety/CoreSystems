import { userService, type User, type UpdateUserData, type CreateUserData } from '@/lib/services/user-service'
import { orgService, type Organization, type OrgMember, type OrgQuota, type OrgBilling } from '@/lib/services/org-service'

export interface AdminStats {
  totalUsers: number
  totalOrganizations: number
  activeUsers: number
  suspendedUsers: number
  totalQuotaUsage: {
    apiCalls: number
    users: number
    storage: number
  }
}

export interface UserWithOrg extends User {
  organizations: Organization[]
  currentOrgRole?: string
}

export interface OrgWithDetails extends Organization {
  members: OrgMember[]
  quotas: OrgQuota[]
  billing: OrgBilling
  memberCount: number
  quotaUsage: {
    apiCalls: number
    users: number
    storage: number
  }
}

export interface AuditLog {
  id: string
  userId: string
  action: string
  resource: string
  resourceId: string
  details?: Record<string, any>
  timestamp: Date
}

class AdminServiceAPI {
  // Dashboard & Analytics
  async getDashboardStats(): Promise<AdminStats> {
    try {
      const [users, orgs] = await Promise.all([
        userService.getUsers(),
        orgService.getOrganizations(),
      ])

      const activeUsers = users.filter(u => u.status === 'active').length
      const suspendedUsers = users.filter(u => u.status === 'suspended').length

      // Calculate total quota usage across all orgs
      let totalApiCalls = 0
      let totalUsers = 0
      let totalStorage = 0

      for (const org of orgs) {
        const quotas = await orgService.getOrganizationQuotas(org.id)
        totalApiCalls += quotas.find(q => q.quotaKey === 'api_calls')?.quotaValue || 0
        totalUsers += quotas.find(q => q.quotaKey === 'users')?.quotaValue || 0
        totalStorage += quotas.find(q => q.quotaKey === 'storage_mb')?.quotaValue || 0
      }

      return {
        totalUsers: users.length,
        totalOrganizations: orgs.length,
        activeUsers,
        suspendedUsers,
        totalQuotaUsage: {
          apiCalls: totalApiCalls,
          users: totalUsers,
          storage: totalStorage,
        },
      }
    } catch (error) {
      console.error('Failed to fetch dashboard stats:', error)
      throw error
    }
  }

  // User Management
  async getAllUsersWithOrgs(): Promise<UserWithOrg[]> {
    const users = await userService.getUsers()
    
    const usersWithOrgs = await Promise.all(
      users.map(async (user) => {
        try {
          // Get user's organizations
          const orgs = await orgService.getMyOrganizations()
          return {
            ...user,
            organizations: orgs,
          }
        } catch {
          return {
            ...user,
            organizations: [],
          }
        }
      })
    )

    return usersWithOrgs
  }

  async getUserWithDetails(userId: string): Promise<UserWithOrg> {
    const user = await userService.getUserById(userId)
    const orgs = await orgService.getMyOrganizations()

    return {
      ...user,
      organizations: orgs,
    }
  }

  async createUser(data: CreateUserData): Promise<User> {
    return userService.createUser(data)
  }

  async updateUser(userId: string, data: UpdateUserData): Promise<User> {
    return userService.updateUser(userId, data)
  }

  async suspendUser(userId: string): Promise<User> {
    return userService.updateUser(userId, { status: 'suspended' })
  }

  async activateUser(userId: string): Promise<User> {
    return userService.updateUser(userId, { status: 'active' })
  }

  async deleteUser(userId: string): Promise<void> {
    await userService.deleteUser(userId)
  }

  // Organization Management
  async getAllOrgsWithDetails(): Promise<OrgWithDetails[]> {
    const orgs = await orgService.getOrganizations()
    
    const orgsWithDetails = await Promise.all(
      orgs.map(async (org) => {
        try {
          const [members, quotas, billing] = await Promise.all([
            orgService.getOrganizationMembers(org.id),
            orgService.getOrganizationQuotas(org.id),
            orgService.getOrganizationBilling(org.id),
          ])

          const quotaUsage = {
            apiCalls: quotas.find(q => q.quotaKey === 'api_calls')?.quotaValue || 0,
            users: quotas.find(q => q.quotaKey === 'users')?.quotaValue || 0,
            storage: quotas.find(q => q.quotaKey === 'storage_mb')?.quotaValue || 0,
          }

          return {
            ...org,
            members,
            quotas,
            billing,
            memberCount: members.length,
            quotaUsage,
          }
        } catch {
          return {
            ...org,
            members: [],
            quotas: [],
            billing: {} as OrgBilling,
            memberCount: 0,
            quotaUsage: {
              apiCalls: 0,
              users: 0,
              storage: 0,
            },
          }
        }
      })
    )

    return orgsWithDetails
  }

  async getOrgWithDetails(orgId: string): Promise<OrgWithDetails> {
    const org = await orgService.getOrganizationById(orgId)
    const [members, quotas, billing] = await Promise.all([
      orgService.getOrganizationMembers(orgId),
      orgService.getOrganizationQuotas(orgId),
      orgService.getOrganizationBilling(orgId),
    ])

    const quotaUsage = {
      apiCalls: quotas.find(q => q.quotaKey === 'api_calls')?.quotaValue || 0,
      users: quotas.find(q => q.quotaKey === 'users')?.quotaValue || 0,
      storage: quotas.find(q => q.quotaKey === 'storage_mb')?.quotaValue || 0,
    }

    return {
      ...org,
      members,
      quotas,
      billing,
      memberCount: members.length,
      quotaUsage,
    }
  }

  async updateOrgPlan(
    orgId: string,
    plan: 'free' | 'pro' | 'enterprise'
  ): Promise<Organization> {
    return orgService.updatePlan(orgId, plan)
  }

  async updateOrgQuota(
    orgId: string,
    quotaKey: string,
    value: number
  ): Promise<void> {
    await orgService.updateQuota(orgId, quotaKey, value)
  }

  async suspendOrganization(orgId: string): Promise<Organization> {
    return orgService.updateOrganization(orgId, { status: 'suspended' })
  }

  async activateOrganization(orgId: string): Promise<Organization> {
    return orgService.updateOrganization(orgId, { status: 'active' })
  }

  async deleteOrganization(orgId: string): Promise<void> {
    await orgService.deleteOrganization(orgId)
  }

  async hardDeleteOrganization(orgId: string): Promise<void> {
    await orgService.hardDeleteOrganization(orgId)
  }

  // Billing Management
  async updateOrgBilling(orgId: string, data: Partial<OrgBilling>): Promise<void> {
    await orgService.updateBilling(orgId, data)
  }

  // Member Management
  async removeOrgMember(orgId: string, userId: string): Promise<void> {
    await orgService.removeMember(orgId, userId)
  }

  async updateOrgMemberRole(
    orgId: string,
    userId: string,
    role: string
  ): Promise<void> {
    await orgService.updateMemberRole(orgId, userId, role)
  }

  // Audit Logs (placeholder - implement when backend ready)
  async getAuditLogs(filters?: {
    userId?: string
    orgId?: string
    action?: string
    startDate?: Date
    endDate?: Date
  }): Promise<AuditLog[]> {
    // TODO: Implement when audit log endpoint is ready
    console.log('Audit logs requested with filters:', filters)
    return []
  }

  // Search
  async searchUsers(query: string): Promise<User[]> {
    return userService.searchUsers(query)
  }

  async searchOrganizations(query: string): Promise<Organization[]> {
    const allOrgs = await orgService.getOrganizations()
    return allOrgs.filter(
      org =>
        org.name.toLowerCase().includes(query.toLowerCase()) ||
        org.slug.toLowerCase().includes(query.toLowerCase())
    )
  }
}

export const adminService = new AdminServiceAPI()
