import { SidebarSectionPage } from '@/components/dashboard/SidebarSectionPage';
import { getSidebarSectionPage } from '@/components/dashboard/sidebar-sections';

export default function OutboundSectionPage({ params }: { params: { slug?: string[] } }) {
  return <SidebarSectionPage {...getSidebarSectionPage('outbound', params.slug)} />;
}