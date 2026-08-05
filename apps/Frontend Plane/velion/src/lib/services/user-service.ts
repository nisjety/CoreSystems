import { apiClient } from '@/lib/api-client'

// User types
export interface User {
  id: string
  email?: string
  name?: string
  avatar?: string
  onboardingComplete?: boolean
  emailHash: string
  displayName?: string
  tenantId?: string
  objectId?: string
  userTier: 'admin' | 'premium' | 'regular' | 'trial'
  status: 'active' | 'inactive' | 'suspended' | 'pending'
  aiModelPreference?: string
  tokenUsageLimit: number
  languagePreference: string
  timezone?: string
  department?: string
  jobTitle?: string
  managerId?: string
  createdAt: Date
  updatedAt: Date
  lastSeen: Date
  profile?: UserProfile
}

export interface UserProfile {
  firstName?: string
  lastName?: string
  displayName?: string
  phoneNumber?: string
  officeLocation?: string
  timezone?: string
  avatarUrl?: string
  bio?: string
  skills?: string[]
  interests?: string[]
}

export interface CreateUserData {
  email: string
  displayName?: string
  tenantId?: string
  objectId?: string
  userTier?: 'admin' | 'premium' | 'regular' | 'trial'
  languagePreference?: string
  timezone?: string
  authProvider?: string
  microsoftProfile?: any
}

export interface UpdateUserData {
  displayName?: string
  userTier?: 'admin' | 'premium' | 'regular' | 'trial'
  status?: 'active' | 'inactive' | 'suspended' | 'pending'
  aiModelPreference?: string
  languagePreference?: string
  timezone?: string
  department?: string
  jobTitle?: string
  managerId?: string
}

/**
 * G18: shape returned by `/api/user/me/session-context`.
 * `onboardingStatus` drives post-login routing — see `verevon-gap.md` §4.2.
 */
export interface SessionContext {
  userId: string
  orgId?: string
  role?: 'owner' | 'admin' | 'member' | 'viewer' | string
  onboardingStatus?: 'COMPLETED' | 'CONNECTORS_PENDING' | 'CREATED' | 'PROFILE_READY' | string
}

export interface UpdateProfileData {
  firstName?: string
  lastName?: string
  displayName?: string
  phoneNumber?: string
  officeLocation?: string
  timezone?: string
  position?: string
  department?: string
  status?: 'online' | 'away' | 'busy' | 'offline'
  avatarUrl?: string
  bio?: string
  skills?: string[]
  interests?: string[]
}

class UserServiceAPI {
  private extractUserPayload(payload: unknown): Record<string, any> {
    if (payload && typeof payload === 'object' && 'user' in (payload as Record<string, any>)) {
      return ((payload as Record<string, any>).user ?? {}) as Record<string, any>
    }
    return (payload ?? {}) as Record<string, any>
  }

  private normalizeCurrentUser(payload: unknown): User {
    const raw = this.extractUserPayload(payload)

    return {
      ...(raw as User),
      id: String(raw.id ?? ''),
      email: typeof raw.email === 'string' ? raw.email : undefined,
      name: typeof raw.name === 'string' ? raw.name : undefined,
      avatar: typeof raw.avatar === 'string' ? raw.avatar : undefined,
      onboardingComplete: Boolean(raw.onboarding_complete ?? raw.onboardingComplete ?? false),
      emailHash: typeof raw.emailHash === 'string' ? raw.emailHash : '',
      userTier: (raw.userTier ?? 'regular') as User['userTier'],
      status: (raw.status ?? 'active') as User['status'],
      tokenUsageLimit: Number(raw.tokenUsageLimit ?? 0),
      languagePreference: String(raw.languagePreference ?? 'en'),
      createdAt: raw.createdAt ?? raw.created_at ?? new Date(),
      updatedAt: raw.updatedAt ?? raw.updated_at ?? new Date(),
      lastSeen: raw.lastSeen ?? raw.last_login_at ?? new Date(),
    }
  }

  private normalizeCurrentUserProfile(payload: unknown): UserProfile | null {
    const raw = this.extractUserPayload(payload)
    if (!raw || typeof raw !== 'object') {
      return null
    }

    const firstName = typeof raw.firstName === 'string'
      ? raw.firstName
      : typeof raw.first_name === 'string'
        ? raw.first_name
        : undefined

    const lastName = typeof raw.lastName === 'string'
      ? raw.lastName
      : typeof raw.last_name === 'string'
        ? raw.last_name
        : undefined

    const displayName = typeof raw.displayName === 'string'
      ? raw.displayName
      : typeof raw.display_name === 'string'
        ? raw.display_name
        : typeof raw.name === 'string'
          ? raw.name
          : undefined

    const phoneNumber = typeof raw.phoneNumber === 'string'
      ? raw.phoneNumber
      : typeof raw.phone === 'string'
        ? raw.phone
        : undefined

    const officeLocation = typeof raw.officeLocation === 'string'
      ? raw.officeLocation
      : typeof raw.location === 'string'
        ? raw.location
        : undefined

    const avatarUrl = typeof raw.avatarUrl === 'string'
      ? raw.avatarUrl
      : typeof raw.avatar === 'string'
        ? raw.avatar
        : undefined

    return {
      firstName,
      lastName,
      displayName,
      phoneNumber,
      officeLocation,
      timezone: typeof raw.timezone === 'string' ? raw.timezone : undefined,
      avatarUrl,
      bio: typeof raw.bio === 'string' ? raw.bio : undefined,
      skills: Array.isArray(raw.skills) ? raw.skills : undefined,
      interests: Array.isArray(raw.interests) ? raw.interests : undefined,
    }
  }

  private getUserEndpoint(endpoint: string): string {
    // Use the API proxy route to user service
    return `/api/user${endpoint}`
  }

  // User endpoints
  async getUsers(): Promise<User[]> {
    return apiClient.get<User[]>(this.getUserEndpoint('/users'))
  }

  async getUserById(id: string): Promise<User> {
    return apiClient.get<User>(this.getUserEndpoint(`/users/${id}`))
  }

  async getUserByEmail(email: string): Promise<User> {
    return apiClient.get<User>(this.getUserEndpoint(`/users/by-email/${encodeURIComponent(email)}`))
  }

  async createUser(data: CreateUserData): Promise<User> {
    return apiClient.post<User>(this.getUserEndpoint('/users'), data)
  }

  async updateUser(id: string, data: UpdateUserData): Promise<User> {
    return apiClient.put<User>(this.getUserEndpoint(`/users/${id}`), data)
  }

  async deleteUser(id: string): Promise<{ message: string }> {
    return apiClient.delete<{ message: string }>(this.getUserEndpoint(`/users/${id}`))
  }

  // Profile endpoints
  async getUserProfile(userId: string): Promise<UserProfile | null> {
    return apiClient.get<UserProfile | null>(this.getUserEndpoint(`/users/${userId}/profile`))
  }

  async updateUserProfile(userId: string, data: UpdateProfileData): Promise<UserProfile> {
    return apiClient.put<UserProfile>(this.getUserEndpoint(`/users/${userId}/profile`), data)
  }

  // Current user endpoints (requires auth)
  async getCurrentUserProfile(email: string, userId?: string): Promise<UserProfile | null> {
    const options = userId
      ? { headers: { 'X-User-Id': userId } }
      : undefined
    try {
      const payload = await apiClient.get<unknown>(
        this.getUserEndpoint('/current'),
        options
      )
      return this.normalizeCurrentUserProfile(payload)
    } catch (error) {
      const message = error instanceof Error ? error.message.toLowerCase() : ''
      if (message.includes('not found') || message.includes('profile') || message.includes('unauthorized')) {
        return null
      }
      throw error
    }
  }

  async getCurrentUser(email: string, userId?: string): Promise<User> {
    const options = userId
      ? { headers: { 'X-User-Id': userId } }
      : undefined
    const payload = await apiClient.get<unknown>(
      this.getUserEndpoint('/current'),
      options
    )
    return this.normalizeCurrentUser(payload)
  }

  async updateCurrentUserProfile(email: string, data: UpdateProfileData, userId?: string): Promise<UserProfile> {
    const options = userId
      ? { headers: { 'X-User-Id': userId } }
      : undefined
    return apiClient.patch<UserProfile>(
      this.getUserEndpoint('/current'),
      data,
      options
    )
  }

  async markOnboardingComplete(email: string, userId?: string): Promise<void> {
    const options = userId
      ? { headers: { 'X-User-Id': userId } }
      : undefined
    return apiClient.post<void>(
      this.getUserEndpoint('/onboarding/complete'),
      {},
      options
    )
  }

  /**
   * G18: canonical post-login routing input. Hits user-core
   * `/api/v1/me/session-context` via the verevon proxy. Returns null on
   * unauthenticated / outage so callers can fall back gracefully.
   *
   * Response shape (per zero-input enterprise roadmap):
   *   { userId, orgId, role, onboardingStatus }
   * onboardingStatus ∈ { 'COMPLETED', 'CONNECTORS_PENDING', 'CREATED', 'PROFILE_READY', ... }
   */
  async getSessionContext(): Promise<SessionContext | null> {
    try {
      return await apiClient.get<SessionContext>('/api/user/me/session-context')
    } catch {
      return null
    }
  }

  // Session endpoints
  async getUserSessions(userId: string): Promise<any[]> {
    return apiClient.get<any[]>(this.getUserEndpoint(`/users/${userId}/sessions`))
  }

  async endSession(sessionId: string): Promise<{ message: string }> {
    return apiClient.delete<{ message: string }>(this.getUserEndpoint(`/users/sessions/${sessionId}`))
  }

  // Search endpoints
  async searchUsers(query: string, limit?: number): Promise<User[]> {
    const params = new URLSearchParams()
    params.append('q', query)
    if (limit) params.append('limit', limit.toString())
    
    return apiClient.get<User[]>(this.getUserEndpoint(`/users/search?${params.toString()}`))
  }

  async getUsersByTenant(tenantId: string): Promise<User[]> {
    return apiClient.get<User[]>(this.getUserEndpoint(`/users/tenant/${tenantId}`))
  }

  async deleteCurrentUser(): Promise<void> {
    return apiClient.delete<void>(this.getUserEndpoint('/users/me'))
  }
}

export const userService = new UserServiceAPI()
