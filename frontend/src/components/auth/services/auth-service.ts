import { authClient } from '@/components/auth/lib/auth-client'

export interface User {
  id: string
  email: string
  name: string
  image?: string
  emailVerified: boolean
  role?: string
  createdAt: Date
  updatedAt: Date
}

export interface Session {
  user: User
  session: {
    id: string
    userId: string
    expiresAt: Date
    token: string
  }
}

export class AuthService {
  /**
   * Sign in with email and password
   */
  async login(credentials: { email: string; password: string }): Promise<Session> {
    const response = await authClient.signIn.email(credentials)
    if (response.error) {
      throw new Error(response.error.message || 'Login failed')
    }
    return response.data as unknown as Session
  }

  /**
   * Sign out current user
   */
  async logout(): Promise<void> {
    const response = await authClient.signOut()
    if (response.error) {
      throw new Error(response.error.message || 'Logout failed')
    }
  }

  /**
   * Get current authenticated user session
   */
  async getCurrentUser(): Promise<User | null> {
    try {
      const session = await authClient.getSession()
      const user = session?.data?.user || null;
      if (!user) return null;
      // Normalize nullable image -> undefined to match `User` type
      return {
        ...user,
        image: user.image ?? undefined,
      } as User;
    } catch {
      return null
    }
  }

  /**
   * Register new user with email and password
   */
  async register(userData: { name: string; email: string; password: string }): Promise<Session> {
    const response = await authClient.signUp.email(userData)
    if (response.error) {
      throw new Error(response.error.message || 'Registration failed')
    }
    return response.data as unknown as Session
  }

  /**
   * Check if user is authenticated (client-side only)
   */
  isAuthenticated(): boolean {
    if (typeof window === 'undefined') return false
    
    // Check session from Better Auth
    const session = authClient.useSession()
    return !!session.data
  }

  /**
   * Get auth token from session (for API calls)
   */
  async getToken(): Promise<string | null> {
    try {
      const session = await authClient.getSession()
      return session?.data?.session?.token || null
    } catch {
      return null
    }
  }
}

// Export singleton instance
export const authService = new AuthService()
