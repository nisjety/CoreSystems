import type { SidebarSectionView } from './SidebarSectionPage';

type SidebarSectionKey =
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
    description: 'Home for queue health, recent issues, and the operating metrics that drive the day.',
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
    title: 'Agent Workspace',
    description: 'Configure Velion agents, knowledge sources, actions, tone, and handoff behavior.',
    views: [
      { id: 'agents', label: 'Agents', description: 'High-level agent administration and orchestration entry point.', href: '/agents' },
      { id: 'settings', label: 'Agent settings', description: 'Prompting, memory, tone, and policy configuration.', href: '/agents/settings' },
      { id: 'actions', label: 'Actions', description: 'The tools and workflows your agents can execute.', href: '/agents/actions' },
    ],
  },
  inbox: {
    eyebrow: 'Inbox',
    title: 'Conversation Workspace',
    description: 'The operator inbox for queue triage, live conversations, and shared customer context.',
    views: [
      { id: 'inbox', label: 'Inbox', description: 'Landing view for queue health and inbox workflows.', href: '/inbox' },
      { id: 'messages', label: 'Velion Chat', description: 'Direct AI chat for drafting, exploration, and non-queue work.', href: '/chat' },
      { id: 'chat-logs', label: 'Chat logs', description: 'Archived transcripts and historical conversations.', href: '/inbox/chat-logs' },
    ],
  },
  knowledge: {
    eyebrow: 'Knowledge',
    title: 'Knowledge Layer',
    description: 'The source-of-truth layer Velion uses to retrieve, answer, train, and validate.',
    views: [
      { id: 'documents', label: 'Documents', description: 'Files, folders, and indexed knowledge objects stored for the organization.', href: '/knowledge/documents' },
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
    eyebrow: 'Automations',
    title: 'Automation workspace',
    description: 'Rules, triggers, macros, routing, and escalation logic for support operations.',
    views: [
      { id: 'outbound', label: 'Automations', description: 'Landing page for routing logic, macros, and rule configuration.', href: '/outbound' },
      { id: 'campaigns', label: 'Automation rules', description: 'Rule-level automation management and rollout control.', href: '/outbound/campaigns', status: 'coming-soon' },
    ],
  },
  people: {
    eyebrow: 'Contacts',
    title: 'Contacts Workspace',
    description: 'Manage customer profiles, accounts, linked conversations, teams, and ownership.',
    views: [
      { id: 'people', label: 'Contacts', description: 'Landing page for customer and account operations.', href: '/people' },
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
    description: 'Ticketing, macros, escalations, tasks, and operator controls kept separate from the live inbox.',
    views: [
      { id: 'helpdesk', label: 'Helpdesk', description: 'Landing page for ticketing operations, task flow, and queue controls.', href: '/helpdesk' },
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
