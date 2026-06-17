import { AgentsView } from '@/components/agents/AgentsView';
import { SidebarSectionPage } from '@/components/dashboard/SidebarSectionPage';
import { getSidebarSectionPage } from '@/components/dashboard/sidebar-sections';

// Slugs that map to SidebarSectionPage sub-views
const AGENT_SECTION_SLUGS = new Set(['settings', 'actions']);

export default async function AgentsSectionPage({
  params,
}: {
  params: Promise<{ slug?: string[] }>;
}) {
  const { slug } = await params;
  const section = slug?.[0];

  if (section && AGENT_SECTION_SLUGS.has(section)) {
    return <SidebarSectionPage {...getSidebarSectionPage('agents', slug)} />;
  }

  return <AgentsView />;
}