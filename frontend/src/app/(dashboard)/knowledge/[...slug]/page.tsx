import { SidebarSectionPage } from '@/components/dashboard/SidebarSectionPage';
import { getSidebarSectionPage } from '@/components/dashboard/sidebar-sections';

export default function KnowledgeSectionPage({ params }: { params: { slug: string[] } }) {
  return <SidebarSectionPage {...getSidebarSectionPage('knowledge', params.slug)} />;
}