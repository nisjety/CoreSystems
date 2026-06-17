/**
 * Account hooks
 *
 * Re-exports the core profile hooks and adds account-specific mutations.
 */
export {
  useCurrentProfile,
  useUpdateProfile,
} from '@/components/core/profile/hooks/useProfile'

import { useQuery, useMutation } from '@tanstack/react-query'
import { accountService } from '../services/account-service'
import type { NotificationPreferences } from '../types'

const accountQueryKeys = {
  billing: (orgId: string) => ['account', 'billing', orgId] as const,
  quotas: (orgId: string) => ['account', 'quotas', orgId] as const,
  notifications: () => ['account', 'notifications'] as const,
}

export function useOrgBilling(orgId: string | undefined) {
  return useQuery({
    queryKey: accountQueryKeys.billing(orgId ?? ''),
    queryFn: () => accountService.getOrganizationBilling(orgId!),
    enabled: !!orgId,
    staleTime: 5 * 60 * 1000,
    retry: 1,
  })
}

export function useOrgQuotas(orgId: string | undefined) {
  return useQuery({
    queryKey: accountQueryKeys.quotas(orgId ?? ''),
    queryFn: () => accountService.getOrganizationQuotas(orgId!),
    enabled: !!orgId,
    staleTime: 5 * 60 * 1000,
    retry: 1,
  })
}

export function useNotificationPreferences() {
  return useQuery({
    queryKey: accountQueryKeys.notifications(),
    queryFn: () => accountService.getNotificationPreferences(),
    staleTime: 10 * 60 * 1000,
  })
}

export function useChangePassword() {
  return useMutation({
    mutationFn: ({
      currentPassword,
      newPassword,
    }: {
      currentPassword: string
      newPassword: string
    }) => accountService.changePassword(currentPassword, newPassword),
  })
}

export function useUpdateNotifications() {
  return useMutation({
    mutationFn: (prefs: Partial<NotificationPreferences>) =>
      accountService.updateNotificationPreferences(prefs),
  })
}

export function useDeleteAccount() {
  return useMutation({
    mutationFn: () => accountService.deleteAccount(),
  })
}
