'use client'

import { useState } from 'react'
import { Check, Loader2 } from 'lucide-react'
import {
  useNotificationPreferences,
  useUpdateNotifications,
} from '../hooks/useAccount'
import type { NotificationPreferences } from '../types'

interface ToggleRowProps {
  label: string
  description: string
  checked: boolean
  onChange: (v: boolean) => void
}

function ToggleRow({ label, description, checked, onChange }: ToggleRowProps) {
  return (
    <li className="flex items-start justify-between gap-6 rounded-xl border border-[#E4E1DC] bg-white px-4 py-4">
      <div className="min-w-0 flex-1">
        <p className="font-inter text-[13px] font-medium text-[#1C1C1A]">{label}</p>
        <p className="mt-0.5 font-inter text-[12px] text-[#9B9691]">{description}</p>
      </div>
      {/* Toggle switch */}
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={[
          'relative mt-0.5 h-5 w-9 shrink-0 rounded-full transition-colors duration-200',
          checked ? 'bg-[#1C1C1A]' : 'bg-[#D8D2C6]',
        ].join(' ')}
      >
        <span
          className={[
            'absolute top-0.5 h-4 w-4 rounded-full bg-white shadow-sm transition-transform duration-200',
            checked ? 'translate-x-4' : 'translate-x-0.5',
          ].join(' ')}
        />
      </button>
    </li>
  )
}

const NOTIFICATION_CONFIG: {
  key: keyof NotificationPreferences
  label: string
  description: string
}[] = [
  {
    key: 'emailDigest',
    label: 'Weekly email digest',
    description: 'Receive a weekly summary of activity and highlights.',
  },
  {
    key: 'securityAlerts',
    label: 'Security alerts',
    description: 'Get notified about sign-ins from new devices or unusual activity.',
  },
  {
    key: 'productUpdates',
    label: 'Product updates',
    description: 'News about new features and improvements.',
  },
  {
    key: 'teamInvites',
    label: 'Team invitations',
    description: 'Notify me when I am invited to join an organisation.',
  },
  {
    key: 'mentionsOnly',
    label: 'Mentions only',
    description: 'Only notify me when I am directly mentioned in a conversation.',
  },
]

export function NotificationsSection() {
  const { data, isLoading } = useNotificationPreferences()
  const update = useUpdateNotifications()

  const [prefs, setPrefs] = useState<NotificationPreferences>({
    emailDigest: true,
    securityAlerts: true,
    productUpdates: false,
    teamInvites: true,
    mentionsOnly: false,
  })

  // Use server data if available
  const live = data ?? prefs

  const toggle = (key: keyof NotificationPreferences) => (v: boolean) => {
    const next = { ...live, [key]: v }
    setPrefs(next)
    update.mutate(next)
  }

  return (
    <section id="notifications" className="scroll-mt-6">
      <h2 className="mb-1.5 font-inter text-[22px] font-semibold tracking-[-0.02em] text-[#1C1C1A]">
        Notifications
      </h2>
      <p className="mb-6 font-inter text-[13px] text-[#9B9691]">
        Choose which updates you want to receive.
      </p>

      {isLoading ? (
        <div className="flex items-center gap-2 text-[#9B9691] font-inter text-[13px]">
          <Loader2 size={14} className="animate-spin" /> Loading…
        </div>
      ) : (
        <ul className="space-y-2">
          {NOTIFICATION_CONFIG.map(({ key, label, description }) => (
            <ToggleRow
              key={key}
              label={label}
              description={description}
              checked={live[key]}
              onChange={toggle(key)}
            />
          ))}
        </ul>
      )}

      {update.isSuccess && (
        <p className="mt-3 flex items-center gap-1.5 font-inter text-[12px] text-[#2E7D52]">
          <Check size={12} /> Preferences saved
        </p>
      )}
    </section>
  )
}
