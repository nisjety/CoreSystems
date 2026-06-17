import { notFound, redirect } from 'next/navigation'

type SettingsSectionPageProps = {
  params: Promise<{ section: string }>
}

export default async function SettingsSectionPage({ params }: SettingsSectionPageProps) {
  const { section } = await params

  if (section === 'profile') {
    redirect('/profile')
  }

  if (section === 'linked-accounts') {
    redirect('/profile/linked-accounts')
  }

  if (section === 'security') {
    redirect('/settings/security')
  }

  if (section === 'notifications') {
    redirect('/settings/notifications')
  }

  notFound()
}
