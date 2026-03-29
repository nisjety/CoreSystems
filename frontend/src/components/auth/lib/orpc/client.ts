import { authClient } from '../auth-client-enterprise';
import { z } from 'zod';
// Lightweight logger (avoids separate file to keep tree small)
const __CONSENT_DEBUG__ = (process.env.NEXT_PUBLIC_DEBUG_CONSENT === '1') || (process.env.NEXT_PUBLIC_DEBUG === '1');
const consentDebug = (...args: unknown[]) => { if (__CONSENT_DEBUG__) {
  console.log('[consent]', ...args);
}};

// Enhanced ORPC client for Better Auth integration
// This replaces the mock API client with type-safe ORPC calls

// Base schemas for authentication operations
export const AuthSchemas = {
  // Sign-in schema
  signIn: z.object({
    email: z.string().email(),
    password: z.string().min(8),
    rememberMe: z.boolean().optional(),
  }),

  // Sign-up schema
  signUp: z.object({
    name: z.string().min(2),
    email: z.string().email(),
    password: z.string().min(8),
    confirmPassword: z.string().min(8),
  }).refine((data) => data.password === data.confirmPassword, {
    message: "Passwords don't match",
    path: ["confirmPassword"],
  }),

  // Password reset schema
  resetPassword: z.object({
    email: z.string().email(),
  }),

  // Profile update schema
  updateProfile: z.object({
    name: z.string().min(2).optional(),
    email: z.string().email().optional(),
  }),

  // 2FA enable schema
  enable2FA: z.object({
    method: z.enum(['email', 'sms', 'totp']),
    phoneNumber: z.string().optional(),
  }),

  // SSO authentication schema
  ssoAuth: z.object({
    email: z.string().email(),
    domain: z.string().optional(),
  }),

  // Organization creation schema
  createOrganization: z.object({
    name: z.string().min(2),
    slug: z.string().min(2),
    domain: z.string().optional(),
  }),
} as const;

// Response schemas
export const ResponseSchemas = {
  user: z.object({
    id: z.string(),
    email: z.string().email(),
    name: z.string(),
    emailVerified: z.boolean(),
    phoneNumber: z.string().optional(),
    phoneVerified: z.boolean().optional(),
    twoFactorEnabled: z.boolean().optional(),
    createdAt: z.string(),
    updatedAt: z.string(),
  }),

  session: z.object({
    token: z.string(),
    expiresAt: z.string(),
    user: z.object({
      id: z.string(),
      email: z.string(),
      name: z.string(),
    }),
  }),

  authResult: z.object({
    success: z.boolean(),
    user: z.object({
      id: z.string(),
      email: z.string(),
      name: z.string(),
    }).optional(),
    session: z.object({
      token: z.string(),
      expiresAt: z.string(),
    }).optional(),
    requiresVerification: z.boolean().optional(),
    requires2FA: z.boolean().optional(),
    message: z.string().optional(),
  }),

  enable2FAResult: z.object({
    success: z.boolean(),
    qrCode: z.string().optional(),
    secret: z.string().optional(),
    backupCodes: z.array(z.string()).optional(),
    message: z.string().optional(),
  }),

  genericResult: z.object({
    success: z.boolean(),
    message: z.string(),
  }),
} as const;

// Enhanced ORPC client for authentication
export class AuthORPCClient {
  private baseURL: string;

  constructor() {
    if (typeof window !== 'undefined') {
      this.baseURL = `${window.location.origin}/api/auth`;
      return;
    }

    const serverUrl =
      process.env.API_AUTH_URL ||
      process.env.NEXT_PUBLIC_API_URL ||
      'http://auth-service:3011';
    this.baseURL = serverUrl.endsWith('/api/auth')
      ? serverUrl
      : `${serverUrl}/api/auth`;
  }

  private async makeRequest<T>(
    endpoint: string,
    options: RequestInit = {},
    schema?: z.ZodSchema<T>
  ): Promise<T> {
    try {
      // Get current session from Better Auth for session cookies
      await authClient.getSession();
      
      const response = await fetch(`${this.baseURL}${endpoint}`, {
        ...options,
        headers: {
          'Content-Type': 'application/json',
          // Include session cookies automatically via Better Auth
          ...options.headers,
        },
        credentials: 'include', // Important for session cookies
      });

      if (!response.ok) {
        const errorData = await response.json().catch(() => ({}));
        throw new Error(errorData.message || `HTTP ${response.status}: ${response.statusText}`);
      }

      const data = await response.json();
      
      // Validate response with schema if provided
      if (schema) {
        return schema.parse(data);
      }
      
      return data as T;
    } catch (error) {
      console.error(`ORPC Auth Error [${endpoint}]:`, error);
      throw error;
    }
  }

  // Authentication methods using Better Auth client directly
  async signIn(data: z.infer<typeof AuthSchemas.signIn>) {
    // Validate input
    const validatedData = AuthSchemas.signIn.parse(data);
    
    try {
      // Use Better Auth client for sign-in
      const result = await authClient.signIn.email({
        email: validatedData.email,
        password: validatedData.password,
        rememberMe: validatedData.rememberMe,
      });

      // Better Auth returns {data, error} — it does NOT throw on HTTP errors
      if (result.error) {
        throw new Error(result.error.message || 'Sign-in failed');
      }

      return {
        success: true,
        user: result.data?.user,
        message: 'Successfully signed in',
      };
    } catch (error) {
      throw new Error(error instanceof Error ? error.message : 'Sign-in failed');
    }
  }

  async signUp(data: z.infer<typeof AuthSchemas.signUp>) {
    // Validate input
    const validatedData = AuthSchemas.signUp.parse(data);
    
    try {
      // Use Better Auth client for sign-up
      const result = await authClient.signUp.email({
        email: validatedData.email,
        password: validatedData.password,
        name: validatedData.name,
      });

      // Better Auth returns {data, error} — it does NOT throw on HTTP errors
      if (result.error) {
        throw new Error(result.error.message || 'Sign-up failed');
      }

      return {
        success: true,
        user: result.data?.user,
        requiresVerification: !result.data?.user?.emailVerified,
        message: 'Account created successfully',
      };
    } catch (error) {
      throw new Error(error instanceof Error ? error.message : 'Sign-up failed');
    }
  }

  async resetPassword(data: z.infer<typeof AuthSchemas.resetPassword>) {
    // Validate input
    const validatedData = AuthSchemas.resetPassword.parse(data);
    
    try {
      // Use Better Auth client for password reset
      await authClient.forgetPassword({
        email: validatedData.email,
        redirectTo: `${window.location.origin}/reset-password`,
      });

      return {
        success: true,
        message: 'Password reset link sent to your email',
      };
    } catch (error) {
      throw new Error(error instanceof Error ? error.message : 'Password reset failed');
    }
  }

  async signOut() {
    try {
      // Use Better Auth client for sign-out
      await authClient.signOut();
      
      return {
        success: true,
        message: 'Successfully signed out',
      };
    } catch (error) {
      throw new Error(error instanceof Error ? error.message : 'Sign-out failed');
    }
  }

  // SSO authentication
  async authenticateSSO(data: z.infer<typeof AuthSchemas.ssoAuth>) {
    // Validate input
    const validatedData = AuthSchemas.ssoAuth.parse(data);
    
    try {
      // Use Better Auth SSO client
      const result = await authClient.signIn.sso({
        email: validatedData.email,
        callbackURL: `${window.location.origin}/auth/callback`,
      });

      return {
        success: true,
        redirectUrl: result.data?.url,
        message: 'Redirecting to SSO provider',
      };
    } catch (error) {
      throw new Error(error instanceof Error ? error.message : 'SSO authentication failed');
    }
  }

  // Organization management
  async createOrganization(data: z.infer<typeof AuthSchemas.createOrganization>) {
    // Validate input
    const validatedData = AuthSchemas.createOrganization.parse(data);
    
    try {
      // Use Better Auth organization client
      const result = await authClient.organization.create({
        name: validatedData.name,
        slug: validatedData.slug,
        metadata: validatedData.domain ? { domain: validatedData.domain } : undefined,
      });

      return {
        success: true,
        organization: result.data,
        message: 'Organization created successfully',
      };
    } catch (error) {
      throw new Error(error instanceof Error ? error.message : 'Organization creation failed');
    }
  }

  // Profile management
  async getProfile() {
    try {
      // Get current session which includes user data
      const session = await authClient.getSession();
      
      if (!session.data?.user) {
        throw new Error('No authenticated user');
      }

      const user = session.data.user;
      
      // Helper function to convert date to ISO string
      const toISOString = (date: Date | string | undefined | null): string => {
        if (date instanceof Date) {
          return date.toISOString();
        }
        if (typeof date === 'string') {
          return date;
        }
        return new Date().toISOString();
      };

      return ResponseSchemas.user.parse({
        id: user.id,
        email: user.email,
        name: user.name,
        emailVerified: user.emailVerified || false,
        phoneNumber: '', // Property doesn't exist on user type
        phoneVerified: false, // Property doesn't exist on user type
        twoFactorEnabled: false, // Property doesn't exist on user type
        createdAt: toISOString(user.createdAt),
        updatedAt: toISOString(user.updatedAt),
      });
    } catch (error) {
      throw new Error(error instanceof Error ? error.message : 'Failed to get profile');
    }
  }

  async updateProfile(data: z.infer<typeof AuthSchemas.updateProfile>) {
    // Validate input
    const validatedData = AuthSchemas.updateProfile.parse(data);
    
    try {
      // Use Better Auth client for profile update
      const result = await authClient.updateUser(validatedData);

      return ResponseSchemas.user.parse(result.data);
    } catch (error) {
      throw new Error(error instanceof Error ? error.message : 'Profile update failed');
    }
  }

  // 2FA management
  async enable2FA(data: z.infer<typeof AuthSchemas.enable2FA>) {
    // Validate input
    AuthSchemas.enable2FA.parse(data);
    
    try {
      // Mock implementation since twoFactor is not available on authClient
      // In a real implementation, this would use the proper Better Auth 2FA setup
      
      return ResponseSchemas.enable2FAResult.parse({
        success: true,
        message: '2FA setup initiated (mock implementation)',
      });
    } catch (error) {
      throw new Error(error instanceof Error ? error.message : '2FA enable failed');
    }
  }

  async disable2FA() {
    try {
      // Mock implementation since twoFactor is not available on authClient
      // In a real implementation, this would use the proper Better Auth 2FA disable
      console.log('Disabling 2FA for user with password verification');
      
      return ResponseSchemas.genericResult.parse({
        success: true,
        message: '2FA disabled successfully (mock implementation)',
      });
    } catch (error) {
      throw new Error(error instanceof Error ? error.message : '2FA disable failed');
    }
  }

  // Email verification
  async resendEmailVerification() {
    try {
      // Get current user session to get email
      const session = await authClient.getSession();
      
      if (!session.data?.user?.email) {
        throw new Error('No authenticated user email found');
      }

      await authClient.sendVerificationEmail({
        email: session.data.user.email,
      });
      
      return {
        success: true,
        message: 'Verification email sent',
      };
    } catch (error) {
      throw new Error(error instanceof Error ? error.message : 'Failed to send verification email');
    }
  }

  // Password change
  async changePassword(currentPassword: string, newPassword: string) {
    try {
      await authClient.changePassword({
        currentPassword,
        newPassword,
      });

      return {
        success: true,
        message: 'Password changed successfully',
      };
    } catch (error) {
      throw new Error(error instanceof Error ? error.message : 'Password change failed');
    }
  }

  // Social authentication
  async signInWithSocial(provider: 'google' | 'microsoft') {
    try {
      const redirectTo = `${window.location.origin}/auth/callback`;
      const endpoint = `${window.location.origin}/api/auth/oauth/initiate`;

      const response = await fetch(endpoint, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        credentials: 'include',
        redirect: 'manual',
        body: JSON.stringify({
          provider,
          redirectTo,
        }),
      });

      if (!response.ok) {
        const errorText = await response.text();
        throw new Error(errorText || response.statusText);
      }

      const data = await response.json().catch(() => ({}));

      return {
        success: true,
        redirectUrl: data?.url,
        message: `Redirecting to ${provider}`,
      };
    } catch (error) {
      console.error(`${provider} authentication error:`, error);
      throw new Error(error instanceof Error ? error.message : `${provider} authentication failed`);
    }
  }

  // Email availability check for real-time validation
  async checkEmailAvailability(email: string): Promise<{ available: boolean; message?: string }> {
    try {
      // Simple validation first
      if (!email || !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) {
        return { available: false, message: 'Invalid email format' };
      }

      // In a real implementation, this would check against the user database
      // For now, we'll use a simple pattern to simulate availability
      // Simulate that some emails are taken
      const isTaken = email.includes('admin') || email.includes('test') || email.startsWith('user');
      
      return {
        available: !isTaken,
        message: isTaken ? 'This email is already registered' : undefined,
      };
    } catch {
      return { available: true }; // Fail open for availability checks
    }
  }

  // Consent management methods
  async updateUserConsent(data: {
    userId: string;
    organizationId?: string;
    choices: Record<string, boolean>;
    timestamp: number;
    userAgent: string;
    ipAddress: string;
  }): Promise<{ success: boolean }> {
    try {
      // Real API call to Next.js route to set/update consent cookie server-side
      const payload = {
        purposes: data.choices,
        userId: data.userId,
        sessionId: crypto.randomUUID(),
        method: 'preferences',
      };
      let res = await fetch('/api/consent', {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      });
      if (!res.ok) {
        // Fallback to POST if no existing consent cookie
        res = await fetch('/api/consent', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(payload),
        });
      }
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      return { success: true };
    } catch (error) {
      throw new Error(error instanceof Error ? error.message : 'Failed to update consent');
    }
  }

  async createConsentAuditLog(data: {
    userId: string;
    organizationId?: string;
    action: string;
    timestamp: number;
    newConsent?: Record<string, boolean>;
    previousConsent?: Record<string, boolean>;
  }): Promise<{ success: boolean }> {
    try {
  // For now the audit is handled inside server cookie actions when set/update/withdraw.
  // If you have a separate audit sink, POST here.
  consentDebug('createConsentAuditLog', data);
  return { success: true };
    } catch (error) {
      throw new Error(error instanceof Error ? error.message : 'Failed to create audit log');
    }
  }

  async getUserConsent(): Promise<{ choices: Record<string, boolean>; timestamp: number } | null> {
    try {
  const res = await fetch('/api/consent', { method: 'GET' });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const json = await res.json();
  return json.consent;
    } catch (error) {
      throw new Error(error instanceof Error ? error.message : 'Failed to get user consent');
    }
  }
}

// Create and export the ORPC client instance
export const authORPCClient = new AuthORPCClient();

// Export types for use in components
export type AuthORPCClientType = InstanceType<typeof AuthORPCClient>;
export type SignInData = z.infer<typeof AuthSchemas.signIn>;
export type SignUpData = z.infer<typeof AuthSchemas.signUp>;
export type ResetPasswordData = z.infer<typeof AuthSchemas.resetPassword>;
export type UpdateProfileData = z.infer<typeof AuthSchemas.updateProfile>;
export type Enable2FAData = z.infer<typeof AuthSchemas.enable2FA>;
export type SSOAuthData = z.infer<typeof AuthSchemas.ssoAuth>;
export type CreateOrganizationData = z.infer<typeof AuthSchemas.createOrganization>;
