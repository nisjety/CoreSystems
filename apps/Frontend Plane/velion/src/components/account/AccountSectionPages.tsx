'use client'

import type { ReactNode } from 'react'

import { DangerZoneSection } from './sections/DangerZoneSection'
import { LinkedAccountsSection } from './sections/LinkedAccountsSection'
import { NotificationsSection } from './sections/NotificationsSection'
import { ProfileSection } from './sections/ProfileSection'
import { SecuritySection } from './sections/SecuritySection'

function AccountSurface({
  title,
  children,
}: {
  title: string
  description: string
  children: ReactNode
}) {
  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto bg-white">
      <div className="mx-auto w-full max-w-[580px] px-6 py-10">
        <h1 className="mb-10 text-[22px] font-semibold tracking-tight text-[#111111]">
          {title}
        </h1>
        <div className="space-y-10">
          {children}
        </div>
      </div>
    </div>
  )
}

export function AccountProfilePage() {
  return (
    <AccountSurface
      title="Account settings"
      description="Refine your identity, security, sign-in methods, and notification preferences from one calm workspace."
    >
      <ProfileSection />
      <SecuritySection />
      <LinkedAccountsSection />
      <NotificationsSection />
      <DangerZoneSection />
    </AccountSurface>
  )
}

export function AccountSecurityPage() {
  return (
    <AccountSurface
      title="Security"
      description="Manage your password, strengthen sign-in protection, and keep your Verevon identity under control."
    >
      <SecuritySection />
    </AccountSurface>
  )
}

export function AccountLinkedAccountsPage() {
  return (
    <AccountSurface
      title="Sign-in methods"
      description="Review the providers attached to your Verevon identity and keep workspace integrations separate from authentication."
    >
      <LinkedAccountsSection />
    </AccountSurface>
  )
}

export function AccountNotificationsPage() {
  return (
    <AccountSurface
      title="Notifications"
      description="Choose which product and security signals reach you so the account surface stays useful instead of noisy."
    >
      <NotificationsSection />
    </AccountSurface>
  )
}
