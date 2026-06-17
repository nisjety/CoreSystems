'use client'

import Link from 'next/link'
import { Users, ArrowRight, Loader2, Crown, Shield, User } from 'lucide-react'
import { useCurrentOrganization } from '@/components/core/profile/hooks/useProfile'
import { useTeamMembers } from '@/components/team/hooks/useTeam'

const ROLE_LABELS: Record<string, string> = {
  owner:  'Owner',
  admin:  'Admin',
  member: 'Member',
  viewer: 'Viewer',
}

const ROLE_ICON: Record<string, React.ComponentType<{ size?: number; strokeWidth?: number; className?: string }>> = {
  owner:  Crown,
  admin:  Shield,
  member: User,
  viewer: User,
}

export function MembersSection() {
  const { data: org, isLoading: orgLoading } = useCurrentOrganization()
  const { data: members, isLoading: membersLoading } = useTeamMembers(org?.id)

  const isLoading = orgLoading || membersLoading

  return (
    <section id="members" className="scroll-mt-6">
      <div className="mb-4 flex items-start justify-between gap-4">
        <div>
          <h2 className="font-inter text-[22px] font-semibold tracking-[-0.02em] text-[#1C1C1A]">
            Members
          </h2>
          <p className="mt-0.5 font-inter text-[13px] text-[#9B9691]">
            {members ? `${members.length} member${members.length !== 1 ? 's' : ''}` : 'Manage who has access'}
          </p>
        </div>
        <Link
          href="/team"
          className="flex shrink-0 items-center gap-1.5 rounded-xl border border-[#D8D2C6] bg-white px-3.5 py-2 font-inter text-[12px] font-medium text-[#4A4A48] transition-colors hover:border-[#1C1C1A] hover:text-[#1C1C1A]"
        >
          Manage team
          <ArrowRight size={13} strokeWidth={1.8} />
        </Link>
      </div>

      {isLoading ? (
        <div className="flex items-center gap-2 font-inter text-[13px] text-[#9B9691]">
          <Loader2 size={14} className="animate-spin" /> Loading…
        </div>
      ) : members && members.length > 0 ? (
        <div className="overflow-hidden rounded-[10px] border border-[#E4E1DC] bg-white">
          {members.slice(0, 5).map((member, i) => {
            const RoleIcon = ROLE_ICON[member.role] ?? User
            const initial = (member.displayName ?? member.email ?? '?').charAt(0).toUpperCase()
            const isLast = i === Math.min(4, members.length - 1)

            return (
              <div
                key={member.userId}
                className={[
                  'flex items-center gap-3 px-4 py-3',
                  !isLast ? 'border-b border-[#F4F1EB]' : '',
                ].join(' ')}
              >
                <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-[#EAE6DF] font-inter text-[12px] font-semibold text-[#4A4A48]">
                  {initial}
                </div>
                <div className="min-w-0 flex-1">
                  <div className="truncate font-inter text-[13px] font-medium text-[#1C1C1A]">
                    {member.displayName ?? member.email}
                  </div>
                  {member.displayName && (
                    <div className="truncate font-inter text-[11px] text-[#9B9691]">
                      {member.email}
                    </div>
                  )}
                </div>
                <div className="flex shrink-0 items-center gap-1.5 font-inter text-[11px] text-[#9B9691]">
                  <RoleIcon size={12} strokeWidth={1.6} />
                  {ROLE_LABELS[member.role] ?? member.role}
                </div>
              </div>
            )
          })}

          {members.length > 5 && (
            <div className="border-t border-[#F4F1EB] px-4 py-2.5">
              <Link
                href="/team"
                className="font-inter text-[12px] text-[#9B9691] transition-colors hover:text-[#1C1C1A]"
              >
                +{members.length - 5} more — view all
              </Link>
            </div>
          )}
        </div>
      ) : (
        <div className="rounded-xl border border-dashed border-[#D8D2C6] px-4 py-6 text-center">
          <Users size={18} className="mx-auto mb-2 text-[#C8C1B3]" />
          <p className="font-inter text-[13px] text-[#9B9691]">
            No members yet.{' '}
            <Link href="/team" className="underline underline-offset-2 hover:text-[#1C1C1A]">
              Invite your team
            </Link>
          </p>
        </div>
      )}
    </section>
  )
}
