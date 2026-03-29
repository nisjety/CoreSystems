import { SidebarSectionPage } from '@/components/dashboard/SidebarSectionPage';
import { getSidebarSectionPage } from '@/components/dashboard/sidebar-sections';

export default function ReportsSectionPage({ params }: { params: { slug?: string[] } }) {
  return <SidebarSectionPage {...getSidebarSectionPage('reports', params.slug)} />;
}