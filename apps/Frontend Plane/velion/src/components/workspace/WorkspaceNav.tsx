'use client'

import { Building2, Users, CreditCard } from 'lucide-react'
import type { WorkspaceSectionId } from './types'

const NAV_ITEMS: { id: WorkspaceSectionId; label: string; icon: React.ComponentType<{ size?: number; strokeWidth?: number; className?: string }> }[] = [
  { id: 'general',      label: 'General',      icon: Building2  },
  { id: 'members',      label: 'Members',      icon: Users      },
  { id: 'billing',      label: 'Billing',      icon: CreditCard },
]

interface WorkspaceNavProps {
  active: WorkspaceSectionId
  onSelect: (id: WorkspaceSectionId) => void
}

export function WorkspaceNav({ active, onSelect }: WorkspaceNavProps) {
  return (
    <nav className="flex flex-col gap-0.5">
      {NAV_ITEMS.map(({ id, label, icon: Icon }) => {
        const isActive = active === id
        return (
          <button
            key={id}
            onClick={() => onSelect(id)}
            className={[
              'group flex w-full items-center gap-2.5 rounded-xl px-3 py-2.5 text-left',
              'font-inter text-[13px] transition-colors duration-150',
              isActive
                ? 'bg-[#1C1C1A] text-white'
                : 'text-[#4A4A48] hover:bg-[#EAE6DF] hover:text-[#1C1C1A]',
            ].join(' ')}
          >
            <Icon
              size={14}
              strokeWidth={1.6}
              className={[
                'shrink-0',
                isActive ? 'text-white' : 'text-[#9B9691] group-hover:text-[#1C1C1A]',
              ].join(' ')}
            />
            {label}
          </button>
        )
      })}
    </nav>
  )
}
