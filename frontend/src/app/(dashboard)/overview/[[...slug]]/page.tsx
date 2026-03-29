import { SidebarSectionPage } from '@/components/dashboard/SidebarSectionPage';
import { getSidebarSectionPage } from '@/components/dashboard/sidebar-sections';

export default function OverviewSectionPage({ params }: { params: { slug?: string[] } }) {
  return <SidebarSectionPage {...getSidebarSectionPage('overview', params.slug)} />;
}