import { redirect } from 'next/navigation';

type ProfileSectionPageProps = {
  params: Promise<{ section: string }>;
};

export default async function ProfileSectionPage({ params }: ProfileSectionPageProps) {
  const { section } = await params;

  redirect(section === 'profile' ? '/settings' : `/settings/${section}`);
}
