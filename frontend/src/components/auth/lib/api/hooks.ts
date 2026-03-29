import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';
import { 
  authORPCClient, 
  type SignInData, 
  type SignUpData, 
  type ResetPasswordData,
  type UpdateProfileData,
  type Enable2FAData,
  type SSOAuthData,
  type CreateOrganizationData
} from '../orpc/client';

// Result type helpers – refine "any" usages
interface AuthResultBase {
  success?: boolean;
  requires2FA?: boolean;
  requiresVerification?: boolean;
  redirectUrl?: string;
  user?: unknown; // Replace with concrete User type if available
}

type SignInResult = AuthResultBase & { user?: unknown };
type SignUpResult = AuthResultBase & { user?: unknown };
type SSOResult = AuthResultBase & { redirectUrl?: string };
interface Enable2FAResult extends AuthResultBase { qrCodeDataUrl?: string }
interface CreateOrganizationResult extends AuthResultBase { organizationId?: string }
import { useI18n } from '../../hooks/use-i18n';

// Enhanced query keys with better organization
export const queryKeys = {
  auth: {
    session: ['auth', 'session'] as const,
    profile: ['auth', 'profile'] as const,
  },
  user: {
    profile: ['user', 'profile'] as const,
    organizations: ['user', 'organizations'] as const,
  },
  validation: {
    emailAvailability: (email: string) => ['validation', 'email', email] as const,
  },
} as const;

// Enhanced authentication hooks with ORPC integration

// Profile management hooks
export function useUserProfile() {
  return useQuery({
    queryKey: queryKeys.user.profile,
    queryFn: () => authORPCClient.getProfile(),
    staleTime: 5 * 60 * 1000, // 5 minutes
    retry: 1,
    retryOnMount: false,
  });
}

export function useUpdateProfile() {
  const queryClient = useQueryClient();
  const { t } = useI18n();

  return useMutation({
    mutationFn: (data: UpdateProfileData) => authORPCClient.updateProfile(data),
    onSuccess: (updatedUser) => {
      // Update cached profile data
      queryClient.setQueryData(queryKeys.user.profile, updatedUser);
      toast.success(t('toasts.success.profileUpdated'));
    },
    onError: (error) => {
      console.error('Profile update error:', error);
      toast.error(error instanceof Error ? error.message : t('toasts.errors.serverError'));
    },
  });
}

// Authentication hooks
export function useSignIn() {
  const queryClient = useQueryClient();
  const { t } = useI18n();

  return useMutation({
    mutationFn: (data: SignInData) => authORPCClient.signIn(data),
  onSuccess: (result: SignInResult) => {
      if (result.success && result.user) {
        // Update cached user data
        queryClient.setQueryData(queryKeys.user.profile, result.user);
        toast.success(t('toasts.success.authSuccess'));
        
        // Redirect to dashboard or intended page
        window.location.href = '/dashboard';
      } else if (result.requires2FA) {
        toast.info('2FA authentication required');
      } else if (result.requiresVerification) {
        toast.warning('Please verify your email address');
      }
    },
    onError: (error) => {
      console.error('Sign-in error:', error);
      toast.error(error instanceof Error ? error.message : t('toasts.errors.authFailed'));
    },
  });
}

export function useSignUp() {
  const queryClient = useQueryClient();
  const { t } = useI18n();

  return useMutation({
    mutationFn: (data: SignUpData) => authORPCClient.signUp(data),
  onSuccess: (result: SignUpResult) => {
      if (result.success && result.user) {
        // Update cached user data
        queryClient.setQueryData(queryKeys.user.profile, result.user);
        
        if (result.requiresVerification) {
          toast.success('Account created! Please verify your email address.');
        } else {
          toast.success(t('toasts.success.authSuccess'));
          window.location.href = '/dashboard';
        }
      }
    },
    onError: (error) => {
      console.error('Sign-up error:', error);
      toast.error(error instanceof Error ? error.message : t('toasts.errors.authFailed'));
    },
  });
}

export function useResetPassword() {
  const { t } = useI18n();

  return useMutation({
    mutationFn: (data: ResetPasswordData) => authORPCClient.resetPassword(data),
    onSuccess: () => {
      toast.success('Password reset link sent to your email');
    },
    onError: (error) => {
      console.error('Reset password error:', error);
      toast.error(error instanceof Error ? error.message : t('toasts.errors.serverError'));
    },
  });
}

export function useSignOut() {
  const queryClient = useQueryClient();
  const { t } = useI18n();

  return useMutation({
    mutationFn: () => authORPCClient.signOut(),
    onSuccess: () => {
      // Clear all cached data
      queryClient.clear();
      toast.success('Successfully signed out');
      window.location.href = '/sign-in';
    },
    onError: (error) => {
      console.error('Sign-out error:', error);
      toast.error(error instanceof Error ? error.message : t('toasts.errors.serverError'));
    },
  });
}

// SSO authentication
export function useSSO() {
  return useMutation({
    mutationFn: (data: SSOAuthData) => authORPCClient.authenticateSSO(data),
  onSuccess: (result: SSOResult) => {
      if (result.success && result.redirectUrl) {
        toast.info('Redirecting to SSO provider...');
        window.location.href = result.redirectUrl;
      }
    },
    onError: (error) => {
      console.error('SSO error:', error);
      toast.error(error instanceof Error ? error.message : 'SSO authentication failed');
    },
  });
}

// Social authentication
export function useSocialAuth() {
  return useMutation({
    mutationFn: (provider: 'google' | 'microsoft') => 
      authORPCClient.signInWithSocial(provider),
  onSuccess: (result: SSOResult) => {
      if (result.success && result.redirectUrl) {
        window.location.href = result.redirectUrl;
      }
    },
    onError: (error) => {
      console.error('Social auth error:', error);
      toast.error(error instanceof Error ? error.message : 'Social authentication failed');
    },
  });
}

// Organization management
export function useCreateOrganization() {
  const queryClient = useQueryClient();
  const { t } = useI18n();

  return useMutation({
    mutationFn: (data: CreateOrganizationData) => authORPCClient.createOrganization(data),
  onSuccess: (result: CreateOrganizationResult) => {
      if (result.success) {
        // Invalidate organizations list
        queryClient.invalidateQueries({ queryKey: queryKeys.user.organizations });
        toast.success(t('toasts.success.organizationCreated'));
      }
    },
    onError: (error) => {
      console.error('Organization creation error:', error);
      toast.error(error instanceof Error ? error.message : 'Organization creation failed');
    },
  });
}

// 2FA management hooks
export function useEnable2FA() {
  return useMutation({
    mutationFn: (data: Enable2FAData) => authORPCClient.enable2FA(data),
  onSuccess: (result: Enable2FAResult) => {
      if (result.success) {
        toast.success('2FA enabled successfully');
        return result; // Return result for QR code handling
      }
    },
    onError: (error) => {
      console.error('2FA enable error:', error);
      toast.error(error instanceof Error ? error.message : '2FA enable failed');
    },
  });
}

export function useDisable2FA() {
  return useMutation({
    mutationFn: () => authORPCClient.disable2FA(),
    onSuccess: () => {
      toast.success('2FA disabled successfully');
    },
    onError: (error) => {
      console.error('2FA disable error:', error);
      toast.error(error instanceof Error ? error.message : '2FA disable failed');
    },
  });
}

// Email verification
export function useResendEmailVerification() {
  return useMutation({
    mutationFn: () => authORPCClient.resendEmailVerification(),
    onSuccess: () => {
      toast.success('Verification email sent');
    },
    onError: (error) => {
      console.error('Email verification error:', error);
      toast.error(error instanceof Error ? error.message : 'Failed to send verification email');
    },
  });
}

// Password change
export function useChangePassword() {
  const { t } = useI18n();

  return useMutation({
    mutationFn: ({ currentPassword, newPassword }: { 
      currentPassword: string; 
      newPassword: string 
    }) => authORPCClient.changePassword(currentPassword, newPassword),
    onSuccess: () => {
      toast.success(t('toasts.success.passwordChanged'));
    },
    onError: (error) => {
      console.error('Password change error:', error);
      toast.error(error instanceof Error ? error.message : 'Password change failed');
    },
  });
}

// Real-time email availability validation
export function useEmailAvailability(email: string, enabled: boolean = true) {
  return useQuery({
    queryKey: queryKeys.validation.emailAvailability(email),
    queryFn: () => authORPCClient.checkEmailAvailability(email),
    enabled: enabled && !!email && email.includes('@'),
    staleTime: 30 * 1000, // 30 seconds
    retry: 1,
    refetchOnWindowFocus: false,
  });
}

// Enhanced form mutation hook with analytics
export function useFormMutation<TData, TResult>(
  mutationFn: (data: TData) => Promise<TResult>,
  options?: {
    onSuccess?: (data: TResult, variables: TData) => void;
    onError?: (error: Error, variables: TData) => void;
    onMutate?: (variables: TData) => void;
    trackingId?: string;
  }
) {
  return useMutation({
    mutationFn,
    onMutate: (variables) => {
      // Analytics tracking for form interactions
      if (options?.trackingId) {
        // Track form submission attempt
        console.log(`Form analytics: ${options.trackingId} - mutation started`, variables);
      }
      options?.onMutate?.(variables);
    },
    onSuccess: (data, variables) => {
      // Analytics tracking for successful form submission
      if (options?.trackingId) {
        console.log(`Form analytics: ${options.trackingId} - mutation success`, { data, variables });
      }
      options?.onSuccess?.(data, variables);
    },
    onError: (error, variables) => {
      // Analytics tracking for form errors
      if (options?.trackingId) {
        console.error(`Form analytics: ${options.trackingId} - mutation error`, { error, variables });
      }
      options?.onError?.(error as Error, variables);
    },
  });
}
