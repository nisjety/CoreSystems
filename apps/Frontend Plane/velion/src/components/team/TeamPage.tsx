'use client'

import { useState } from 'react'
import { UserPlus, Loader2, Users, AlertCircle } from 'lucide-react'
import { MemberGroupSection } from './MemberGroupSection'
import { AddMemberModal } from './AddMemberModal'
import {
  useCurrentOrganization,
  useTeamMembers,
  useInviteMember,
  useRemoveMember,
  useUpdateMemberRole,
} from './hooks/useTeam'
import type { MemberRole, TeamMemberDetail } from './services/team-service'

const ADMIN_ROLES  = new Set(['owner', 'admin'])
const MEMBER_ROLES = new Set(['member', 'viewer'])

export function TeamPage() {
  const [showModal, setShowModal] = useState(false)

  const { data: org, isLoading: orgLoading } = useCurrentOrganization()
  const { data: members, isLoading: membersLoading, isError } = useTeamMembers(org?.id)

  const inviteMember     = useInviteMember(org?.id)
  const removeMember     = useRemoveMember(org?.id)
  const updateMemberRole = useUpdateMemberRole(org?.id)

  const isLoading = orgLoading || membersLoading

  const admins  = (members ?? []).filter((m: TeamMemberDetail) => ADMIN_ROLES.has(m.role))
  const regular = (members ?? []).filter((m: TeamMemberDetail) => MEMBER_ROLES.has(m.role))

  const handleInvite = async (email: string, role: MemberRole) => {
    await inviteMember.mutateAsync({ email, role })
  }

  const handleRemove = (userId: string) => removeMember.mutate(userId)

  const handleEditRole = (userId: string, role: MemberRole) =>
    updateMemberRole.mutate({ userId, role })

  return (
    <div className="h-full overflow-y-auto bg-[#F8F7F4]">
      {/* ── Header ──────────────────────────────────────────────────── */}
      <div className="border-b border-[#E4E1DC] bg-white px-6 py-6 md:px-10">
        <div className="mx-auto flex max-w-4xl items-start justify-between gap-4">
          <div>
            <div className="mb-1 flex items-center gap-2">
              <Users size={16} className="text-[#9B9691]" />
              <span className="font-inter text-[11px] uppercase tracking-[0.12em] text-[#9B9691]">
                Team management
              </span>
            </div>
            <h1 className="font-inter text-[26px] font-semibold tracking-[-0.02em] text-[#1C1C1A]">
              Team members
            </h1>
            <p className="mt-1 font-inter text-[13px] text-[#9B9691]">
              Manage your team members and their account permissions here.
            </p>
          </div>

          <button
            onClick={() => setShowModal(true)}
            className="flex shrink-0 items-center gap-2 rounded-xl border border-[#D8D2C6] bg-white px-4 py-2.5 font-inter text-[13px] font-medium text-[#1C1C1A] shadow-[0_1px_2px_rgba(0,0,0,0.04)] transition-colors hover:border-[#1C1C1A] hover:bg-[#F4F1EB]"
          >
            <UserPlus size={14} />
            Add team member
          </button>
        </div>
      </div>

      {/* ── Content ─────────────────────────────────────────────────── */}
      <div className="mx-auto max-w-4xl space-y-10 px-6 py-8 md:px-10 md:py-10">

        {isLoading && (
          <div className="flex items-center gap-2 text-[#9B9691] font-inter text-[13px]">
            <Loader2 size={14} className="animate-spin" /> Loading members…
          </div>
        )}

        {isError && (
          <div className="flex items-center gap-2 rounded-xl bg-[#FFF0ED] px-4 py-3 font-inter text-[13px] text-[#C0402A]">
            <AlertCircle size={14} />
            Failed to load team members. Please refresh.
          </div>
        )}

        {!isLoading && !isError && (
          <>
            <MemberGroupSection
              title="Admin users"
              description="Admins can add and remove users and manage organisation-level settings."
              members={admins}
              onRemove={handleRemove}
              onEditRole={handleEditRole}
            />

            <div className="h-px bg-[#E4E1DC]" />

            <MemberGroupSection
              title="Account users"
              description="Account users can access workspace features, collaborate, and view shared content."
              members={regular}
              onRemove={handleRemove}
              onEditRole={handleEditRole}
            />
          </>
        )}

        {!isLoading && !isError && members?.length === 0 && (
          <div className="rounded-[10px] border border-dashed border-[#D8D2C6] py-16 text-center">
            <Users size={32} className="mx-auto mb-3 text-[#D8D2C6]" />
            <p className="font-inter text-[14px] font-medium text-[#4A4A48]">No team members yet</p>
            <p className="mt-1 font-inter text-[12px] text-[#9B9691]">
              Invite your first team member to get started.
            </p>
            <button
              onClick={() => setShowModal(true)}
              className="mt-5 flex items-center gap-2 rounded-xl bg-[#1C1C1A] px-5 py-2.5 font-inter text-[13px] font-medium text-white transition-colors hover:bg-[#383530] mx-auto"
            >
              <UserPlus size={14} />
              Add team member
            </button>
          </div>
        )}
      </div>

      {/* ── Invite modal ────────────────────────────────────────────── */}
      {showModal && (
        <AddMemberModal
          onClose={() => setShowModal(false)}
          onInvite={handleInvite}
        />
      )}
    </div>
  )
}
