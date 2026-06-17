import { notFound, redirect } from 'next/navigation'

import {
  AccountLinkedAccountsPage,
  AccountNotificationsPage,
  AccountSecurityPage,
} from '@/components/account/AccountSectionPages'

type ProfileSectionPageProps = {
  params: Promise<{ section: string }>
}

export default async function ProfileSectionPage({ params }: ProfileSectionPageProps) {
  const { section } = await params

  if (section === 'profile') {
    redirect('/profile')
  }

  if (section === 'security') {
    return <AccountSecurityPage />
  }

  if (section === 'linked-accounts') {
    return <AccountLinkedAccountsPage />
  }

  if (section === 'notifications') {
    return <AccountNotificationsPage />
  }

  notFound()
}
