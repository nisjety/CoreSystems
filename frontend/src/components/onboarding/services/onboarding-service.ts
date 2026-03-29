import { authService } from '@/components/auth/services/auth-service'
import { userService, type CreateUserData, type UpdateProfileData } from '@/lib/services/user-service'
import { orgService, type CreateOrgData } from '@/lib/services/org-service'
import { analyticsService } from '@/lib/services/analytics-service'

const ONBOARDING_DEBUG =
  process.env.NEXT_PUBLIC_DEBUG_ONBOARDING === '1' || process.env.NEXT_PUBLIC_DEBUG === '1'

const onboardingDebug = (...args: unknown[]) => {
  if (ONBOARDING_DEBUG) {
    console.log(...args)
  }
}

export interface OnboardingWebsiteData {
  url: string
  crawlJobId?: string
}

export interface OnboardingConnectData {
  sharePoint: boolean
  oneDrive: boolean
  teams: boolean
  outlook: boolean
  skipped: boolean
}

export interface OnboardingState {
  step: 'profile' | 'organization' | 'website' | 'connect' | 'team' | 'complete'
  completed: boolean
  profile?: {
    firstName?: string
    lastName?: string
    displayName?: string
    phoneNumber?: string
    timezone?: string
  }
  organization?: {
    id?: string
    name?: string
    slug?: string
    plan?: 'free' | 'pro' | 'enterprise'
    action?: 'create' | 'join'
  }
  team?: {
    invitedMembers?: string[]
  }
}

export interface OnboardingProfileData {
  firstName: string
  lastName: string
  displayName?: string
  phoneNumber?: string
  timezone?: string
  jobTitle?: string
  department?: string
}

export interface OnboardingOrgData {
  action: 'create' | 'join'
  organizationName?: string
  organizationSlug?: string
  invitationCode?: string
  plan?: 'free' | 'pro' | 'enterprise'
  /** Norwegian organisation number from Enhetsregisteret (optional). */
  orgNumber?: string
  /** Full Brreg entity data; present when the user verified the org. */
  brregData?: Record<string, unknown>
}

export interface OnboardingTeamData {
  inviteEmails: string[]
  roles: Record<string, 'admin' | 'member' | 'viewer'>
}

class OnboardingServiceAPI {
  private STORAGE_KEY = 'onboarding_state'

  private generateSlug(name: string): string {
    return name
      .toLowerCase()
      .trim()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '')
  }

  // Get onboarding state from localStorage
  getOnboardingState(): OnboardingState | null {
    if (typeof window === 'undefined') return null
    
    try {
      const stored = localStorage.getItem(this.STORAGE_KEY)
      return stored ? JSON.parse(stored) : null
    } catch {
      return null
    }
  }

  // Save onboarding state to localStorage
  saveOnboardingState(state: OnboardingState): void {
    if (typeof window === 'undefined') return
    
    try {
      localStorage.setItem(this.STORAGE_KEY, JSON.stringify(state))
    } catch (error) {
      console.error('Failed to save onboarding state:', error)
    }
  }

  // Clear onboarding state
  clearOnboardingState(): void {
    if (typeof window === 'undefined') return
    
    try {
      localStorage.removeItem(this.STORAGE_KEY)
    } catch (error) {
      console.error('Failed to clear onboarding state:', error)
    }
  }

  // Ensure user is provisioned in user-core (auto-provision from OAuth)
  private async ensureUserProvisioned(user: { id: string; email: string; name: string; image?: string }): Promise<void> {
    try {
      onboardingDebug('🔄 Ensuring user is provisioned:', user.id)
      
      // Try to get user from user-core, which will auto-create if doesn't exist
      await userService.getCurrentUser(user.email, user.id)
      
      onboardingDebug('✅ User provisioned successfully')
    } catch {
      console.warn('⚠️ Failed to provision user, continuing onboarding flow')
      // Don't throw - let onboarding continue even if provisioning fails
    }
  }

  // Check if user needs onboarding
  async needsOnboarding(): Promise<boolean> {
    try {
      const user = await authService.getCurrentUser()
      if (!user) {
        onboardingDebug('🔍 needsOnboarding: No authenticated user found')
        return false
      }

      onboardingDebug('🔍 needsOnboarding: Checking for user:', user.id)

      // First, ensure user is provisioned in user-core
      // This auto-creates the user record with OAuth data if it doesn't exist
      await this.ensureUserProvisioned(user)

      // Check if user has completed profile and onboarding flag from backend
      // Note: This will auto-create the profile if it doesn't exist
      let hasProfile = false
      let onboardingComplete = false
      try {
        const profile = await userService.getCurrentUser(user.email, user.id)
        onboardingComplete = Boolean((profile as any)?.onboardingComplete)
        if (onboardingComplete) {
          onboardingDebug('🔍 needsOnboarding: onboarding already complete in backend')
          return false
        }

        const userProfile = await userService.getCurrentUserProfile(user.email, user.id)
        hasProfile = !!(userProfile && userProfile.firstName && userProfile.lastName)
        onboardingDebug('🔍 needsOnboarding: hasProfile =', hasProfile, userProfile)
      } catch {
        console.warn('⚠️ needsOnboarding: User/profile lookup not available yet')
      }

      // If local onboarding state is complete, do not re-trigger onboarding
      const localState = this.getOnboardingState()
      if (localState?.completed) {
        onboardingDebug('🔍 needsOnboarding: local onboarding state is complete')
        return false
      }

      // Check if user belongs to an organization
      let hasOrg: boolean | null = null
      try {
        const orgs = await orgService.getMyOrganizations(user.id)
        hasOrg = !!(orgs && orgs.length > 0)
        onboardingDebug('🔍 needsOnboarding: hasOrg =', hasOrg, orgs?.length)
      } catch {
        console.warn('⚠️ needsOnboarding: Organizations not available yet')
        // Don't force onboarding on transient org lookup failures
        hasOrg = null
      }

      const needsOnboarding = !hasProfile || hasOrg === false
      onboardingDebug('🔍 needsOnboarding: Final result =', needsOnboarding, '(hasProfile:', hasProfile, ', hasOrg:', hasOrg, ', onboardingComplete:', onboardingComplete, ')')
      return needsOnboarding
    } catch (error) {
      console.error('❌ needsOnboarding: Unexpected error:', error)
      // On unexpected error, avoid forcing onboarding loops
      return false
    }
  }

  // Persist onboarding progress to backend (best-effort, never throws)
  private async _pushOrgOnboardingState(
    orgId: string,
    status: string,
    steps: Record<string, unknown>,
  ): Promise<void> {
    try {
      await fetch(`/api/org/internal/orgs/${orgId}/onboarding/state`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ status, steps }),
      })
    } catch (err) {
      console.warn('⚠️ Failed to push onboarding state to backend:', err)
    }
  }

  // Initialize onboarding
  async startOnboarding(): Promise<OnboardingState> {
    const user = await authService.getCurrentUser()
    
    // Track analytics
    if (user) {
      analyticsService.trackOnboardingStarted(user.id)
    }

    const state: OnboardingState = {
      step: 'profile',
      completed: false,
    }
    
    this.saveOnboardingState(state)
    return state
  }

  // Step 1: Save profile data
  async completeProfile(data: OnboardingProfileData): Promise<void> {
    const user = await authService.getCurrentUser()
    if (!user) throw new Error('User not authenticated')

    analyticsService.trackStepEntered('profile', user.id)

    try {
      const profileData: UpdateProfileData = {
        firstName: data.firstName,
        lastName: data.lastName,
        displayName: data.displayName || `${data.firstName} ${data.lastName}`,
        phoneNumber: data.phoneNumber,
        officeLocation: data.timezone,
        timezone: data.timezone,
        position: data.jobTitle,
        department: data.department,
      }

      await userService.updateCurrentUserProfile(user.email, profileData, user.id)

      // Update onboarding state
      const state = this.getOnboardingState() || { step: 'profile', completed: false }
      state.profile = {
        firstName: data.firstName,
        lastName: data.lastName,
        displayName: profileData.displayName,
        phoneNumber: data.phoneNumber,
        timezone: data.timezone,
      }
      state.step = 'organization'
      this.saveOnboardingState(state)

      analyticsService.trackStepCompleted('profile', user.id, undefined, {
        name: profileData.displayName,
        timezone: data.timezone,
      })
    } catch (error) {
      analyticsService.trackStepError(
        'profile',
        error instanceof Error ? error.message : 'Unknown error',
        user.id
      )
      throw error
    }
  }

  // Step 2: Create or join organization
  async setupOrganization(data: OnboardingOrgData): Promise<void> {
    const user = await authService.getCurrentUser()
    if (!user) throw new Error('User not authenticated')

    analyticsService.trackStepEntered('organization', user.id)

    try {
      let orgId: string

      if (data.action === 'create') {
        // Create new organization
        if (!data.organizationName) throw new Error('Organization name required')
        const derivedSlug = this.generateSlug(data.organizationName)
        
        const orgData: CreateOrgData = {
          name: data.organizationName,
          slug: derivedSlug,
          plan: data.plan || 'free',
          ...(data.orgNumber ? { org_number: data.orgNumber } : {}),
          ...(data.brregData ? { 
            brreg_data: data.brregData,
            verification_status: 'verified' as const
          } : {}),
        }

        const organization = await orgService.createOrganization(orgData, user.id)
        orgId = organization.id
      } else {
        // Join existing organization via invitation code
        if (!data.invitationCode) throw new Error('Invitation code required')

        const result = await orgService.acceptInvitation(data.invitationCode.trim())
        orgId = result.organizationId
      }

      // Update onboarding state
      const state = this.getOnboardingState() || { step: 'organization', completed: false }
      state.organization = {
        id: orgId,
        name: data.organizationName,
        slug: data.organizationName ? this.generateSlug(data.organizationName) : undefined,
        plan: data.plan,
        action: data.action,
      }
      state.step = 'website'
      this.saveOnboardingState(state)

      // Persist to backend (org-core onboarding state)
      await this._pushOrgOnboardingState(orgId, 'ORG_CREATED', {
        plan: data.plan,
        action: data.action,
      })

      analyticsService.trackStepCompleted('organization', user.id, orgId, {
        action: data.action,
        org_name: data.organizationName,
        plan: data.plan,
      })
    } catch (error) {
      analyticsService.trackStepError(
        'organization',
        error instanceof Error ? error.message : 'Unknown error',
        user.id,
        undefined,
        { action: data.action }
      )
      throw error
    }
  } 

  // Step 3: Connect company website (Quarry crawl job)
  async setupWebsite(data: OnboardingWebsiteData): Promise<void> {
    const state = this.getOnboardingState() || { step: 'website' as const, completed: false }
    const orgId = (state as any).organization?.id

    analyticsService.trackStepEntered('website', undefined, orgId)

    try {
      // Persist the URL and optional job ID in state
      ;(state as any).website = {
        url: data.url,
        crawlJobId: data.crawlJobId,
      }
      state.step = 'connect'
      this.saveOnboardingState(state)

      // If crawl job completed, ingest results into Data Plane
      if (data.crawlJobId && orgId) {
        try {
          await this._ingestCrawlResultsToDataPlane(data.crawlJobId, orgId, data.url)
        } catch (ingestError) {
          console.error('Crawl result ingestion failed (non-blocking):', ingestError)
          // Don't throw—allow step to complete even if ingestion fails; can be retried
        }
      }

      // Persist to backend if org is already set
      if (orgId) {
        await this._pushOrgOnboardingState(orgId, 'WEBSITE_CONFIGURED', {
          website_url: data.url,
          crawl_job_id: data.crawlJobId ?? null,
        })
      }

      analyticsService.trackStepCompleted('website', undefined, orgId, {
        url: data.url,
        crawl_job_id: data.crawlJobId,
      })
    } catch (error) {
      analyticsService.trackStepError(
        'website',
        error instanceof Error ? error.message : 'Unknown error',
        undefined,
        orgId
      )
      throw error
    }
  }

  // Fetch crawl results from Quarry and ingest into Data Plane
  // Delegates to the server-side API route to avoid CORS restrictions.
  private async _ingestCrawlResultsToDataPlane(
    crawlJobId: string,
    orgId: string,
    sourceUrl: string
  ): Promise<void> {
    const response = await fetch('/api/ingestion/ingest-job', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ crawlJobId, orgId, sourceUrl }),
    })

    if (!response.ok) {
      const text = await response.text().catch(() => '')
      throw new Error(`Ingest-job API failed (${response.status}): ${text}`)
    }

    const result = await response.json()
    onboardingDebug(
      `Ingested ${result.ingestedCount}/${result.totalProducts} crawl results to Data Plane for org ${orgId}`,
      result.errors?.length ? `(${result.failedCount} failed)` : ''
    )
  }

  // Step 4: Connect data sources (Microsoft)
  async setupConnections(data: OnboardingConnectData): Promise<void> {
    const state = this.getOnboardingState() || { step: 'connect' as const, completed: false }
    const user = await authService.getCurrentUser()
    const orgId = (state as any).organization?.id

    analyticsService.trackStepEntered('connect', user?.id, orgId)

    try {
      ;(state as any).connect = data
      state.step = 'team'
      this.saveOnboardingState(state)

      // Persist to backend if org is already set
      if (orgId) {
        await this._pushOrgOnboardingState(orgId, 'CONNECTIONS_CONFIGURED', {
          sharepoint: data.sharePoint,
          onedrive: data.oneDrive,
          teams: data.teams,
          outlook: data.outlook,
        })
      }

      analyticsService.trackStepCompleted('connect', user?.id, orgId, {
        sharepoint: data.sharePoint,
        onedrive: data.oneDrive,
        teams: data.teams,
        outlook: data.outlook,
      })
    } catch (error) {
      analyticsService.trackStepError(
        'connect',
        error instanceof Error ? error.message : 'Unknown error',
        user?.id,
        orgId
      )
      throw error
    }
  }

  // Step 5: Invite team members (optional)
  async inviteTeamMembers(data: OnboardingTeamData): Promise<void> {
    const state = this.getOnboardingState()
    if (!state?.organization?.id) throw new Error('Organization not set')

    const orgId = state.organization.id
    const user = await authService.getCurrentUser()

    analyticsService.trackStepEntered('team', user?.id, orgId)

    try {
      // Invite each team member
      const invitedEmails = []
      for (const email of data.inviteEmails) {
        const role = data.roles[email] || 'member'
        try {
          await orgService.inviteMember(orgId, { email, role })
          invitedEmails.push(email)
        } catch (err) {
          console.warn(`Failed to invite ${email}:`, err)
        }
      }

      // Update onboarding state
      state.team = {
        invitedMembers: invitedEmails,
      }
      state.step = 'complete'
      this.saveOnboardingState(state)

      analyticsService.trackStepCompleted('team', user?.id, orgId, {
        invited_count: invitedEmails.length,
        total_requested: data.inviteEmails.length,
      })
    } catch (error) {
      analyticsService.trackStepError(
        'team',
        error instanceof Error ? error.message : 'Unknown error',
        user?.id,
        orgId
      )
      throw error
    }
  }

  // Complete onboarding
  async completeOnboarding(): Promise<void> {
    const state = this.getOnboardingState()
    if (!state) throw new Error('No onboarding state found')

    const user = await authService.getCurrentUser()
    const orgId = (state as any).organization?.id

    analyticsService.trackStepEntered('complete', user?.id, orgId)

    try {
      state.completed = true
      state.step = 'complete'
      this.saveOnboardingState(state)

      // Mark onboarding as complete in user-core
      if (user) {
        // Update user's onboarding_complete flag
        await userService.markOnboardingComplete(user.email, user.id)
        onboardingDebug('✅ Onboarding marked as complete in user service')

        // Track completion
        analyticsService.trackStepCompleted('complete', user.id, orgId)
        analyticsService.trackOnboardingCompleted(user.id, orgId || '', {
          total_steps: 6,
        })

        // Print summary for debugging
        analyticsService.printSummary()
      }
    } catch (error) {
      analyticsService.trackStepError(
        'complete',
        error instanceof Error ? error.message : 'Unknown error',
        user?.id,
        orgId
      )
      throw error
    } finally {
      // Clear state after a short delay to allow redirect
      setTimeout(() => {
        this.clearOnboardingState()
      }, 1000)
    }
  }

  // Skip team invitation step
  async skipTeamInvitation(): Promise<void> {
    const state = this.getOnboardingState()
    if (!state) throw new Error('No onboarding state found')

    const user = await authService.getCurrentUser()
    const orgId = (state as any).organization?.id

    analyticsService.trackStepSkipped('team', user?.id, orgId, 'user_skipped')

    state.step = 'complete'
    this.saveOnboardingState(state)
  }

  // Get current onboarding step
  getCurrentStep(): 'profile' | 'organization' | 'website' | 'connect' | 'team' | 'complete' | null {
    const state = this.getOnboardingState()
    return state?.step || null
  }

  // Check if onboarding is complete
  isOnboardingComplete(): boolean {
    const state = this.getOnboardingState()
    return state?.completed || false
  }
}

export const onboardingService = new OnboardingServiceAPI()
