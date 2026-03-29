import { authClient } from '../auth-client-enterprise';
import type {
  SSOProvider,
  Organization,
  OrganizationMember,
  OrganizationInvitation,
  PasskeyCredential,
  SocialProvider,
  EmailValidationResponse,
  TotpSetupResponse,
  TotpVerifyResponse,
  EmailOtpSendResponse,
  EmailOtpVerifyResponse,
  SmsOtpSendResponse,
  SmsOtpVerifyResponse,
  TwoFactorMethodsResponse,
  TwoFactorToggleResponse,
  RecoveryCodesResponse,
  RecoveryCodesRegenerateResponse,
  RecoveryCodeVerifyResponse,
  SecurityAuditResponse,
  UserProfile,
  UpdateProfile,
  ProfileUpdateResponse,
  ConsentResponse,
  UpdateConsent,
  ConsentUpdateResponse,
  ConsentWithdrawResponse,
  PasswordStrengthCheck,
  PasswordStrengthResponse,
} from './auth-contracts';

// Enhanced client for auth providers with Better Auth integration
export class AuthProviderClient {
  private baseURL: string;

  constructor() {
    // Use Next.js frontend API routes which proxy to auth-service
    // This ensures requests go through the frontend's API middleware
    if (typeof window !== 'undefined') {
      // Client-side: use relative path or current origin
      this.baseURL = process.env.NEXT_PUBLIC_APP_URL || window.location.origin;
    } else {
      // Server-side: use the frontend's own URL or localhost
      this.baseURL = process.env.NEXT_PUBLIC_APP_URL || 'http://localhost:3000';
    }
  }

  // Enhanced fetch with Better Auth session handling
  private async enhancedFetch(endpoint: string, options: RequestInit = {}): Promise<Response> {
    try {
      // Get current session from Better Auth
      const session = await authClient.getSession();
      
      const headers = new Headers(options.headers);
      headers.set('Content-Type', 'application/json');
      
      // Include session token if available
      if (session.data?.session?.token) {
        headers.set('Authorization', `Bearer ${session.data.session.token}`);
      }

      const response = await fetch(`${this.baseURL}${endpoint}`, {
        ...options,
        headers,
        credentials: 'include', // Important for session cookies
      });

      if (!response.ok) {
        const errorData = await response.json().catch(() => ({}));
        throw new Error(errorData.message || `HTTP ${response.status}: ${response.statusText}`);
      }

      return response;
    } catch (error) {
      console.error('Auth Provider API Error:', error);
      throw error;
    }
  }

  // SSO Methods
  async discoverSSOProviders(email: string, domain?: string): Promise<{
    success: boolean;
    providers: SSOProvider[];
    domain: string;
    message?: string;
  }> {
    try {
      const response = await this.enhancedFetch('/auth/sso/discover', {
        method: 'POST',
        body: JSON.stringify({ email, domain }),
      });
      return await response.json();
    } catch (error) {
      console.error('SSO Discovery failed:', error);
      throw new Error(error instanceof Error ? error.message : 'SSO discovery failed');
    }
  }

  async authenticateSSO(email: string, providerId: string, redirectTo?: string): Promise<{
    success: boolean;
    redirectUrl?: string;
    message: string;
  }> {
    try {
      const response = await this.enhancedFetch('/auth/sso/authenticate', {
        method: 'POST',
        body: JSON.stringify({ email, providerId, redirectTo }),
      });
      return await response.json();
    } catch (error) {
      console.error('SSO Authentication failed:', error);
      throw new Error(error instanceof Error ? error.message : 'SSO authentication failed');
    }
  }

  // Organization Methods
  async getOrganizations(): Promise<{
    organizations: Organization[];
    total: number;
  }> {
    try {
      const response = await this.enhancedFetch('/organizations', {
        method: 'GET',
      });
      return await response.json();
    } catch (error) {
      console.error('Get organizations failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to fetch organizations');
    }
  }

  async createOrganization(name: string, slug: string, domain?: string): Promise<Organization> {
    try {
      const response = await this.enhancedFetch('/organizations', {
        method: 'POST',
        body: JSON.stringify({ name, slug, domain }),
      });
      return await response.json();
    } catch (error) {
      console.error('Create organization failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to create organization');
    }
  }

  async getOrganizationMembers(organizationId: string): Promise<{
    members: OrganizationMember[];
    total: number;
  }> {
    try {
      const response = await this.enhancedFetch(`/organizations/${organizationId}/members`, {
        method: 'GET',
      });
      return await response.json();
    } catch (error) {
      console.error('Get organization members failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to fetch organization members');
    }
  }

  async getOrganizationInvitations(organizationId: string): Promise<{
    invitations: OrganizationInvitation[];
    total: number;
  }> {
    try {
      const response = await this.enhancedFetch(`/organizations/${organizationId}/invitations`, {
        method: 'GET',
      });
      return await response.json();
    } catch (error) {
      console.error('Get organization invitations failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to fetch organization invitations');
    }
  }

  async createInvitation(email: string, role: 'admin' | 'member', organizationId: string): Promise<OrganizationInvitation> {
    try {
      const response = await this.enhancedFetch(`/organizations/${organizationId}/invitations`, {
        method: 'POST',
        body: JSON.stringify({ email, role }),
      });
      return await response.json();
    } catch (error) {
      console.error('Create invitation failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to create invitation');
    }
  }

  async deleteInvitation(organizationId: string, invitationId: string): Promise<{
    success: boolean;
    message: string;
  }> {
    try {
      const response = await this.enhancedFetch(`/organizations/${organizationId}/invitations/${invitationId}`, {
        method: 'DELETE',
      });
      return await response.json();
    } catch (error) {
      console.error('Delete invitation failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to delete invitation');
    }
  }

  // Passkey Methods
  async getPasskeyCredentials(): Promise<{
    credentials: PasskeyCredential[];
    total: number;
  }> {
    try {
      const response = await this.enhancedFetch('/auth/passkey/credentials', {
        method: 'GET',
      });
      return await response.json();
    } catch (error) {
      console.error('Get passkey credentials failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to fetch passkey credentials');
    }
  }

  async registerPasskey(email: string, displayName?: string): Promise<{
    success: boolean;
    credential?: PasskeyCredential;
    message: string;
  }> {
    try {
      const response = await this.enhancedFetch('/auth/passkey/register', {
        method: 'POST',
        body: JSON.stringify({ email, displayName }),
      });
      return await response.json();
    } catch (error) {
      console.error('Passkey registration failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Passkey registration failed');
    }
  }

  async authenticatePasskey(email?: string): Promise<{
    success: boolean;
    credential?: PasskeyCredential;
    message: string;
  }> {
    try {
      const response = await this.enhancedFetch('/auth/passkey/authenticate', {
        method: 'POST',
        body: JSON.stringify({ email }),
      });
      return await response.json();
    } catch (error) {
      console.error('Passkey authentication failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Passkey authentication failed');
    }
  }

  async deletePasskey(credentialId: string): Promise<{
    success: boolean;
    message: string;
  }> {
    try {
      const response = await this.enhancedFetch(`/auth/passkey/credentials/${credentialId}`, {
        method: 'DELETE',
      });
      return await response.json();
    } catch (error) {
      console.error('Delete passkey failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to delete passkey');
    }
  }

  // Social Provider Methods
  async getSocialProviders(): Promise<{
    providers: SocialProvider[];
  }> {
    try {
      const response = await this.enhancedFetch('/auth/social/providers', {
        method: 'GET',
      });
      return await response.json();
    } catch (error) {
      console.error('Get social providers failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to fetch social providers');
    }
  }

  async authenticateSocial(
    provider: 'google' | 'microsoft',
    redirectTo?: string
  ): Promise<{
    success: boolean;
    redirectUrl?: string;
    message: string;
  }> {
    try {
      const response = await this.enhancedFetch('/auth/social/authenticate', {
        method: 'POST',
        body: JSON.stringify({ provider, redirectTo }),
      });
      return await response.json();
    } catch (error) {
      console.error('Social authentication failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Social authentication failed');
    }
  }

  // OAuth Initiation (New oRPC endpoint)
  async initiateOAuth(
    provider: 'google' | 'microsoft' | 'vipps' | 'okta',
    redirectTo?: string
  ): Promise<{
    success: boolean;
    url: string;
  }> {
    try {
      const response = await this.enhancedFetch('/auth/oauth/initiate', {
        method: 'POST',
        body: JSON.stringify({ provider, redirectTo }),
      });
      return await response.json();
    } catch (error) {
      console.error('OAuth initiation failed:', error);
      throw new Error(error instanceof Error ? error.message : 'OAuth initiation failed');
    }
  }

  // Validation Methods
  async validateEmail(email: string): Promise<EmailValidationResponse> {
    try {
      const response = await this.enhancedFetch('/auth/validate/email', {
        method: 'POST',
        body: JSON.stringify({ email }),
      });
      return await response.json();
    } catch (error) {
      console.error('Email validation failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Email validation failed');
    }
  }

  // Real-time email availability check for forms
  async checkEmailAvailability(email: string): Promise<{ 
    available: boolean; 
    valid: boolean;
    isBusinessEmail: boolean;
    message?: string 
  }> {
    try {
      const result = await this.validateEmail(email);
      return {
        available: result.available,
        valid: result.valid,
        isBusinessEmail: result.isBusinessEmail,
        message: result.message,
      };
    } catch {
      // Return default values on error to not break UX
      return { 
        available: true, 
        valid: true, 
        isBusinessEmail: false,
        message: 'Unable to validate email at this time'
      };
    }
  }

  // Utility method for domain extraction
  getDomainFromEmail(email: string): string {
    return email.split('@')[1]?.toLowerCase() || '';
  }

  // Check if email is from a business domain
  isBusinessEmail(email: string): boolean {
    const personalDomains = ['gmail.com', 'yahoo.com', 'hotmail.com', 'outlook.com', 'icloud.com'];
    const domain = this.getDomainFromEmail(email);
    return !personalDomains.includes(domain);
  }

  // ===================================
  // VERIFICATION METHODS
  // ===================================

  // TOTP/Authenticator Methods
  async setupTotp(): Promise<TotpSetupResponse> {
    try {
      const response = await this.enhancedFetch('/auth/verification/totp/setup', {
        method: 'POST',
      });
      return await response.json();
    } catch (error) {
      console.error('TOTP setup failed:', error);
      throw new Error(error instanceof Error ? error.message : 'TOTP setup failed');
    }
  }

  async verifyTotp(code: string): Promise<TotpVerifyResponse> {
    try {
      const response = await this.enhancedFetch('/auth/verification/totp/verify', {
        method: 'POST',
        body: JSON.stringify({ code }),
      });
      return await response.json();
    } catch (error) {
      console.error('TOTP verification failed:', error);
      throw new Error(error instanceof Error ? error.message : 'TOTP verification failed');
    }
  }

  // Email OTP Methods
  async sendEmailOtp(
    email: string,
    purpose: 'verification' | 'login' | 'reset' | 'change-email' = 'verification',
    language: 'no' | 'en' = 'no'
  ): Promise<EmailOtpSendResponse> {
    try {
      const response = await this.enhancedFetch('/auth/verification/email/send', {
        method: 'POST',
        body: JSON.stringify({ email, purpose, language }),
      });
      return await response.json();
    } catch (error) {
      console.error('Email OTP send failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to send email OTP');
    }
  }

  async verifyEmailOtp(
    email: string,
    code: string,
    purpose: 'verification' | 'login' | 'reset' | 'change-email' = 'verification'
  ): Promise<EmailOtpVerifyResponse> {
    try {
      const response = await this.enhancedFetch('/auth/verification/email/verify', {
        method: 'POST',
        body: JSON.stringify({ email, code, purpose }),
      });
      return await response.json();
    } catch (error) {
      console.error('Email OTP verification failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Email OTP verification failed');
    }
  }

  // SMS OTP Methods
  async sendSmsOtp(
    phoneNumber: string,
    purpose: 'verification' | 'login' | 'reset' = 'verification',
    language: 'no' | 'en' = 'no'
  ): Promise<SmsOtpSendResponse> {
    try {
      const response = await this.enhancedFetch('/auth/verification/sms/send', {
        method: 'POST',
        body: JSON.stringify({ phoneNumber, purpose, language }),
      });
      return await response.json();
    } catch (error) {
      console.error('SMS OTP send failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to send SMS OTP');
    }
  }

  async verifySmsOtp(
    phoneNumber: string,
    code: string,
    purpose: 'verification' | 'login' | 'reset' = 'verification'
  ): Promise<SmsOtpVerifyResponse> {
    try {
      const response = await this.enhancedFetch('/auth/verification/sms/verify', {
        method: 'POST',
        body: JSON.stringify({ phoneNumber, code, purpose }),
      });
      return await response.json();
    } catch (error) {
      console.error('SMS OTP verification failed:', error);
      throw new Error(error instanceof Error ? error.message : 'SMS OTP verification failed');
    }
  }

  // 2FA Management Methods
  async getTwoFactorMethods(): Promise<TwoFactorMethodsResponse> {
    try {
      const response = await this.enhancedFetch('/auth/verification/2fa/methods', {
        method: 'GET',
      });
      return await response.json();
    } catch (error) {
      console.error('Get 2FA methods failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to fetch 2FA methods');
    }
  }

  async toggleTwoFactorMethod(methodId: string, enabled: boolean): Promise<TwoFactorToggleResponse> {
    try {
      const response = await this.enhancedFetch('/auth/verification/2fa/toggle', {
        method: 'POST',
        body: JSON.stringify({ methodId, enabled }),
      });
      return await response.json();
    } catch (error) {
      console.error('Toggle 2FA method failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to toggle 2FA method');
    }
  }

  // Recovery Codes Methods
  async getRecoveryCodes(): Promise<RecoveryCodesResponse> {
    try {
      const response = await this.enhancedFetch('/auth/verification/recovery/codes', {
        method: 'GET',
      });
      return await response.json();
    } catch (error) {
      console.error('Get recovery codes failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to fetch recovery codes');
    }
  }

  async regenerateRecoveryCodes(confirmRegenerate: boolean = true): Promise<RecoveryCodesRegenerateResponse> {
    try {
      const response = await this.enhancedFetch('/auth/verification/recovery/regenerate', {
        method: 'POST',
        body: JSON.stringify({ confirmRegenerate }),
      });
      return await response.json();
    } catch (error) {
      console.error('Regenerate recovery codes failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to regenerate recovery codes');
    }
  }

  async verifyRecoveryCode(code: string): Promise<RecoveryCodeVerifyResponse> {
    try {
      const response = await this.enhancedFetch('/auth/verification/recovery/verify', {
        method: 'POST',
        body: JSON.stringify({ code }),
      });
      return await response.json();
    } catch (error) {
      console.error('Recovery code verification failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Recovery code verification failed');
    }
  }

  // Security Audit Methods
  async getSecurityAudit(
    limit: number = 20,
    offset: number = 0,
    eventType: 'login' | 'logout' | '2fa-setup' | '2fa-verify' | 'password-change' | 'email-change' | 'all' = 'all'
  ): Promise<SecurityAuditResponse> {
    try {
      const params = new URLSearchParams({
        limit: limit.toString(),
        offset: offset.toString(),
        eventType,
      });
      
      const response = await this.enhancedFetch(`/auth/verification/audit?${params.toString()}`, {
        method: 'GET',
      });
      return await response.json();
    } catch (error) {
      console.error('Get security audit failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to fetch security audit');
    }
  }

  // ===================================
  // PROFILE MANAGEMENT METHODS
  // ===================================

  async getProfile(): Promise<{ user: UserProfile }> {
    try {
      const response = await this.enhancedFetch('/auth/profile/getProfile', {
        method: 'POST',
        body: JSON.stringify({}),
      });
      return await response.json();
    } catch (error) {
      console.error('Get profile failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to fetch user profile');
    }
  }

  async updateProfile(data: UpdateProfile): Promise<ProfileUpdateResponse> {
    try {
      const response = await this.enhancedFetch('/auth/profile/updateProfile', {
        method: 'POST',
        body: JSON.stringify(data),
      });
      return await response.json();
    } catch (error) {
      console.error('Update profile failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to update user profile');
    }
  }

  // ===================================
  // GDPR CONSENT METHODS
  // ===================================

  async getConsent(): Promise<ConsentResponse> {
    try {
      const response = await this.enhancedFetch('/api/auth/consent/get', {
        method: 'POST',
        body: JSON.stringify({}),
      });
      return await response.json();
    } catch (error) {
      console.error('Get consent failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to fetch consent preferences');
    }
  }

  async updateConsent(data: UpdateConsent): Promise<ConsentUpdateResponse> {
    try {
      const response = await this.enhancedFetch('/api/auth/consent/update', {
        method: 'POST',
        body: JSON.stringify(data),
      });
      return await response.json();
    } catch (error) {
      console.error('Update consent failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to update consent preferences');
    }
  }

  async withdrawConsent(): Promise<ConsentWithdrawResponse> {
    try {
      const response = await this.enhancedFetch('/api/auth/consent/withdraw', {
        method: 'POST',
        body: JSON.stringify({}),
      });
      return await response.json();
    } catch (error) {
      console.error('Withdraw consent failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to withdraw consent');
    }
  }

  // ===================================
  // PASSWORD STRENGTH METHODS
  // ===================================

  async checkPasswordStrength(password: string): Promise<PasswordStrengthResponse> {
    try {
      const response = await this.enhancedFetch('/auth/password/check-strength', {
        method: 'POST',
        body: JSON.stringify({ password }),
      });
      return await response.json();
    } catch (error) {
      console.error('Password strength check failed:', error);
      throw new Error(error instanceof Error ? error.message : 'Failed to check password strength');
    }
  }

  // Utility Methods for Verification
  formatPhoneNumber(phoneNumber: string): string {
    // Basic Norwegian phone number formatting
    const cleaned = phoneNumber.replace(/\D/g, '');
    if (cleaned.startsWith('47')) {
      return `+${cleaned}`;
    } else if (cleaned.length === 8) {
      return `+47${cleaned}`;
    }
    return `+${cleaned}`;
  }

  maskPhoneNumber(phoneNumber: string): string {
    const cleaned = phoneNumber.replace(/\D/g, '');
    if (cleaned.length >= 8) {
      const last4 = cleaned.slice(-4);
      const prefix = cleaned.slice(0, -4).replace(/./g, '*');
      return `+47${prefix}${last4}`;
    }
    return phoneNumber;
  }

  generateQrCodeUrl(secret: string, email: string, issuer: string = 'ID-Knuten'): string {
    const label = encodeURIComponent(`${issuer}:${email}`);
    const params = new URLSearchParams({
      secret,
      issuer,
    });
    return `otpauth://totp/${label}?${params.toString()}`;
  }
}

// Create and export the client instance
export const authProviderClient = new AuthProviderClient();

// Export types for use in hooks
export type AuthProviderClientType = InstanceType<typeof AuthProviderClient>;
