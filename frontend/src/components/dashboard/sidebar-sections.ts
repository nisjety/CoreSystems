import type { SidebarSectionView } from './SidebarSectionPage';

export type SidebarSectionKey =
  | 'overview'
  | 'agents'
  | 'inbox'
  | 'knowledge'
  | 'reports'
  | 'outbound'
  | 'people'
  | 'deployment'
  | 'helpdesk';

type SidebarSectionDefinition = {
  eyebrow: string;
  title: string;
  description: string;
  views: SidebarSectionView[];
};

export const sidebarSections: Record<SidebarSectionKey, SidebarSectionDefinition> = {
  overview: {
    eyebrow: 'Overview',
    title: 'Overview Workspace',
    description: 'A single place for activity, recents, and lead movement across Aqencia.',
    views: [
      { id: 'overview', label: 'Overview', description: 'Section landing for operating snapshots and summaries.', href: '/overview' },
      { id: 'activity', label: 'Activity', description: 'Recent changes and operational movement across the system.', href: '/overview/activity' },
      { id: 'recents', label: 'Recents', description: 'The work, records, and conversations you touched most recently.', href: '/overview/recents' },
      { id: 'leads', label: 'Leads', description: 'New opportunities and lead flow needing attention.', href: '/overview/leads' },
      { id: 'shared-spaces', label: 'Shared spaces', description: 'Cross-functional shared views for teams and collaborators.', href: '/overview/shared-spaces', status: 'coming-soon' },
    ],
  },
  agents: {
    eyebrow: 'Agents',
    title: 'Agent Control Center',
    description: 'Configure Aqencia agents, behavior, tools, and execution boundaries.',
    views: [
      { id: 'agents', label: 'Agents', description: 'High-level agent administration and orchestration entry point.', href: '/agents' },
      { id: 'settings', label: 'Agent settings', description: 'Prompting, memory, tone, and policy configuration.', href: '/agents/settings' },
      { id: 'actions', label: 'Actions', description: 'The tools and workflows your agents can execute.', href: '/agents/actions' },
    ],
  },
  inbox: {
    eyebrow: 'Inbox',
    title: 'Conversation Inbox',
    description: 'The operational inbox for live messages, transcripts, and escalation routing.',
    views: [
      { id: 'inbox', label: 'Inbox', description: 'Landing view for queue health and inbox workflows.', href: '/inbox' },
      { id: 'messages', label: 'Messages', description: 'Real-time conversations and handoff-ready chats.', href: '/chat' },
      { id: 'chat-logs', label: 'Chat logs', description: 'Archived transcripts and historical conversations.', href: '/inbox/chat-logs' },
    ],
  },
  knowledge: {
    eyebrow: 'Knowledge',
    title: 'Knowledge Layer',
    description: 'The data foundation Aqencia uses to reason, retrieve, and train.',
    views: [
      { id: 'data', label: 'Data', description: 'Structured records, datasets, and indexed knowledge objects.', href: '/knowledge/data' },
      { id: 'sources', label: 'Sources', description: 'Connected websites, repositories, and external inputs.', href: '/knowledge/sources' },
      { id: 'api-integrations', label: 'API integrations', description: 'Live application connectors and API-backed context.', href: '/knowledge/api-integrations' },
      { id: 'training', label: 'Training', description: 'Review loops, correction sets, and knowledge refinement.', href: '/knowledge/training' },
    ],
  },
  reports: {
    eyebrow: 'Reports',
    title: 'Reporting and Insights',
    description: 'Analyze system performance and surface actionable patterns across channels.',
    views: [
      { id: 'reports', label: 'Reports', description: 'Landing page for analytics and insight workflows.', href: '/reports' },
      { id: 'analytics', label: 'Analytics', description: 'Operational metrics, throughput, and funnel views.', href: '/reports/analytics' },
      { id: 'insights', label: 'Insights', description: 'AI-synthesized findings and optimization opportunities.', href: '/reports/insights' },
    ],
  },
  outbound: {
    eyebrow: 'Outbound',
    title: 'Outbound Programs',
    description: 'Coordinate outbound sequences, nurture campaigns, and follow-up automation.',
    views: [
      { id: 'outbound', label: 'Outbound', description: 'Landing page for outbound operations and campaign planning.', href: '/outbound' },
      { id: 'campaigns', label: 'Campaigns', description: 'Program-level outbound launches and experiments.', href: '/outbound/campaigns', status: 'coming-soon' },
    ],
  },
  people: {
    eyebrow: 'People',
    title: 'People Workspace',
    description: 'Manage contacts, internal teams, and lead ownership from one surface.',
    views: [
      { id: 'people', label: 'People', description: 'Landing page for relationship and team operations.', href: '/people' },
      { id: 'contacts', label: 'Contacts', description: 'Customer and partner records with ownership context.', href: '/people/contacts' },
      { id: 'teams', label: 'Teams', description: 'Internal members, roles, and collaboration structure.', href: '/team' },
      { id: 'lead', label: 'Lead', description: 'Lead routing, qualification, and movement across owners.', href: '/people/lead' },
    ],
  },
  deployment: {
    eyebrow: 'Deployment',
    title: 'Deployment Console',
    description: 'Track release readiness, deployment controls, and rollout status.',
    views: [
      { id: 'deployment', label: 'Deployment', description: 'Landing page for deployment readiness and releases.', href: '/deployment' },
      { id: 'checklist', label: 'Release checklist', description: 'Structured checks before shipping changes safely.', href: '/deployment/checklist', status: 'coming-soon' },
    ],
  },
  helpdesk: {
    eyebrow: 'Helpdesk',
    title: 'Helpdesk Operations',
    description: 'Support playbooks, escalations, and operator tools in one control surface.',
    views: [
      { id: 'helpdesk', label: 'Helpdesk', description: 'Landing page for support operations and routing.', href: '/helpdesk' },
      { id: 'knowledge-hub', label: 'Knowledge hub', description: 'Support answers and documentation handoff.', href: '/answers' },
      { id: 'macros', label: 'Macros', description: 'Reusable responses and shortcut bundles.', href: '/helpdesk/macros', status: 'coming-soon' },
      { id: 'escalations', label: 'Escalations', description: 'Urgent queue management and SLA ownership.', href: '/helpdesk/escalations', status: 'coming-soon' },
    ],
  },
};

export function getSidebarSectionPage(sectionKey: SidebarSectionKey, slug?: string[]) {
  const section = sidebarSections[sectionKey];

  if (!section) {
    throw new Error(`Unknown sidebar section: ${sectionKey}`);
  }

  const activeId = slug?.[0] ?? section.views[0].id;
  const currentView = section.views.find((view) => view.id === activeId) ?? section.views[0];

  return {
    eyebrow: section.eyebrow,
    title: section.title,
    description: section.description,
    currentView,
    views: section.views,
  };
}