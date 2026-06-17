'use client'

import { useState } from 'react'
import { MemberRow } from './MemberRow'
import type { TeamMemberDetail, MemberRole } from './services/team-service'

interface MemberGroupSectionProps {
  title: string
  description: string
  members: TeamMemberDetail[]
  currentUserId?: string
  onRemove: (userId: string) => void
  onEditRole: (userId: string, role: MemberRole) => void
}

export function MemberGroupSection({
  title,
  description,
  members,
  currentUserId,
  onRemove,
  onEditRole,
}: MemberGroupSectionProps) {
  const [selected, setSelected] = useState<Set<string>>(new Set())

  const allSelected = members.length > 0 && selected.size === members.length
  const someSelected = selected.size > 0 && !allSelected

  const toggleAll = () => {
    setSelected(allSelected ? new Set() : new Set(members.map(m => m.userId)))
  }

  const toggleOne = (userId: string, checked: boolean) => {
    setSelected(prev => {
      const next = new Set(prev)
      checked ? next.add(userId) : next.delete(userId)
      return next
    })
  }

  return (
    <div className="grid grid-cols-1 gap-6 md:grid-cols-[240px_1fr]">
      {/* ── Left: description ───────────────────────────────────── */}
      <div className="pt-1">
        <h3 className="font-inter text-[14px] font-semibold text-[#1C1C1A]">{title}</h3>
        <p className="mt-1.5 font-inter text-[12px] leading-relaxed text-[#9B9691]">
          {description}
        </p>
      </div>

      {/* ── Right: table ────────────────────────────────────────── */}
      <div className="overflow-hidden rounded-[10px] border border-[#E4E1DC]">
        {members.length === 0 ? (
          <div className="px-6 py-10 text-center">
            <p className="font-inter text-[13px] text-[#C8C1B3]">No members in this group.</p>
          </div>
        ) : (
          <table className="w-full border-collapse">
            <thead>
              <tr className="border-b border-[#E4E1DC] bg-[#FAFAF8]">
                <th className="w-10 px-4 py-3">
                  <input
                    type="checkbox"
                    checked={allSelected}
                    ref={el => { if (el) el.indeterminate = someSelected }}
                    onChange={toggleAll}
                    className="h-3.5 w-3.5 cursor-pointer accent-[#1C1C1A]"
                  />
                </th>
                <th className="py-3 pr-4 text-left font-inter text-[10px] font-medium uppercase tracking-widest text-[#9B9691]">
                  Name
                </th>
                <th className="py-3 pr-6 text-left font-inter text-[10px] font-medium uppercase tracking-widest text-[#9B9691]">
                  Date added
                </th>
                <th className="py-3 pr-6 text-left font-inter text-[10px] font-medium uppercase tracking-widest text-[#9B9691]">
                  Last active
                </th>
                <th className="py-3 pr-4 text-left font-inter text-[10px] font-medium uppercase tracking-widest text-[#9B9691]">
                  Role
                </th>
                <th className="py-3 pr-4" />
              </tr>
            </thead>
            <tbody>
              {members.map(m => (
                <MemberRow
                  key={m.userId}
                  member={m}
                  selected={selected.has(m.userId)}
                  onSelect={checked => toggleOne(m.userId, checked)}
                  onRemove={() => onRemove(m.userId)}
                  onEditRole={role => onEditRole(m.userId, role)}
                  isCurrentUser={m.userId === currentUserId}
                />
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  )
}
