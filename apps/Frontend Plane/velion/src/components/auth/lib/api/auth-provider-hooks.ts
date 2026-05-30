import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { authProviderClient } from './auth-provider-client';

const authProviderKeys = {
  ssoDiscovery: (email: string, domain?: string) =>
    ['authProvider', 'sso', 'discovery', { email, domain }] as const,
  organizationsList: () => ['authProvider', 'organizations', 'list'] as const,
  organizationInvitations: (organizationId: string) =>
    ['authProvider', 'organizations', organizationId, 'invitations'] as const,
  emailAvailability: (email: string) =>
    ['authProvider', 'validation', 'availability', { email }] as const,
  profile: () => ['authProvider', 'profile'] as const,
  passwordStrength: (password: string) =>
    ['authProvider', 'passwordStrength', { password }] as const,
} as const;

// SSO Discovery Hook
function useDiscoverSSO() {
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
function useAuthenticateSSO() {
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

// Real-time Email Availability Hook (debounced)
function useEmailAvailability(email: string, enabled: boolean = true) {
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
function useAuthProviderUtils() {
  return {
    getDomainFromEmail: authProviderClient.getDomainFromEmail.bind(authProviderClient),
    isBusinessEmail: authProviderClient.isBusinessEmail.bind(authProviderClient),
  };
}

// ===================================
// PROFILE MANAGEMENT HOOKS
// ===================================

function useUserProfile() {
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

function useUpdateProfile() {
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

// Real-time password strength checking (debounced)
function usePasswordStrengthAnalysis(password: string, enabled: boolean = true) {
  return useQuery({
    queryKey: authProviderKeys.passwordStrength(password),
    queryFn: () => authProviderClient.checkPasswordStrength(password),
    enabled: enabled && !!password && password.length >= 3, // Only check passwords with 3+ chars
    staleTime: 30 * 1000, // 30 seconds
    gcTime: 2 * 60 * 1000, // 2 minutes
    retry: 1, // Only retry once for password checks
    refetchOnWindowFocus: false, // Don't refetch on window focus for password checks
  });
}
