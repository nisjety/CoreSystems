'use client';

import { RoleBadge } from './RoleBadge';
import { MoreHorizontal } from 'lucide-react';

type Role = 'owner' | 'admin' | 'member' | 'viewer';

export interface TeamMember {
  id: string;
  name: string;
  email: string;
  role: Role;
  joinedAt: string;
  avatarUrl?: string;
}

interface MembersTableProps {
  members: TeamMember[];
  currentUserId?: string;
  onChangeRole?: (memberId: string, role: Role) => void;
  onRemove?: (memberId: string) => void;
}

export function MembersTable({ members, currentUserId, onChangeRole, onRemove }: MembersTableProps) {
  return (
    <div className="border border-[#D8D2C6]">
      {/* Header */}
      <div className="grid grid-cols-[minmax(0,1fr)_140px_120px_32px] gap-4 border-b border-[#D8D2C6] px-5 py-2.5">
        {['Navn', 'Rolle', 'Dato', ''].map((col) => (
          <span key={col} className="font-inter text-[10px] uppercase tracking-widest text-[#C8C1B3]">
            {col}
          </span>
        ))}
      </div>

      {members.map((m, i) => {
        const initials = m.name
          .split(' ')
          .map((w) => w[0])
          .join('')
          .slice(0, 2)
          .toUpperCase();
        const isSelf = m.id === currentUserId;

        return (
          <div
            key={m.id}
            className={`grid grid-cols-[minmax(0,1fr)_140px_120px_32px] items-center gap-4 px-5 py-4 ${i > 0 ? 'border-t border-[#D8D2C6]' : ''}`}
          >
            {/* Name + email */}
            <div className="flex items-center gap-3 min-w-0">
              {m.avatarUrl ? (
                // eslint-disable-next-line @next/next/no-img-element
                <img
                  src={m.avatarUrl}
                  alt={m.name}
                  className="h-8 w-8 shrink-0 border border-[#D8D2C6] object-cover"
                />
              ) : (
                <div className="flex h-8 w-8 shrink-0 items-center justify-center border border-[#D8D2C6] bg-[#EAE6DF] font-inter text-[11px] font-medium text-[#4A4A48]">
                  {initials}
                </div>
              )}
              <div className="min-w-0">
                <p className="font-inter text-[13px] font-medium text-[#2B2B2B] truncate">
                  {m.name} {isSelf && <span className="text-[#C8C1B3]">(deg)</span>}
                </p>
                <p className="font-inter text-[11px] text-[#C8C1B3] truncate">{m.email}</p>
              </div>
            </div>

            {/* Role */}
            <div>
              <RoleBadge role={m.role} />
            </div>

            {/* Joined date */}
            <span className="font-inter text-[11px] text-[#C8C1B3]">
              {new Date(m.joinedAt).toLocaleDateString('nb-NO')}
            </span>

            {/* Actions */}
            {!isSelf && m.role !== 'owner' && (onChangeRole || onRemove) ? (
              <div className="relative group">
                <button className="flex h-7 w-7 items-center justify-center text-[#C8C1B3] hover:text-[#2B2B2B]">
                  <MoreHorizontal size={14} strokeWidth={1.5} />
                </button>
                {/* Dropdown */}
                <div className="invisible absolute right-0 top-8 z-10 border border-[#D8D2C6] bg-[#F4F1EB] group-focus-within:visible min-w-[140px] shadow-sm">
                  {onChangeRole && m.role !== 'admin' && (
                    <button
                      onClick={() => onChangeRole(m.id, 'admin')}
                      className="block w-full px-4 py-2.5 text-left font-inter text-[12px] text-[#2B2B2B] hover:bg-[#EAE6DF]"
                    >
                      Gjør til admin
                    </button>
                  )}
                  {onChangeRole && m.role !== 'member' && (
                    <button
                      onClick={() => onChangeRole(m.id, 'member')}
                      className="block w-full px-4 py-2.5 text-left font-inter text-[12px] text-[#2B2B2B] hover:bg-[#EAE6DF]"
                    >
                      Sett som medlem
                    </button>
                  )}
                  {onRemove && (
                    <button
                      onClick={() => onRemove(m.id)}
                      className="block w-full border-t border-[#D8D2C6] px-4 py-2.5 text-left font-inter text-[12px] text-[#FF2E63] hover:bg-[#EAE6DF]"
                    >
                      Fjern fra team
                    </button>
                  )}
                </div>
              </div>
            ) : (
              <span />
            )}
          </div>
        );
      })}
    </div>
  );
}
