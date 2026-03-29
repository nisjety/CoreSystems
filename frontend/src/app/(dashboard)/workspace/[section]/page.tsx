import { notFound, redirect } from 'next/navigation';
import { WorkspacePage, isWorkspaceSectionId } from '@/components/workspace';

type WorkspaceSectionPageProps = {
  params: Promise<{ section: string }>;
};

export default async function WorkspaceSectionPage({
  params,
}: WorkspaceSectionPageProps) {
  const { section } = await params;

  if (section === 'general') {
    redirect('/workspace');
  }

  if (!isWorkspaceSectionId(section)) {
    notFound();
  }

  return <WorkspacePage initialSection={section} basePath="/workspace" />;
}
