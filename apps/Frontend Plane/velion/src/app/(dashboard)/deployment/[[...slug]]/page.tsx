import { SidebarSectionPage } from '@/components/dashboard/SidebarSectionPage';
import { getSidebarSectionPage } from '@/components/dashboard/sidebar-sections';

export default function DeploymentSectionPage({ params }: { params: { slug?: string[] } }) {
  return <SidebarSectionPage {...getSidebarSectionPage('deployment', params.slug)} />;
}