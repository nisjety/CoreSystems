'use client'

import { useState } from 'react'
import { Trash2, Pencil, MoreHorizontal } from 'lucide-react'
import Image from 'next/image'
import { getAvatarGradient } from '@/components/core/sidebar/utils'
import type { TeamMemberDetail, MemberRole } from './services/team-service'

const ROLE_LABELS: Record<string, string> = {
  owner:  'Owner',
  admin:  'Admin',
  member: 'Member',
  viewer: 'Viewer',
}

interface MemberRowProps {
  member: TeamMemberDetail
  selected: boolean
  onSelect: (checked: boolean) => void
  onRemove: () => void
  onEditRole: (role: MemberRole) => void
  isCurrentUser?: boolean
}

export function MemberRow({
  member,
  selected,
  onSelect,
  onRemove,
  onEditRole,
  isCurrentUser,
}: MemberRowProps) {
  const [roleMenuOpen, setRoleMenuOpen] = useState(false)
  const [avatarFailed, setAvatarFailed] = useState(false)

  const name = member.displayName || member.email || member.userId
  const gradient = getAvatarGradient(name)
  const initials = name
    .split(/\s+/)
    .map(w => w[0] ?? '')
    .join('')
    .slice(0, 2)
    .toUpperCase()

  const dateAdded = new Date(member.joinedAt).toLocaleDateString('en-GB', {
    day: '2-digit', month: 'short', year: 'numeric',
  })

  const lastActive = member.lastActive
    ? new Date(member.lastActive).toLocaleDateString('en-GB', {
        day: '2-digit', month: 'short', year: 'numeric',
      })
    : '—'

  const ROLES: MemberRole[] = ['owner', 'admin', 'member', 'viewer']

  return (
    <tr className="group border-b border-[#E4E1DC] bg-white hover:bg-[#FAFAF8] transition-colors">
      {/* Checkbox */}
      <td className="w-10 px-4 py-3.5">
        <input
          type="checkbox"
          checked={selected}
          onChange={e => onSelect(e.target.checked)}
          className="h-3.5 w-3.5 cursor-pointer accent-[#1C1C1A]"
        />
      </td>

      {/* Avatar + name + email */}
      <td className="min-w-0 py-3.5 pr-4">
        <div className="flex items-center gap-3 min-w-0">
          <div
            className={`relative flex h-8 w-8 shrink-0 items-center justify-center overflow-hidden rounded-full text-white font-semibold text-[12px] bg-linear-to-br ${gradient}`}
          >
            {member.avatarUrl && !avatarFailed ? (
              <Image
                src={member.avatarUrl}
                alt={name}
                fill
                unoptimized
                sizes="32px"
                className="object-cover"
                onError={() => setAvatarFailed(true)}
              />
            ) : (
              initials
            )}
          </div>
          <div className="min-w-0">
            <p className="font-inter text-[13px] font-medium text-[#1C1C1A] truncate">
              {member.displayName || member.userId}
              {isCurrentUser && (
                <span className="ml-1.5 font-normal text-[#9B9691]">(you)</span>
              )}
            </p>
            {member.email && (
              <p className="font-inter text-[11px] text-[#9B9691] truncate">{member.email}</p>
            )}
          </div>
        </div>
      </td>

      {/* Date added */}
      <td className="whitespace-nowrap py-3.5 pr-6 font-inter text-[12px] text-[#9B9691]">
        {dateAdded}
      </td>

      {/* Last active */}
      <td className="whitespace-nowrap py-3.5 pr-6 font-inter text-[12px] text-[#9B9691]">
        {lastActive}
      </td>

      {/* Role (editable) */}
      <td className="relative py-3.5 pr-4">
        <div className="relative inline-block">
          <button
            onClick={() => setRoleMenuOpen(o => !o)}
            className="flex items-center gap-1 rounded-[6px] border border-[#E4E1DC] bg-[#F8F7F4] px-2.5 py-1 font-inter text-[11px] text-[#4A4A48] hover:border-[#C8C1B3] transition-colors"
          >
            {ROLE_LABELS[member.role] ?? member.role}
            <MoreHorizontal size={11} className="text-[#C8C1B3]" />
          </button>
          {roleMenuOpen && (
            <div className="absolute left-0 top-full z-10 mt-1 min-w-[100px] rounded-xl border border-[#E4E1DC] bg-white py-1 shadow-[0_8px_24px_rgba(0,0,0,0.08)]">
              {ROLES.map(r => (
                <button
                  key={r}
                  onClick={() => { onEditRole(r); setRoleMenuOpen(false) }}
                  className={`block w-full px-3 py-2 text-left font-inter text-[12px] transition-colors hover:bg-[#F4F1EB] ${member.role === r ? 'text-[#1C1C1A] font-semibold' : 'text-[#4A4A48]'}`}
                >
                  {ROLE_LABELS[r]}
                </button>
              ))}
            </div>
          )}
        </div>
      </td>

      {/* Actions */}
      <td className="py-3.5 pr-4">
        <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
          <button
            onClick={onRemove}
            className="flex h-7 w-7 items-center justify-center rounded-[6px] text-[#C8C1B3] hover:bg-[#FFF0ED] hover:text-[#C0402A] transition-colors"
            title="Remove member"
          >
            <Trash2 size={13} />
          </button>
          <button
            className="flex h-7 w-7 items-center justify-center rounded-[6px] text-[#C8C1B3] hover:bg-[#F4F1EB] hover:text-[#1C1C1A] transition-colors"
            title="Edit member"
          >
            <Pencil size={13} />
          </button>
        </div>
      </td>
    </tr>
  )
}
