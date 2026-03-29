import { SidebarSectionPage } from '@/components/dashboard/SidebarSectionPage';
import { getSidebarSectionPage } from '@/components/dashboard/sidebar-sections';

export default function HelpdeskSectionPage({ params }: { params: { slug?: string[] } }) {
  return <SidebarSectionPage {...getSidebarSectionPage('helpdesk', params.slug)} />;
}