import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { teamService } from '../services/team-service'
import { useCurrentOrganization } from '@/components/core/profile/hooks/useProfile'
import type { MemberRole } from '../services/team-service'

export { useCurrentOrganization }

export const teamQueryKeys = {
  members: (orgId: string) => ['team', 'members', orgId] as const,
}

export function useTeamMembers(orgId: string | undefined) {
  return useQuery({
    queryKey: teamQueryKeys.members(orgId ?? ''),
    queryFn:  () => teamService.getMembers(orgId!),
    enabled:  !!orgId,
    staleTime: 2 * 60 * 1000,
    retry: 1,
  })
}

export function useInviteMember(orgId: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ email, role }: { email: string; role: MemberRole }) =>
      teamService.inviteMember(orgId!, email, role),
    onSuccess: () => {
      if (orgId) qc.invalidateQueries({ queryKey: teamQueryKeys.members(orgId) })
    },
  })
}

export function useRemoveMember(orgId: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (userId: string) => teamService.removeMember(orgId!, userId),
    onSuccess: () => {
      if (orgId) qc.invalidateQueries({ queryKey: teamQueryKeys.members(orgId) })
    },
  })
}

export function useUpdateMemberRole(orgId: string | undefined) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ userId, role }: { userId: string; role: MemberRole }) =>
      teamService.updateMemberRole(orgId!, userId, role),
    onSuccess: () => {
      if (orgId) qc.invalidateQueries({ queryKey: teamQueryKeys.members(orgId) })
    },
  })
}
