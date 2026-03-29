import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { profileService } from '../lib/profile-service';
import { orgService } from '@/lib/services';
import type { UpdateProfilePayload } from '../types';

export const profileKeys = {
  all: ['profile'] as const,
  current: () => [...profileKeys.all, 'current'] as const,
  providers: () => [...profileKeys.all, 'providers'] as const,
  organizations: () => [...profileKeys.all, 'organizations'] as const,
  currentOrg: () => [...profileKeys.all, 'currentOrg'] as const,
};

/** Fetch current user's full profile from user-service */
export function useCurrentProfile() {
  return useQuery({
    queryKey: profileKeys.current(),
    queryFn: () => profileService.getCurrentUser(),
    staleTime: 5 * 60 * 1000,
    retry: (failCount, err) => {
      const e = err as { status?: number };
      if (e?.status === 401 || e?.status === 403) return false;
      return failCount < 2;
    },
    enabled: typeof window !== 'undefined',
    refetchOnWindowFocus: false,
  });
}

/** Fetch linked OAuth providers for the current user */
export function useLinkedProviders() {
  return useQuery({
    queryKey: profileKeys.providers(),
    queryFn: () => profileService.getLinkedProviders(),
    select: (data) => (Array.isArray(data) ? data : []),
    staleTime: 10 * 60 * 1000,
    retry: 1,
    enabled: typeof window !== 'undefined',
    refetchOnWindowFocus: false,
  });
}

/** Mutation to update profile fields */
export function useUpdateProfile() {
  const qc = useQueryClient();

  return useMutation({
    mutationFn: (payload: UpdateProfilePayload) => profileService.updateProfile(payload),
    onSuccess: () => {
      // Invalidate profile cache so UI reflects changes immediately
      qc.invalidateQueries({ queryKey: profileKeys.current() });
      // Also refresh sidebar user data (avatar, name may have changed)
      qc.invalidateQueries({ queryKey: ['user'] });
    },
  });
}

/** Fetch all organizations the current user is a member of */
export function useUserOrganizations() {
  return useQuery({
    queryKey: profileKeys.organizations(),
    queryFn: async () => {
      const profile = await profileService.getCurrentUser();
      const orgs = await orgService.getMyOrganizations(profile?.id);
      const seen = new Set<string>();
      return orgs.filter((org) => {
        const key = (org.slug || org.id || '').toLowerCase();
        if (!key || seen.has(key)) {
          return false;
        }
        seen.add(key);
        return true;
      });
    },
    staleTime: 5 * 60 * 1000,
    retry: 1,
    enabled: typeof window !== 'undefined',
    refetchOnWindowFocus: false,
  });
}

/** Fetch the user's current/active organization (first org from the list) */
export function useCurrentOrganization() {
  return useQuery({
    queryKey: profileKeys.currentOrg(),
    queryFn: async () => {
      const profile = await profileService.getCurrentUser();
      const orgs = await orgService.getMyOrganizations(profile?.id);
      // Return the first organization as the "current" one
      // TODO: Implement proper active organization tracking in org-core
      return orgs.length > 0 ? orgs[0] : null;
    },
    staleTime: 5 * 60 * 1000,
    retry: 1,
    enabled: typeof window !== 'undefined',
    refetchOnWindowFocus: false,
  });
}

/** Encapsulated modal state for profile modal UI */
export function useProfileModal(options?: { onOpen?: () => void; onClose?: () => void }) {
  const [isOpen, setIsOpen] = useState(false);

  const openProfileModal = () => {
    options?.onOpen?.();
    setIsOpen(true);
  };

  const closeProfileModal = () => {
    setIsOpen(false);
    options?.onClose?.();
  };

  return {
    isProfileModalOpen: isOpen,
    openProfileModal,
    closeProfileModal,
  };
}
