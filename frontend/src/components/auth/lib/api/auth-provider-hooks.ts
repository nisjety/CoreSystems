import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { authProviderClient } from './auth-provider-client';

// Query keys for auth providers
export const authProviderKeys = {
  all: ['authProvider'] as const,
  
  // SSO keys
  sso: ['authProvider', 'sso'] as const,
  ssoDiscovery: (email: string, domain?: string) => 
    ['authProvider', 'sso', 'discovery', { email, domain }] as const,
  
  // Organization keys
  organizations: ['authProvider', 'organizations'] as const,
  organizationsList: () => ['authProvider', 'organizations', 'list'] as const,
  organizationMembers: (organizationId: string) => 
    ['authProvider', 'organizations', organizationId, 'members'] as const,
  organizationInvitations: (organizationId: string) => 
    ['authProvider', 'organizations', organizationId, 'invitations'] as const,
  
  // Passkey keys
  passkeys: ['authProvider', 'passkeys'] as const,
  passkeyCredentials: () => ['authProvider', 'passkeys', 'credentials'] as const,
  
  // Social provider keys
  social: ['authProvider', 'social'] as const,
  socialProviders: () => ['authProvider', 'social', 'providers'] as const,
  
  // Validation keys
  validation: ['authProvider', 'validation'] as const,
  emailValidation: (email: string) => 
    ['authProvider', 'validation', 'email', { email }] as const,
  emailAvailability: (email: string) => 
    ['authProvider', 'validation', 'availability', { email }] as const,
} as const;

// SSO Discovery Hook
export function useDiscoverSSO() {
  return useMutation({
    mutationFn: async ({ email, domain }: { email: string; domain?: string }) => {
      return authProviderClient.discoverSSOProviders(email, domain);
    },
    onSuccess: (data) => {
      console.log('SSO providers discovered:', data.providers);
    },
    onError: (error) => {
      console.error('SSO discovery failed:', error);
    },
  });
}

// SSO Authentication Hook
export function useAuthenticateSSO() {
  return useMutation({
    mutationFn: async ({ 
      email, 
      providerId, 
      redirectTo 
    }: { 
      email: string; 
      providerId: string; 
      redirectTo?: string; 
    }) => {
      return authProviderClient.authenticateSSO(email, providerId, redirectTo);
    },
    onSuccess: (data) => {
      if (data.redirectUrl) {
        window.location.href = data.redirectUrl;
      }
    },
    onError: (error) => {
      console.error('SSO authentication failed:', error);
    },
  });
}

// Organizations List Hook
export function useOrganizations() {
  return useQuery({
    queryKey: authProviderKeys.organizationsList(),
    queryFn: () => authProviderClient.getOrganizations(),
    staleTime: 5 * 60 * 1000, // 5 minutes
    gcTime: 10 * 60 * 1000, // 10 minutes
  });
}

// Create Organization Hook
export function useCreateOrganization() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ 
      name, 
      slug, 
      domain 
    }: { 
      name: string; 
      slug: string; 
      domain?: string; 
    }) => {
      return authProviderClient.createOrganization(name, slug, domain);
    },
    onSuccess: () => {
      // Invalidate and refetch organizations list
      queryClient.invalidateQueries({ 
        queryKey: authProviderKeys.organizationsList() 
      });
    },
    onError: (error) => {
      console.error('Organization creation failed:', error);
    },
  });
}

// Organization Members Hook
export function useOrganizationMembers(organizationId: string) {
  return useQuery({
    queryKey: authProviderKeys.organizationMembers(organizationId),
    queryFn: () => authProviderClient.getOrganizationMembers(organizationId),
    enabled: !!organizationId,
    staleTime: 5 * 60 * 1000, // 5 minutes
  });
}

// Organization Invitations Hook
export function useOrganizationInvitations(organizationId: string) {
  return useQuery({
    queryKey: authProviderKeys.organizationInvitations(organizationId),
    queryFn: () => authProviderClient.getOrganizationInvitations(organizationId),
    enabled: !!organizationId,
    staleTime: 2 * 60 * 1000, // 2 minutes (more frequent for invitations)
  });
}

// Create Invitation Hook
export function useCreateInvitation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ 
      email, 
      role, 
      organizationId 
    }: { 
      email: string; 
      role: 'admin' | 'member'; 
      organizationId: string; 
    }) => {
      return authProviderClient.createInvitation(email, role, organizationId);
    },
    onSuccess: (_, variables) => {
      // Invalidate organization invitations for this org
      queryClient.invalidateQueries({ 
        queryKey: authProviderKeys.organizationInvitations(variables.organizationId) 
      });
    },
    onError: (error) => {
      console.error('Invitation creation failed:', error);
    },
  });
}

// Delete Invitation Hook
export function useDeleteInvitation() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ 
      organizationId, 
      invitationId 
    }: { 
      organizationId: string; 
      invitationId: string; 
    }) => {
      return authProviderClient.deleteInvitation(organizationId, invitationId);
    },
    onSuccess: (_, variables) => {
      // Invalidate organization invitations for this org
      queryClient.invalidateQueries({ 
        queryKey: authProviderKeys.organizationInvitations(variables.organizationId) 
      });
    },
    onError: (error) => {
      console.error('Invitation deletion failed:', error);
    },
  });
}

// Passkey Credentials Hook
export function usePasskeyCredentials() {
  return useQuery({
    queryKey: authProviderKeys.passkeyCredentials(),
    queryFn: () => authProviderClient.getPasskeyCredentials(),
    staleTime: 5 * 60 * 1000, // 5 minutes
  });
}

// Register Passkey Hook
export function useRegisterPasskey() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ 
      email, 
      displayName 
    }: { 
      email: string; 
      displayName?: string; 
    }) => {
      return authProviderClient.registerPasskey(email, displayName);
    },
    onSuccess: () => {
      // Invalidate passkey credentials
      queryClient.invalidateQueries({ 
        queryKey: authProviderKeys.passkeyCredentials() 
      });
    },
    onError: (error) => {
      console.error('Passkey registration failed:', error);
    },
  });
}

// Authenticate Passkey Hook
export function useAuthenticatePasskey() {
  return useMutation({
    mutationFn: async ({ email }: { email?: string }) => {
      return authProviderClient.authenticatePasskey(email);
    },
    onSuccess: (data) => {
      if (data.success) {
        console.log('Passkey authentication successful');
        // Redirect or update auth state as needed
      }
    },
    onError: (error) => {
      console.error('Passkey authentication failed:', error);
    },
  });
}

// Delete Passkey Hook
export function useDeletePasskey() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ credentialId }: { credentialId: string }) => {
      return authProviderClient.deletePasskey(credentialId);
    },
    onSuccess: () => {
      // Invalidate passkey credentials
      queryClient.invalidateQueries({ 
        queryKey: authProviderKeys.passkeyCredentials() 
      });
    },
    onError: (error) => {
      console.error('Passkey deletion failed:', error);
    },
  });
}

// Social Providers Hook
export function useSocialProviders() {
  return useQuery({
    queryKey: authProviderKeys.socialProviders(),
    queryFn: () => authProviderClient.getSocialProviders(),
    staleTime: 30 * 60 * 1000, // 30 minutes (rarely changes)
    gcTime: 60 * 60 * 1000, // 1 hour
  });
}

// Social Authentication Hook
export function useAuthenticateSocial() {
  return useMutation({
    mutationFn: async ({ 
      provider, 
      redirectTo 
    }: { 
      provider: 'google' | 'microsoft'; 
      redirectTo?: string; 
    }) => {
      return authProviderClient.authenticateSocial(provider, redirectTo);
    },
    onSuccess: (data) => {
      if (data.redirectUrl) {
        window.location.href = data.redirectUrl;
      }
    },
    onError: (error) => {
      console.error('Social authentication failed:', error);
    },
  });
}

// Email Validation Hook (for forms)
export function useEmailValidation() {
  return useMutation({
    mutationFn: async ({ email }: { email: string }) => {
      return authProviderClient.validateEmail(email);
    },
    onError: (error) => {
      console.error('Email validation failed:', error);
    },
  });
}

// Real-time Email Availability Hook (debounced)
export function useEmailAvailability(email: string, enabled: boolean = true) {
  return useQuery({
    queryKey: authProviderKeys.emailAvailability(email),
    queryFn: () => authProviderClient.checkEmailAvailability(email),
    enabled: enabled && !!email && email.includes('@'),
    staleTime: 30 * 1000, // 30 seconds
    gcTime: 2 * 60 * 1000, // 2 minutes
    retry: 1, // Only retry once for availability checks
  });
}

// Utility Hooks
export function useAuthProviderUtils() {
  return {
    getDomainFromEmail: authProviderClient.getDomainFromEmail.bind(authProviderClient),
    isBusinessEmail: authProviderClient.isBusinessEmail.bind(authProviderClient),
  };
}

// Combined hook for auth provider data
export function useAuthProviderData() {
  const organizations = useOrganizations();
  const socialProviders = useSocialProviders();
  const passkeyCredentials = usePasskeyCredentials();

  return {
    organizations,
    socialProviders,
    passkeyCredentials,
    isLoading: organizations.isLoading || socialProviders.isLoading || passkeyCredentials.isLoading,
    isError: organizations.isError || socialProviders.isError || passkeyCredentials.isError,
  };
}

// Hook for prefetching auth provider data
export function usePrefetchAuthProviderData() {
  const queryClient = useQueryClient();

  return {
    prefetchOrganizations: () => {
      queryClient.prefetchQuery({
        queryKey: authProviderKeys.organizationsList(),
        queryFn: () => authProviderClient.getOrganizations(),
        staleTime: 5 * 60 * 1000,
      });
    },
    prefetchSocialProviders: () => {
      queryClient.prefetchQuery({
        queryKey: authProviderKeys.socialProviders(),
        queryFn: () => authProviderClient.getSocialProviders(),
        staleTime: 30 * 60 * 1000,
      });
    },
    prefetchPasskeyCredentials: () => {
      queryClient.prefetchQuery({
        queryKey: authProviderKeys.passkeyCredentials(),
        queryFn: () => authProviderClient.getPasskeyCredentials(),
        staleTime: 5 * 60 * 1000,
      });
    },
  };
}

// ===================================
// VERIFICATION HOOKS
// ===================================

// TOTP/Authenticator Hooks
export function useSetupTotp() {
  return useMutation({
    mutationFn: () => authProviderClient.setupTotp(),
    meta: {
      errorMessage: 'Failed to setup TOTP authenticator',
      successMessage: 'TOTP authenticator setup successfully',
    },
  });
}

export function useVerifyTotp() {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: (code: string) => authProviderClient.verifyTotp(code),
    onSuccess: () => {
      // Invalidate 2FA methods and user session
      queryClient.invalidateQueries({ queryKey: ['auth', 'twoFactorMethods'] });
      queryClient.invalidateQueries({ queryKey: ['auth', 'user'] });
      queryClient.invalidateQueries({ queryKey: ['auth', 'securityAudit'] });
    },
    meta: {
      errorMessage: 'Invalid TOTP code',
      successMessage: 'TOTP verification successful',
    },
  });
}

// Email OTP Hooks
export function useSendEmailOtp() {
  return useMutation({
    mutationFn: ({ 
      email, 
      purpose = 'verification' as const, 
      language = 'no' as const 
    }: {
      email: string;
      purpose?: 'verification' | 'login' | 'reset' | 'change-email';
      language?: 'no' | 'en';
    }) => authProviderClient.sendEmailOtp(email, purpose, language),
    meta: {
      errorMessage: 'Failed to send email verification code',
      successMessage: 'Verification code sent to your email',
    },
  });
}

export function useVerifyEmailOtp() {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: ({ 
      email, 
      code, 
      purpose = 'verification' as const 
    }: {
      email: string;
      code: string;
      purpose?: 'verification' | 'login' | 'reset' | 'change-email';
    }) => authProviderClient.verifyEmailOtp(email, code, purpose),
    onSuccess: () => {
      // Invalidate relevant queries based on purpose
      queryClient.invalidateQueries({ queryKey: ['auth', 'user'] });
      queryClient.invalidateQueries({ queryKey: ['auth', 'securityAudit'] });
    },
    meta: {
      errorMessage: 'Invalid email verification code',
      successMessage: 'Email verification successful',
    },
  });
}

// SMS OTP Hooks
export function useSendSmsOtp() {
  return useMutation({
    mutationFn: ({ 
      phoneNumber, 
      purpose = 'verification' as const, 
      language = 'no' as const 
    }: {
      phoneNumber: string;
      purpose?: 'verification' | 'login' | 'reset';
      language?: 'no' | 'en';
    }) => authProviderClient.sendSmsOtp(phoneNumber, purpose, language),
    meta: {
      errorMessage: 'Failed to send SMS verification code',
      successMessage: 'Verification code sent to your phone',
    },
  });
}

export function useVerifySmsOtp() {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: ({ 
      phoneNumber, 
      code, 
      purpose = 'verification' as const 
    }: {
      phoneNumber: string;
      code: string;
      purpose?: 'verification' | 'login' | 'reset';
    }) => authProviderClient.verifySmsOtp(phoneNumber, code, purpose),
    onSuccess: () => {
      // Invalidate relevant queries
      queryClient.invalidateQueries({ queryKey: ['auth', 'user'] });
      queryClient.invalidateQueries({ queryKey: ['auth', 'securityAudit'] });
    },
    meta: {
      errorMessage: 'Invalid SMS verification code',
      successMessage: 'SMS verification successful',
    },
  });
}

// 2FA Management Hooks
export function useTwoFactorMethods() {
  return useQuery({
    queryKey: ['auth', 'twoFactorMethods'],
    queryFn: () => authProviderClient.getTwoFactorMethods(),
    staleTime: 5 * 60 * 1000, // 5 minutes
    retry: (failureCount, error) => {
      // Don't retry on auth errors
      if (error instanceof Error && error.message.includes('401')) {
        return false;
      }
      return failureCount < 2;
    },
  });
}

export function useToggleTwoFactorMethod() {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: ({ methodId, enabled }: { methodId: string; enabled: boolean }) => 
      authProviderClient.toggleTwoFactorMethod(methodId, enabled),
    onSuccess: () => {
      // Invalidate 2FA methods and security audit
      queryClient.invalidateQueries({ queryKey: ['auth', 'twoFactorMethods'] });
      queryClient.invalidateQueries({ queryKey: ['auth', 'securityAudit'] });
      queryClient.invalidateQueries({ queryKey: ['auth', 'user'] });
    },
    meta: {
      errorMessage: 'Failed to update 2FA method',
      successMessage: '2FA method updated successfully',
    },
  });
}

// Recovery Codes Hooks
export function useRecoveryCodes() {
  return useQuery({
    queryKey: ['auth', 'recoveryCodes'],
    queryFn: () => authProviderClient.getRecoveryCodes(),
    staleTime: 10 * 60 * 1000, // 10 minutes - longer cache for sensitive data
    retry: (failureCount, error) => {
      // Don't retry on auth errors
      if (error instanceof Error && error.message.includes('401')) {
        return false;
      }
      return failureCount < 2;
    },
  });
}

export function useRegenerateRecoveryCodes() {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: (confirmRegenerate: boolean = true) => 
      authProviderClient.regenerateRecoveryCodes(confirmRegenerate),
    onSuccess: () => {
      // Invalidate recovery codes and security audit
      queryClient.invalidateQueries({ queryKey: ['auth', 'recoveryCodes'] });
      queryClient.invalidateQueries({ queryKey: ['auth', 'securityAudit'] });
    },
    meta: {
      errorMessage: 'Failed to regenerate recovery codes',
      successMessage: 'Recovery codes regenerated successfully',
    },
  });
}

export function useVerifyRecoveryCode() {
  const queryClient = useQueryClient();
  
  return useMutation({
    mutationFn: (code: string) => authProviderClient.verifyRecoveryCode(code),
    onSuccess: () => {
      // Invalidate recovery codes, user session, and security audit
      queryClient.invalidateQueries({ queryKey: ['auth', 'recoveryCodes'] });
      queryClient.invalidateQueries({ queryKey: ['auth', 'user'] });
      queryClient.invalidateQueries({ queryKey: ['auth', 'securityAudit'] });
    },
    meta: {
      errorMessage: 'Invalid recovery code',
      successMessage: 'Recovery code verification successful',
    },
  });
}

// Security Audit Hooks
export function useSecurityAudit({
  limit = 20,
  offset = 0,
  eventType = 'all' as const,
}: {
  limit?: number;
  offset?: number;
  eventType?: 'login' | 'logout' | '2fa-setup' | '2fa-verify' | 'password-change' | 'email-change' | 'all';
} = {}) {
  return useQuery({
    queryKey: ['auth', 'securityAudit', { limit, offset, eventType }],
    queryFn: () => authProviderClient.getSecurityAudit(limit, offset, eventType),
    staleTime: 2 * 60 * 1000, // 2 minutes - shorter cache for audit data
    retry: (failureCount, error) => {
      // Don't retry on auth errors
      if (error instanceof Error && error.message.includes('401')) {
        return false;
      }
      return failureCount < 2;
    },
  });
}

// Utility Hooks for Verification
export function useVerificationUtilities() {
  return {
    formatPhoneNumber: authProviderClient.formatPhoneNumber.bind(authProviderClient),
    maskPhoneNumber: authProviderClient.maskPhoneNumber.bind(authProviderClient),
    generateQrCodeUrl: authProviderClient.generateQrCodeUrl.bind(authProviderClient),
  };
}

// Composite Hook for 2FA Setup Flow
export function useTwoFactorSetupFlow() {
  const setupTotp = useSetupTotp();
  const verifyTotp = useVerifyTotp();
  const twoFactorMethods = useTwoFactorMethods();
  const toggleMethod = useToggleTwoFactorMethod();
  const recoveryCodes = useRecoveryCodes();
  const regenerateCodes = useRegenerateRecoveryCodes();
  const utilities = useVerificationUtilities();
  
  return {
    setupTotp,
    verifyTotp,
    twoFactorMethods,
    toggleMethod,
    recoveryCodes,
    regenerateCodes,
    utilities,
    isSetupInProgress: setupTotp.isPending || verifyTotp.isPending,
    hasError: setupTotp.isError || verifyTotp.isError || twoFactorMethods.isError,
    error: setupTotp.error || verifyTotp.error || twoFactorMethods.error,
  };
}

// Composite Hook for OTP Verification Flow
export function useOtpVerificationFlow() {
  const sendEmailOtp = useSendEmailOtp();
  const verifyEmailOtp = useVerifyEmailOtp();
  const sendSmsOtp = useSendSmsOtp();
  const verifySmsOtp = useVerifySmsOtp();
  const verifyRecoveryCode = useVerifyRecoveryCode();
  const utilities = useVerificationUtilities();
  
  return {
    sendEmailOtp,
    verifyEmailOtp,
    sendSmsOtp,
    verifySmsOtp,
    verifyRecoveryCode,
    utilities,
    isVerificationInProgress: verifyEmailOtp.isPending || verifySmsOtp.isPending || verifyRecoveryCode.isPending,
    isSendingOtp: sendEmailOtp.isPending || sendSmsOtp.isPending,
    hasError: sendEmailOtp.isError || verifyEmailOtp.isError || sendSmsOtp.isError || verifySmsOtp.isError || verifyRecoveryCode.isError,
    error: sendEmailOtp.error || verifyEmailOtp.error || sendSmsOtp.error || verifySmsOtp.error || verifyRecoveryCode.error,
  };
}

// ===================================
// PROFILE MANAGEMENT HOOKS
// ===================================

export function useUserProfile() {
  return useQuery({
    queryKey: ['auth', 'profile'],
    queryFn: () => authProviderClient.getProfile(),
    staleTime: 5 * 60 * 1000, // 5 minutes
    retry: (failureCount, error) => {
      // Don't retry on auth errors
      if (error instanceof Error && error.message.includes('401')) {
        return false;
      }
      return failureCount < 2;
    },
  });
}

export function useUpdateProfile() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: { name?: string; image?: string; metadata?: Record<string, unknown> }) =>
      authProviderClient.updateProfile(data),
    onSuccess: () => {
      // Invalidate profile and user data
      queryClient.invalidateQueries({ queryKey: ['auth', 'profile'] });
      queryClient.invalidateQueries({ queryKey: ['auth', 'user'] });
    },
    meta: {
      errorMessage: 'Failed to update profile',
      successMessage: 'Profile updated successfully',
    },
  });
}

// ===================================
// GDPR CONSENT HOOKS
// ===================================

export function useConsentPreferences() {
  return useQuery({
    queryKey: ['auth', 'consent'],
    queryFn: () => authProviderClient.getConsent(),
    staleTime: 10 * 60 * 1000, // 10 minutes
    retry: (failureCount, error) => {
      // Don't retry on auth errors
      if (error instanceof Error && error.message.includes('401')) {
        return false;
      }
      return failureCount < 2;
    },
  });
}

export function useUpdateConsentPreferences() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (data: {
      necessary: boolean;
      analytics: boolean;
      marketing: boolean;
      performance?: boolean;
      functional?: boolean;
    }) => authProviderClient.updateConsent(data),
    onSuccess: () => {
      // Invalidate consent data
      queryClient.invalidateQueries({ queryKey: ['auth', 'consent'] });
    },
    meta: {
      errorMessage: 'Failed to update consent preferences',
      successMessage: 'Consent preferences updated successfully',
    },
  });
}

export function useWithdrawConsent() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => authProviderClient.withdrawConsent(),
    onSuccess: () => {
      // Invalidate consent data and potentially redirect user
      queryClient.invalidateQueries({ queryKey: ['auth', 'consent'] });
      queryClient.invalidateQueries({ queryKey: ['auth', 'user'] });
    },
    meta: {
      errorMessage: 'Failed to withdraw consent',
      successMessage: 'All consent has been withdrawn',
    },
  });
}

// ===================================
// PASSWORD STRENGTH HOOKS
// ===================================

export function usePasswordStrengthCheck() {
  return useMutation({
    mutationFn: (password: string) => authProviderClient.checkPasswordStrength(password),
    meta: {
      errorMessage: 'Failed to check password strength',
    },
  });
}

// Real-time password strength checking (debounced)
export function usePasswordStrengthAnalysis(password: string, enabled: boolean = true) {
  return useQuery({
    queryKey: ['auth', 'passwordStrength', { password }],
    queryFn: () => authProviderClient.checkPasswordStrength(password),
    enabled: enabled && !!password && password.length >= 3, // Only check passwords with 3+ chars
    staleTime: 30 * 1000, // 30 seconds
    gcTime: 2 * 60 * 1000, // 2 minutes
    retry: 1, // Only retry once for password checks
    refetchOnWindowFocus: false, // Don't refetch on window focus for password checks
  });
}

// Export all hooks and utilities
export type AuthProviderHooks = {
  // SSO
  useDiscoverSSO: typeof useDiscoverSSO;
  useAuthenticateSSO: typeof useAuthenticateSSO;
  
  // Organizations
  useOrganizations: typeof useOrganizations;
  useCreateOrganization: typeof useCreateOrganization;
  useOrganizationMembers: typeof useOrganizationMembers;
  useOrganizationInvitations: typeof useOrganizationInvitations;
  useCreateInvitation: typeof useCreateInvitation;
  useDeleteInvitation: typeof useDeleteInvitation;
  
  // Passkeys
  usePasskeyCredentials: typeof usePasskeyCredentials;
  useRegisterPasskey: typeof useRegisterPasskey;
  useAuthenticatePasskey: typeof useAuthenticatePasskey;
  useDeletePasskey: typeof useDeletePasskey;
  
  // Social
  useSocialProviders: typeof useSocialProviders;
  useAuthenticateSocial: typeof useAuthenticateSocial;
  
  // Validation
  useEmailValidation: typeof useEmailValidation;
  useEmailAvailability: typeof useEmailAvailability;
  
  // Utilities
  useAuthProviderUtils: typeof useAuthProviderUtils;
  useAuthProviderData: typeof useAuthProviderData;
  usePrefetchAuthProviderData: typeof usePrefetchAuthProviderData;
  
  // Verification
  useSetupTotp: typeof useSetupTotp;
  useVerifyTotp: typeof useVerifyTotp;
  useSendEmailOtp: typeof useSendEmailOtp;
  useVerifyEmailOtp: typeof useVerifyEmailOtp;
  useSendSmsOtp: typeof useSendSmsOtp;
  useVerifySmsOtp: typeof useVerifySmsOtp;
  useTwoFactorMethods: typeof useTwoFactorMethods;
  useToggleTwoFactorMethod: typeof useToggleTwoFactorMethod;
  useRecoveryCodes: typeof useRecoveryCodes;
  useRegenerateRecoveryCodes: typeof useRegenerateRecoveryCodes;
  useVerifyRecoveryCode: typeof useVerifyRecoveryCode;
  useSecurityAudit: typeof useSecurityAudit;
  useVerificationUtilities: typeof useVerificationUtilities;
  useTwoFactorSetupFlow: typeof useTwoFactorSetupFlow;
  useOtpVerificationFlow: typeof useOtpVerificationFlow;

  // Profile Management
  useUserProfile: typeof useUserProfile;
  useUpdateProfile: typeof useUpdateProfile;

  // GDPR Consent
  useConsentPreferences: typeof useConsentPreferences;
  useUpdateConsentPreferences: typeof useUpdateConsentPreferences;
  useWithdrawConsent: typeof useWithdrawConsent;

  // Password Strength
  usePasswordStrengthCheck: typeof usePasswordStrengthCheck;
  usePasswordStrengthAnalysis: typeof usePasswordStrengthAnalysis;
};
