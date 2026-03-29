'use client'

import {
  User,
  Lock,
  Link2,
  Bell,
} from 'lucide-react'
import type { AccountNavSectionId } from './types'

export interface NavItem {
  id: AccountNavSectionId
  label: string
  icon: React.ComponentType<{ size?: number; strokeWidth?: number; className?: string }>
}

const NAV_ITEMS: NavItem[] = [
  { id: 'profile',         label: 'Profile',          icon: User },
  { id: 'security',        label: 'Security',         icon: Lock },
  { id: 'linked-accounts', label: 'Linked accounts',  icon: Link2 },
  { id: 'notifications',   label: 'Notifications',    icon: Bell },
]

interface AccountNavProps {
  active: AccountNavSectionId
  onSelect: (id: AccountNavSectionId) => void
}

export function AccountNav({ active, onSelect }: AccountNavProps) {
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
