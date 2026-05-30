import { apiClient } from '@/lib/api-client'

export type OrgPlan =
  | 'free'
  | 'trial'
  | 'hobby'
  | 'standard'
  | 'pro'
  | 'enterprise'

export type PaidOrgPlan = Exclude<OrgPlan, 'free' | 'trial'>

// Organization types matching org-core schema
export interface Organization {
  id: string
  name: string
  slug: string
  plan: OrgPlan
  status: 'active' | 'suspended' | 'deleted'
  ownerUserId: string
  metadata?: Record<string, any>
  createdAt: Date
  updatedAt: Date
  // Brreg verification fields (Norwegian Enhetsregisteret)
  orgNumber?: string
  verificationStatus?: 'unverified' | 'verified'
  brregData?: Record<string, any>
}

type OrgCapabilities = Record<string, boolean>

export interface OrgQuota {
  orgId: string
  quotaKey: string
  quotaValue: number
  quotaLimit: number
  resetPeriod?: string
  lastResetAt?: Date
  updatedAt: Date
}

export interface OrgBilling {
  orgId: string
  plan: OrgPlan
  subscriptionStatus: 'active' | 'trialing' | 'past_due' | 'canceled' | 'unpaid'
  stripeCustomerId?: string
  stripeSubscriptionId?: string
  paymentMethodId?: string
  trialEndsAt?: Date
  billingEmail?: string
  currentPeriodStart?: Date
  currentPeriodEnd?: Date
  entitlements?: Record<string, boolean>
  quotaLimits?: Record<string, number>
  credits?: number
  createdAt: Date
  updatedAt: Date
}

interface OrgCheckoutSession {
  id: string
  url: string
}

interface OrgCompliance {
  orgId: string
  gdprCompliant: boolean
  hipaaCompliant: boolean
  soc2Compliant: boolean
  dataResidency: string
  retentionPolicyDays: number
  mfaRequired: boolean
  createdAt: Date
  updatedAt: Date
}

interface OrgRoleMapping {
  orgId: string
  role: string
  permissions: string[]
  createdAt: Date
  updatedAt: Date
}

export interface OrgMember {
  id: string
  organizationId: string
  userId: string
  role: string
  invitedBy?: string
  joinedAt: Date
  status: 'active' | 'invited' | 'suspended'
}

export interface CreateOrgData {
  name: string
  slug?: string
  plan?: OrgPlan
  // Brreg verification (optional)
  org_number?: string
  verification_status?: 'unverified' | 'verified'
  brreg_data?: Record<string, any>
}

interface UpdateOrgData {
  name?: string
  slug?: string
  plan?: OrgPlan
  status?: 'active' | 'suspended'
  metadata?: Record<string, any>
}

interface InviteMemberData {
  email: string
  role: 'owner' | 'admin' | 'member' | 'viewer'
}

class OrgServiceAPI {
  private getOrgEndpoint(endpoint: string): string {
    // Use the API proxy route to org service
    return `/api/org${endpoint}`
  }

  // Organization CRUD
  async getOrganizations(): Promise<Organization[]> {
    return apiClient.get<Organization[]>(this.getOrgEndpoint('/orgs'))
  }

  async getOrganizationById(id: string): Promise<Organization> {
    return apiClient.get<Organization>(this.getOrgEndpoint(`/orgs/${id}`))
  }

  async createOrganization(data: CreateOrgData, userId?: string): Promise<Organization> {
    const options = userId
      ? { headers: { 'X-User-Id': userId } }
      : undefined
    return apiClient.post<Organization>(this.getOrgEndpoint('/orgs'), data, options)
  }

  async updateOrganization(id: string, data: UpdateOrgData): Promise<Organization> {
    return apiClient.put<Organization>(this.getOrgEndpoint(`/orgs/${id}`), data)
  }

  async updateCapabilities(
    orgId: string,
    capabilities: OrgCapabilities,
  ): Promise<Organization> {
    return apiClient.patch<Organization>(
      this.getOrgEndpoint(`/orgs/${orgId}/capabilities`),
      { capabilities },
    )
  }

  async deleteOrganization(id: string): Promise<{ message: string }> {
    return apiClient.delete<{ message: string }>(this.getOrgEndpoint(`/orgs/${id}`))
  }

  // Organization details (with quotas, billing, compliance)
  async getOrganizationDetails(id: string): Promise<{
    organization: Organization
    quotas: OrgQuota[]
    billing: OrgBilling
    compliance: OrgCompliance
  }> {
    return apiClient.get(this.getOrgEndpoint(`/orgs/${id}/details`))
  }

  // Quotas
  async getOrganizationQuotas(orgId: string): Promise<OrgQuota[]> {
    return apiClient.get<OrgQuota[]>(this.getOrgEndpoint(`/orgs/${orgId}/quotas`))
  }

  async updateQuota(orgId: string, quotaKey: string, value: number): Promise<OrgQuota> {
    return apiClient.put<OrgQuota>(
      this.getOrgEndpoint(`/orgs/${orgId}/quotas/${quotaKey}`),
      { value }
    )
  }

  // Billing
  async getOrganizationBilling(orgId: string): Promise<OrgBilling> {
    return apiClient.get<OrgBilling>(this.getOrgEndpoint(`/orgs/${orgId}/billing`))
  }

  async updateBilling(orgId: string, data: Partial<OrgBilling>): Promise<OrgBilling> {
    return apiClient.put<OrgBilling>(
      this.getOrgEndpoint(`/orgs/${orgId}/billing`),
      data
    )
  }

  async updatePlan(
    orgId: string,
    plan: OrgPlan,
    options?: { reason?: string; onboarding?: Record<string, unknown> },
  ): Promise<Organization> {
    return apiClient.post<Organization>(
      this.getOrgEndpoint(`/orgs/${orgId}/plan`),
      { plan, ...options }
    )
  }

  async createCheckoutSession(
    orgId: string,
    plan: PaidOrgPlan,
    successUrl: string,
    cancelUrl: string,
  ): Promise<OrgCheckoutSession> {
    return apiClient.post<OrgCheckoutSession>(
      this.getOrgEndpoint(`/orgs/${orgId}/checkout-session`),
      { plan, successUrl, cancelUrl },
    )
  }

  // Compliance
  async getOrganizationCompliance(orgId: string): Promise<OrgCompliance> {
    return apiClient.get<OrgCompliance>(this.getOrgEndpoint(`/orgs/${orgId}/compliance`))
  }

  async updateCompliance(orgId: string, data: Partial<OrgCompliance>): Promise<OrgCompliance> {
    return apiClient.put<OrgCompliance>(
      this.getOrgEndpoint(`/orgs/${orgId}/compliance`),
      data
    )
  }

  // Members
  async inviteMember(orgId: string, data: InviteMemberData): Promise<{ invitation_id: string; status: string }> {
    return apiClient.post<{ invitation_id: string; status: string }>(
      this.getOrgEndpoint(`/orgs/${orgId}/members/invite`),
      data
    )
  }

  async removeMember(orgId: string, userId: string): Promise<{ ok: boolean }> {
    return apiClient.delete<{ ok: boolean }>(
      this.getOrgEndpoint(`/orgs/${orgId}/members/${userId}`)
    )
  }

  async getOrganizationMembers(orgId: string): Promise<OrgMember[]> {
    const data = await apiClient.get<{ members: OrgMember[]; count: number }>(
      this.getOrgEndpoint(`/orgs/${orgId}/members`)
    )
    return data.members ?? []
  }

  async acceptInvitation(invitationId: string): Promise<{ organizationId: string }> {
    // Delegates to auth-core Better Auth invitation acceptance
    return apiClient.post<{ organizationId: string }>(
      '/api/auth/organization/accept-invitation',
      { invitationId },
    )
  }

  async updateMemberRole(
    orgId: string,
    userId: string,
    role: string
  ): Promise<OrgMember> {
    return apiClient.put<OrgMember>(
      this.getOrgEndpoint(`/orgs/${orgId}/members/${userId}/role`),
      { role }
    )
  }

  // Current user's organizations
  async getMyOrganizations(userId?: string): Promise<Organization[]> {
    const options = userId
      ? { headers: { 'X-User-Id': userId } }
      : undefined
    try {
      return await apiClient.get<Organization[]>(this.getOrgEndpoint('/orgs/me'), options)
    } catch (error) {
      const message = error instanceof Error ? error.message.toLowerCase() : ''
      if (message.includes('organization not found') || message.includes('not found')) {
        return []
      }
      throw error
    }
  }

  async getCurrentOrganization(): Promise<Organization | null> {
    try {
      // org-core only has /orgs/me (list); we return the primary (first) org
      const orgs = await apiClient.get<Organization[]>(this.getOrgEndpoint('/orgs/me'))
      return Array.isArray(orgs) && orgs.length > 0 ? orgs[0] : null
    } catch {
      return null
    }
  }

  // GDPR
  async hardDeleteOrganization(orgId: string): Promise<{ success: boolean; deletedRecords: any }> {
    return apiClient.post(this.getOrgEndpoint(`/orgs/${orgId}/gdpr/hard-delete`))
  }
}

export const orgService = new OrgServiceAPI()
