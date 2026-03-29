import { notFound, redirect } from 'next/navigation';
import { AccountPage, isAccountNavSectionId } from '@/components/account';

type SettingsSectionPageProps = {
  params: Promise<{ section: string }>;
};

export default async function SettingsSectionPage({ params }: SettingsSectionPageProps) {
  const { section } = await params;

  if (section === 'profile') {
    redirect('/settings');
  }

  if (!isAccountNavSectionId(section)) {
    notFound();
  }

  return <AccountPage initialSection={section} basePath="/settings" />;
}
